use std::collections::VecDeque;

use cortex_types::config::FrameAuditConfig;

const TOOL_WINDOW: usize = 10;
const TURN_WINDOW: usize = 10;

/// Result of a frame audit check.
#[derive(Debug, Clone)]
pub struct FrameCheckEvent {
    pub signals: Vec<String>,
    pub level: FrameRiskLevel,
    pub risk_score: f64,
    pub confidence_score: f64,
}

/// Four-level frame anchoring risk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameRiskLevel {
    None,
    Low,
    Medium,
    High,
}

impl FrameRiskLevel {
    #[must_use]
    pub const fn from_score(score: f64) -> Self {
        if score >= 0.7 {
            Self::High
        } else if score >= 0.4 {
            Self::Medium
        } else if score >= 0.2 {
            Self::Low
        } else {
            Self::None
        }
    }

    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
        }
    }
}

impl std::fmt::Display for FrameRiskLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Detects cognitive frame anchoring from observable signals.
pub struct FrameAuditDetector {
    recent_tools: VecDeque<String>,
    goal_history: VecDeque<String>,
    correction_count: usize,
    turn_count_for_corrections: usize,
    failure_streak: usize,
    config: FrameAuditConfig,
}

impl FrameAuditDetector {
    #[must_use]
    pub fn new(config: FrameAuditConfig) -> Self {
        let goal_cap = config.goal_stagnation_threshold;
        Self {
            recent_tools: VecDeque::with_capacity(TOOL_WINDOW + 1),
            goal_history: VecDeque::with_capacity(goal_cap + 1),
            correction_count: 0,
            turn_count_for_corrections: 0,
            failure_streak: 0,
            config,
        }
    }

    /// Record a tool call name into the sliding window.
    pub fn record_tool_call(&mut self, tool_name: &str) {
        self.recent_tools.push_back(tool_name.to_string());
        if self.recent_tools.len() > TOOL_WINDOW {
            self.recent_tools.pop_front();
        }
    }

    /// Record a tool execution result.
    pub const fn record_tool_result(&mut self, success: bool) {
        if success {
            self.failure_streak = 0;
        } else {
            self.failure_streak += 1;
        }
    }

    /// Record current goal state for stagnation detection.
    pub fn record_goal_state(&mut self, goal: &str) {
        self.goal_history.push_back(goal.to_string());
        if self.goal_history.len() > self.config.goal_stagnation_threshold {
            self.goal_history.pop_front();
        }
    }

    /// Record a user correction event.
    pub const fn record_user_correction(&mut self) {
        self.correction_count += 1;
        self.turn_count_for_corrections += 1;
        // Age out corrections beyond the window
        while self.turn_count_for_corrections > TURN_WINDOW && self.correction_count > 0 {
            self.correction_count -= 1;
            self.turn_count_for_corrections -= 1;
        }
    }

    /// Advance the turn counter for correction window aging.
    pub const fn advance_turn(&mut self) {
        self.turn_count_for_corrections += 1;
        if self.turn_count_for_corrections > TURN_WINDOW {
            self.turn_count_for_corrections = TURN_WINDOW;
            if self.correction_count > 0 {
                self.correction_count -= 1;
            }
        }
    }

    /// Run frame audit check. Returns `None` if no signals detected (risk level None).
    #[must_use]
    pub fn check(&self, confidence_score: f64) -> Option<FrameCheckEvent> {
        let mut signals = Vec::new();
        let mut weighted_score: f64 = 0.0;

        weighted_score = self.collect_goal_stagnation(&mut signals, weighted_score);
        weighted_score = self.collect_tool_monotony(&mut signals, weighted_score);
        weighted_score = self.collect_correction_frequency(&mut signals, weighted_score);
        weighted_score =
            self.collect_low_confidence(&mut signals, weighted_score, confidence_score);
        weighted_score = self.collect_failure_streak(&mut signals, weighted_score);

        let level = FrameRiskLevel::from_score(weighted_score);
        if level == FrameRiskLevel::None {
            return None;
        }

        Some(FrameCheckEvent {
            signals,
            level,
            risk_score: weighted_score,
            confidence_score,
        })
    }

