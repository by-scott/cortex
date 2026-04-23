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
