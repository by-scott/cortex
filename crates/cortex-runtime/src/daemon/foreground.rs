use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundSlotError {
    ShuttingDown,
    Timeout,
}

impl ForegroundSlotError {
    pub const fn operator_detail(self) -> &'static str {
        match self {
            Self::ShuttingDown => "service shutting down",
            Self::Timeout => "another turn is in progress -- timed out after 30s",
        }
    }

    pub const fn user_message(self) -> &'static str {
        match self {
            Self::ShuttingDown => "Turn queue unavailable.",
            Self::Timeout => "Another turn is in progress. Please wait.",
        }
    }
}

/// RAII guard that marks the foreground runtime as busy for the duration of an
/// active foreground execution.
struct ForegroundActivity {
    state: Arc<crate::heartbeat::HeartbeatState>,
    active: bool,
}

impl ForegroundActivity {
    fn acquire(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        state
            .foreground_busy
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            state: Arc::clone(state),
            active: true,
        }
    }

    fn finish(&mut self) {
        if !self.active {
            return;
        }
        self.state
            .foreground_busy
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.state.touch();
        self.active = false;
    }
}

impl Drop for ForegroundActivity {
    fn drop(&mut self) {
        self.finish();
    }
}

/// Unified foreground execution scope that keeps queue ownership and heartbeat
/// busy-state aligned for the lifetime of one user-visible turn.
pub struct ForegroundExecution<'a> {
    _permit: Option<tokio::sync::SemaphorePermit<'a>>,
    activity: ForegroundActivity,
}

impl<'a> ForegroundExecution<'a> {
    pub fn queued(
        permit: tokio::sync::SemaphorePermit<'a>,
        state: &Arc<crate::heartbeat::HeartbeatState>,
    ) -> Self {
        Self {
            _permit: Some(permit),
            activity: ForegroundActivity::acquire(state),
        }
    }

    pub fn immediate(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        Self {
            _permit: None,
            activity: ForegroundActivity::acquire(state),
        }
    }

    pub fn finish_visible(&mut self) {
        self.activity.finish();
    }
}