    fn collect_goal_stagnation(&self, signals: &mut Vec<String>, score: f64) -> f64 {
        let value = self.compute_goal_stagnation();
        if value > 0.0 {
            signals.push("goal_stagnation".to_string());
        }
        value.mul_add(self.config.weight_goal_stagnation, score)
    }

    fn collect_tool_monotony(&self, signals: &mut Vec<String>, score: f64) -> f64 {
        let value = self.compute_tool_monotony();
        if value > 0.0 {
            signals.push("tool_monotony".to_string());
        }
        value.mul_add(self.config.weight_tool_monotony, score)
    }

    fn collect_correction_frequency(&self, signals: &mut Vec<String>, score: f64) -> f64 {
        let value = self.compute_correction_frequency();
        if value > 0.0 {
            signals.push("correction_frequency".to_string());
        }
        value.mul_add(self.config.weight_correction, score)
    }

    fn collect_low_confidence(
        &self,
        signals: &mut Vec<String>,
        score: f64,
        confidence_score: f64,
    ) -> f64 {
        let value: f64 = if confidence_score < self.config.low_confidence_threshold {
            1.0
        } else {
            0.0
        };
        if value > 0.0 {
            signals.push("low_confidence".to_string());
        }
        value.mul_add(self.config.weight_low_confidence, score)
    }

    fn collect_failure_streak(&self, signals: &mut Vec<String>, score: f64) -> f64 {
        let value: f64 = if self.failure_streak >= self.config.failure_streak_threshold {
            1.0
        } else {
            0.0
        };
        if value > 0.0 {
            signals.push("failure_streak".to_string());
        }
        value.mul_add(self.config.weight_failure_streak, score)
    }

    fn compute_goal_stagnation(&self) -> f64 {
        if self.goal_history.len() < self.config.goal_stagnation_threshold {
            return 0.0;
        }
        let first = &self.goal_history[0];
        if self.goal_history.iter().all(|g| g == first) {
            1.0
        } else {
            0.0
        }
    }

    fn compute_tool_monotony(&self) -> f64 {
        if self.recent_tools.is_empty() {
            return 0.0;
        }
        let mut counts = std::collections::HashMap::new();
        for tool in &self.recent_tools {
            *counts.entry(tool.as_str()).or_insert(0usize) += 1;
        }
        let max_count = counts.values().max().copied().unwrap_or(0);
        let mc = u32::try_from(max_count).unwrap_or(u32::MAX);
        let rt = u32::try_from(self.recent_tools.len()).unwrap_or(1);
        let ratio = f64::from(mc) / f64::from(rt);
        if ratio > self.config.monotony_threshold {
            1.0
        } else {
            0.0
        }
    }

    const fn compute_correction_frequency(&self) -> f64 {
        if self.correction_count >= self.config.correction_threshold {
            1.0
        } else {
            0.0
        }
    }
}

