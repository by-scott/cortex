use cortex_types::config::FrameAuditConfig;

use super::adaptive::AdaptiveThresholds;
use super::doom_loop::DoomLoopDetector;
use super::fatigue::FatigueAccumulator;
use super::frame_audit::{FrameAuditDetector, FrameRiskLevel};
use super::rpe::ToolUtilityTracker;
use std::time::Instant;

pub struct MetaMonitor {
    pub doom_loop: DoomLoopDetector,
    pub fatigue: FatigueAccumulator,
    pub rpe: ToolUtilityTracker,
    pub frame_audit: FrameAuditDetector,
    pub adaptive: AdaptiveThresholds,
    turn_start: Option<Instant>,
    duration_limit_secs: u64,
}

pub struct MetaAlert {
    pub kind: AlertKind,
    pub message: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AlertKind {
    DoomLoop,
    Duration,
    Fatigue,
    FrameAnchoring,
    HealthDegraded,
}

impl MetaMonitor {
    #[must_use]
    pub fn new(
        doom_loop_threshold: usize,
        fatigue_threshold: f64,
        duration_limit_secs: u64,
        frame_anchoring_threshold: f64,
        frame_audit_config: FrameAuditConfig,
    ) -> Self {
        Self {
            doom_loop: DoomLoopDetector::new(doom_loop_threshold),
            fatigue: FatigueAccumulator::new(fatigue_threshold),
            rpe: ToolUtilityTracker::new(),
            frame_audit: FrameAuditDetector::new(frame_audit_config),
            adaptive: AdaptiveThresholds::new(
                f64::from(u32::try_from(doom_loop_threshold).unwrap_or(u32::MAX)),
                fatigue_threshold,
                frame_anchoring_threshold,
            ),
            turn_start: None,
            duration_limit_secs,
        }
    }

    pub fn start_turn(&mut self) {
        self.turn_start = Some(Instant::now());
        self.doom_loop.reset();
    }

    pub fn end_turn(&mut self, complexity: f64) {
        self.fatigue.accumulate(complexity);
        self.frame_audit.advance_turn();
        self.turn_start = None;
    }

    pub fn record_tool_call(&mut self, tool_name: &str, input: &str) {
        self.doom_loop.record_tool_call(tool_name, input);
        self.frame_audit.record_tool_call(tool_name);
    }

    pub fn record_tool_result(&mut self, tool_name: &str, success: bool, output: &str) {
        self.doom_loop.record_tool_result(output);
        self.rpe.record(tool_name, if success { 1.0 } else { 0.0 });
        self.frame_audit.record_tool_result(success);
    }

    pub fn record_goal_state(&mut self, goal: &str) {
        self.frame_audit.record_goal_state(goal);
    }

    pub const fn record_user_correction(&mut self) {
        self.frame_audit.record_user_correction();
    }

    /// Record the outcome of an alert for adaptive threshold adjustment.
    /// `is_true_positive`: true if the alert led to a strategy change, false if it was a false alarm.
    pub fn record_alert_outcome(&mut self, kind: &AlertKind, is_true_positive: bool) {
        self.adaptive.record_outcome(kind, is_true_positive);
        // Apply updated thresholds
        self.doom_loop = DoomLoopDetector::new(self.adaptive.effective_doom_loop_threshold());
        self.fatigue = FatigueAccumulator::new(self.adaptive.effective_fatigue_threshold());
    }

    /// Check all detectors, return any alerts.
    /// Backward-compatible wrapper using default confidence.
    #[must_use]
    pub fn check(&self) -> Vec<MetaAlert> {
        self.check_with_confidence(0.5)
    }

