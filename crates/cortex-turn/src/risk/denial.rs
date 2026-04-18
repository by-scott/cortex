use cortex_types::config::DenialConfig;

/// Tracks permission denial patterns to detect persistent blocks.
pub struct DenialTracker {
    consecutive_denials: usize,
    total_denials: usize,
    config: DenialConfig,
}

impl Default for DenialTracker {
    fn default() -> Self {
        Self::new(DenialConfig::default())
    }
}

impl DenialTracker {
    #[must_use]
    pub const fn new(config: DenialConfig) -> Self {
        Self {
            consecutive_denials: 0,
            total_denials: 0,
            config,
        }
    }

    pub const fn record_denial(&mut self) {
        self.consecutive_denials += 1;
        self.total_denials += 1;
    }

    pub const fn record_approval(&mut self) {
        self.consecutive_denials = 0;
    }

    /// Pause suggested after N consecutive denials.
    #[must_use]
    pub const fn should_pause(&self) -> bool {
        self.consecutive_denials >= self.config.consecutive_threshold
    }

    /// Escalation suggested after total denials exceed threshold.
    #[must_use]
    pub const fn should_escalate(&self) -> bool {
        self.total_denials >= self.config.session_threshold
    }

    #[must_use]
    pub const fn consecutive_denials(&self) -> usize {
        self.consecutive_denials
    }

    #[must_use]
    pub const fn total_denials(&self) -> usize {
        self.total_denials
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_state() {
        let t = DenialTracker::default();
        assert_eq!(t.consecutive_denials(), 0);
        assert_eq!(t.total_denials(), 0);
        assert!(!t.should_pause());
        assert!(!t.should_escalate());
    }

    #[test]
    fn three_consecutive_pauses() {
        let mut t = DenialTracker::default();
        for _ in 0..3 {
            t.record_denial();
        }
        assert!(t.should_pause());
    }

    #[test]
    fn approval_resets_consecutive() {
        let mut t = DenialTracker::default();
        t.record_denial();
        t.record_denial();
        t.record_approval();
        assert_eq!(t.consecutive_denials(), 0);
        assert_eq!(t.total_denials(), 2);
    }

    #[test]
    fn ten_total_escalates() {
        let mut t = DenialTracker::default();
        for _ in 0..10 {
            t.record_denial();
            t.record_approval();
        }
        assert!(t.should_escalate());
        assert!(!t.should_pause());
    }
}