impl Default for FrameAuditDetector {
    fn default() -> Self {
        Self::new(FrameAuditConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_signals_returns_none() {
        let detector = FrameAuditDetector::default();
        assert!(detector.check(0.5).is_none());
    }

    #[test]
    fn tool_monotony_triggers_low() {
        let mut detector = FrameAuditDetector::default();
        for _ in 0..8 {
            detector.record_tool_call("read");
        }
        detector.record_tool_call("write");
        detector.record_tool_call("bash");

        let result = detector.check(0.5);
        assert!(result.is_some());
        let event = result.unwrap();
        assert!(event.signals.contains(&"tool_monotony".to_string()));
        assert_eq!(event.level, FrameRiskLevel::Low);
    }

    #[test]
    fn multiple_signals_trigger_high() {
        let mut detector = FrameAuditDetector::default();

        // Goal stagnation
        for _ in 0..5 {
            detector.record_goal_state("same goal");
        }

        // Tool monotony
        for _ in 0..8 {
            detector.record_tool_call("read");
        }
        detector.record_tool_call("write");
        detector.record_tool_call("bash");

        // Correction frequency
        for _ in 0..3 {
            detector.record_user_correction();
        }

        // Failure streak
        for _ in 0..3 {
            detector.record_tool_result(false);
        }

        let result = detector.check(0.5);
        assert!(result.is_some());
        let event = result.unwrap();
        assert_eq!(event.level, FrameRiskLevel::High);
        assert!(event.signals.contains(&"goal_stagnation".to_string()));
        assert!(event.signals.contains(&"tool_monotony".to_string()));
        assert!(event.signals.contains(&"correction_frequency".to_string()));
        assert!(event.signals.contains(&"failure_streak".to_string()));
    }

    #[test]
    fn sliding_window_respects_limit() {
        let mut detector = FrameAuditDetector::default();
        // Fill with monotonous calls
        for _ in 0..8 {
            detector.record_tool_call("read");
        }
        // Then diversify (total 10, last 10 is 5 different)
        for _ in 0..5 {
            detector.record_tool_call("write");
        }
        for _ in 0..5 {
            detector.record_tool_call("bash");
        }
        // Window is last 10: 5 write + 5 bash -> max 50%, below 70%
        assert!(detector.check(0.5).is_none());
    }

    #[test]
    fn risk_level_boundaries() {
        assert_eq!(FrameRiskLevel::from_score(0.0), FrameRiskLevel::None);
        assert_eq!(FrameRiskLevel::from_score(0.19), FrameRiskLevel::None);
        assert_eq!(FrameRiskLevel::from_score(0.2), FrameRiskLevel::Low);
        assert_eq!(FrameRiskLevel::from_score(0.39), FrameRiskLevel::Low);
        assert_eq!(FrameRiskLevel::from_score(0.4), FrameRiskLevel::Medium);
        assert_eq!(FrameRiskLevel::from_score(0.69), FrameRiskLevel::Medium);
        assert_eq!(FrameRiskLevel::from_score(0.7), FrameRiskLevel::High);
        assert_eq!(FrameRiskLevel::from_score(1.0), FrameRiskLevel::High);
    }

    #[test]
    fn low_confidence_signal() {
        let detector = FrameAuditDetector::default();
        // Low confidence alone: 0.15 * 1.0 = 0.15 -> None (below 0.2)
        assert!(detector.check(0.1).is_none());

        // But combined with another signal it pushes over
        let mut detector2 = FrameAuditDetector::default();
        for _ in 0..3 {
            detector2.record_tool_result(false);
        }
        // failure_streak: 0.15 + low_confidence: 0.15 = 0.30 -> Low
        let result = detector2.check(0.1);
        assert!(result.is_some());
        let event = result.unwrap();
        assert!(event.signals.contains(&"low_confidence".to_string()));
        assert!(event.signals.contains(&"failure_streak".to_string()));
        assert_eq!(event.level, FrameRiskLevel::Low);
    }

    #[test]
    fn success_resets_failure_streak() {
        let mut detector = FrameAuditDetector::default();
        detector.record_tool_result(false);
        detector.record_tool_result(false);
        detector.record_tool_result(true); // resets
        detector.record_tool_result(false);
        // streak is 1, below threshold
        assert!(detector.check(0.5).is_none());
    }

    #[test]
    fn goal_stagnation_needs_full_window() {
        let mut detector = FrameAuditDetector::default();
        // Only 3 entries, below threshold of 5
        for _ in 0..3 {
            detector.record_goal_state("same");
        }
        assert!(detector.check(0.5).is_none());

        // Now fill to 5
        detector.record_goal_state("same");
        detector.record_goal_state("same");
        // goal_stagnation alone: 0.25 -> Low
        let result = detector.check(0.5);
        assert!(result.is_some());
        assert!(
            result
                .unwrap()
                .signals
                .contains(&"goal_stagnation".to_string())
        );
    }
}