    /// Check all detectors with confidence score for frame audit integration.
    #[must_use]
    pub fn check_with_confidence(&self, confidence_score: f64) -> Vec<MetaAlert> {
        let mut alerts = Vec::new();

        if let Some(msg) = self.doom_loop.check() {
            alerts.push(MetaAlert {
                kind: AlertKind::DoomLoop,
                message: msg,
            });
        }

        if let Some(start) = self.turn_start
            && start.elapsed().as_secs() > self.duration_limit_secs
        {
            alerts.push(MetaAlert {
                kind: AlertKind::Duration,
                message: format!(
                    "turn duration {}s exceeds limit {}s",
                    start.elapsed().as_secs(),
                    self.duration_limit_secs
                ),
            });
        }

        if self.fatigue.should_rest() {
            alerts.push(MetaAlert {
                kind: AlertKind::Fatigue,
                message: format!(
                    "fatigue level {:.2} exceeds threshold",
                    self.fatigue.value()
                ),
            });
        }

        // Frame anchoring check -- alert on Medium or High risk
        if let Some(frame_event) = self.frame_audit.check(confidence_score)
            && matches!(
                frame_event.level,
                FrameRiskLevel::Medium | FrameRiskLevel::High
            )
        {
            alerts.push(MetaAlert {
                kind: AlertKind::FrameAnchoring,
                message: format!(
                    "frame anchoring {} (score {:.2}): signals [{}]",
                    frame_event.level,
                    frame_event.risk_score,
                    frame_event.signals.join(", ")
                ),
            });
        }

        alerts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_monitor(
        doom_loop_threshold: usize,
        fatigue_threshold: f64,
        duration_limit_secs: u64,
    ) -> MetaMonitor {
        MetaMonitor::new(
            doom_loop_threshold,
            fatigue_threshold,
            duration_limit_secs,
            0.5,
            FrameAuditConfig::default(),
        )
    }

    #[test]
    fn monitor_no_alerts_initially() {
        let monitor = default_monitor(3, 0.8, 300);
        assert!(monitor.check().is_empty());
    }

    #[test]
    fn monitor_doom_loop_alert() {
        let mut monitor = default_monitor(3, 0.8, 300);
        monitor.start_turn();
        monitor.record_tool_call("read", "same");
        monitor.record_tool_call("read", "same");
        monitor.record_tool_call("read", "same");

        let alerts = monitor.check();
        assert!(alerts.iter().any(|a| a.kind == AlertKind::DoomLoop));
    }

    #[test]
    fn monitor_end_turn_accumulates_fatigue() {
        let mut monitor = default_monitor(3, 0.5, 300);
        monitor.start_turn();
        monitor.end_turn(0.3);
        monitor.start_turn();
        monitor.end_turn(0.3);
        monitor.start_turn();
        monitor.end_turn(0.3);

        let alerts = monitor.check();
        assert!(alerts.iter().any(|a| a.kind == AlertKind::Fatigue));
    }

    #[test]
    fn monitor_records_tool_results() {
        let mut monitor = default_monitor(3, 0.8, 300);
        monitor.record_tool_result("read", true, "output1");
        monitor.record_tool_result("read", true, "output2");
        assert!(monitor.rpe.utility("read") > 0.5);
    }

    #[test]
    fn monitor_frame_anchoring_alert_at_high_risk() {
        let mut monitor = default_monitor(3, 0.8, 300);

        // Goal stagnation
        for _ in 0..5 {
            monitor.record_goal_state("same goal");
        }

        // Tool monotony
        for _ in 0..8 {
            monitor.record_tool_call("read", "same");
        }
        monitor.record_tool_call("write", "a");
        monitor.record_tool_call("bash", "b");

        // Corrections
        for _ in 0..3 {
            monitor.record_user_correction();
        }

        let alerts = monitor.check();
        assert!(alerts.iter().any(|a| a.kind == AlertKind::FrameAnchoring));
    }

    #[test]
    fn monitor_no_frame_alert_at_low_risk() {
        let monitor = default_monitor(3, 0.8, 300);
        let alerts = monitor.check();
        assert!(!alerts.iter().any(|a| a.kind == AlertKind::FrameAnchoring));
    }

    #[test]
    fn monitor_adaptive_threshold_relaxes_on_false_positives() {
        let mut monitor = default_monitor(3, 0.8, 300);
        let initial_fatigue = monitor.adaptive.effective_fatigue_threshold();
        // Record 10 false positive fatigue alerts
        for _ in 0..10 {
            monitor.record_alert_outcome(&AlertKind::Fatigue, false);
        }
        // Fatigue threshold should have increased (relaxed)
        assert!(
            monitor.adaptive.effective_fatigue_threshold() > initial_fatigue,
            "fatigue threshold should relax: {} > {}",
            monitor.adaptive.effective_fatigue_threshold(),
            initial_fatigue
        );
    }

    #[test]
    fn monitor_frame_record_methods_forwarded() {
        let mut monitor = default_monitor(3, 0.8, 300);

        // Tool calls forwarded to frame audit
        for _ in 0..8 {
            monitor.record_tool_call("read", "same");
        }
        monitor.record_tool_call("write", "a");
        monitor.record_tool_call("bash", "b");

        // Check frame audit detected tool monotony
        let frame_result = monitor.frame_audit.check(0.5);
        assert!(frame_result.is_some());
        assert!(
            frame_result
                .unwrap()
                .signals
                .contains(&"tool_monotony".to_string())
        );
    }
}
