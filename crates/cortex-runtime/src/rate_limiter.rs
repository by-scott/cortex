use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

/// Sliding window rate limiter for per-session and global request limiting.
pub struct RateLimiter {
    per_session_rpm: usize,
    global_rpm: usize,
    session_windows: Mutex<HashMap<String, WindowCounter>>,
    global_window: Mutex<WindowCounter>,
}

struct WindowCounter {
    timestamps: Vec<Instant>,
}

impl WindowCounter {
    const fn new() -> Self {
        Self {
            timestamps: Vec::new(),
        }
    }

    fn count_in_window(&mut self, window_secs: u64) -> usize {
        let cutoff = Instant::now()
            .checked_sub(std::time::Duration::from_secs(window_secs))
            .unwrap_or_else(Instant::now);
        self.timestamps.retain(|t| *t >= cutoff);
        self.timestamps.len()
    }

    /// Check if under limit and record if so. Returns `true` if allowed.
    fn check_and_record(&mut self, window_secs: u64, limit: usize) -> bool {
        if self.count_in_window(window_secs) >= limit {
            return false;
        }
        self.record();
        true
    }

    fn record(&mut self) {
        self.timestamps.push(Instant::now());
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitResult {
    Allowed,
    SessionLimited,
    GlobalLimited,
}

impl RateLimiter {
    #[must_use]
    pub fn new(per_session_rpm: usize, global_rpm: usize) -> Self {
        Self {
            per_session_rpm,
            global_rpm,
            session_windows: Mutex::new(HashMap::new()),
            global_window: Mutex::new(WindowCounter::new()),
        }
    }

    /// Check and record a request. Returns whether it's allowed.
    #[must_use]
    pub fn check(&self, session_id: &str) -> RateLimitResult {
        // Global check
        {
            let mut global = self
                .global_window
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if global.count_in_window(60) >= self.global_rpm {
                return RateLimitResult::GlobalLimited;
            }
        }

        // Per-session check + record
        if !check_and_record_session(&self.session_windows, session_id, self.per_session_rpm) {
            return RateLimitResult::SessionLimited;
        }

        // Record global
        self.global_window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record();

        RateLimitResult::Allowed
    }

    /// Check if a request would be allowed without recording it.
    #[must_use]
    pub fn would_allow(&self, session_id: &str) -> RateLimitResult {
        if self
            .global_window
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .count_in_window(60)
            >= self.global_rpm
        {
            return RateLimitResult::GlobalLimited;
        }
        if check_session_limit(&self.session_windows, session_id, self.per_session_rpm) {
            return RateLimitResult::SessionLimited;
        }
        RateLimitResult::Allowed
    }
}

/// Check per-session rate limit and record if allowed. Returns `true` if allowed.
fn check_and_record_session(
    session_windows: &std::sync::Mutex<std::collections::HashMap<String, WindowCounter>>,
    session_id: &str,
    per_session_rpm: usize,
) -> bool {
    session_windows
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(session_id.to_string())
        .or_insert_with(WindowCounter::new)
        .check_and_record(60, per_session_rpm)
}

/// Check if a session would be rate-limited (without recording).
fn check_session_limit(
    session_windows: &std::sync::Mutex<std::collections::HashMap<String, WindowCounter>>,
    session_id: &str,
    per_session_rpm: usize,
) -> bool {
    session_windows
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .entry(session_id.to_string())
        .or_insert_with(WindowCounter::new)
        .count_in_window(60)
        >= per_session_rpm
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allows_under_limit() {
        let limiter = RateLimiter::new(10, 60);
        assert_eq!(limiter.check("s1"), RateLimitResult::Allowed);
    }

    #[test]
    fn session_limited() {
        let limiter = RateLimiter::new(2, 100);
        assert_eq!(limiter.check("s1"), RateLimitResult::Allowed);
        assert_eq!(limiter.check("s1"), RateLimitResult::Allowed);
        assert_eq!(limiter.check("s1"), RateLimitResult::SessionLimited);
        // Different session still allowed
        assert_eq!(limiter.check("s2"), RateLimitResult::Allowed);
    }

    #[test]
    fn global_limited() {
        let limiter = RateLimiter::new(100, 2);
        assert_eq!(limiter.check("s1"), RateLimitResult::Allowed);
        assert_eq!(limiter.check("s2"), RateLimitResult::Allowed);
        assert_eq!(limiter.check("s3"), RateLimitResult::GlobalLimited);
    }

    #[test]
    fn would_allow_doesnt_count() {
        let limiter = RateLimiter::new(2, 100);
        assert_eq!(limiter.would_allow("s1"), RateLimitResult::Allowed);
        assert_eq!(limiter.would_allow("s1"), RateLimitResult::Allowed);
        // Still allowed because would_allow doesn't record
        assert_eq!(limiter.check("s1"), RateLimitResult::Allowed);
    }
}
