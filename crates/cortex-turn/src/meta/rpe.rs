use std::collections::HashMap;

use cortex_types::config::RpeConfig;

const ALPHA: f64 = 0.3;
const INITIAL_UTILITY: f64 = 0.5;
const MIN_CALLS_FOR_SUGGESTION: usize = 10;
const MIN_CALLS_FOR_DRIFT: usize = 10;

struct ToolUtility {
    value: f64,
    count: usize,
}

/// Tracks RPE-based tool utility using EWMA + UCB1 exploration.
pub struct ToolUtilityTracker {
    utilities: HashMap<String, ToolUtility>,
    config: RpeConfig,
}

pub struct UsageSuggestion {
    pub message: String,
}

impl ToolUtilityTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::with_config(RpeConfig::default())
    }

    #[must_use]
    pub fn with_config(config: RpeConfig) -> Self {
        Self {
            utilities: HashMap::new(),
            config,
        }
    }

    /// Record a tool outcome. outcome: 1.0 = success, 0.0 = failure, 0.5 = partial.
    pub fn record(&mut self, tool_name: &str, outcome: f64) {
        let entry = self
            .utilities
            .entry(tool_name.to_string())
            .or_insert(ToolUtility {
                value: INITIAL_UTILITY,
                count: 0,
            });
        entry.value = outcome.mul_add(ALPHA, entry.value * (1.0 - ALPHA));
        entry.count += 1;
    }

    #[must_use]
    pub fn utility(&self, tool_name: &str) -> f64 {
        self.utilities
            .get(tool_name)
            .map_or(INITIAL_UTILITY, |u| u.value)
    }

    #[must_use]
    pub fn call_count(&self, tool_name: &str) -> usize {
        self.utilities.get(tool_name).map_or(0, |u| u.count)
    }

    /// UCB1 exploration bonus: `sqrt(ln(total) / tool_calls)`.
    #[must_use]
    pub fn exploration_bonus(&self, tool_name: &str) -> f64 {
        let total: usize = self.utilities.values().map(|u| u.count).sum();
        let tool_calls = self.call_count(tool_name).max(1);
        if total == 0 {
            return 0.0;
        }
        let total_f = f64::from(u32::try_from(total).unwrap_or(u32::MAX));
        let calls_f = f64::from(u32::try_from(tool_calls).unwrap_or(u32::MAX));
        (total_f.ln() / calls_f).sqrt()
    }

    /// Returns true if the exploration bonus exceeds the tool's current utility.
    #[must_use]
    pub fn should_explore(&self, tool_name: &str) -> bool {
        self.exploration_bonus(tool_name) > self.utility(tool_name)
    }

    /// Tools with high exploration bonus, sorted descending.
    #[must_use]
    pub fn exploration_candidates(&self) -> Vec<(String, f64)> {
        let total: usize = self.utilities.values().map(|u| u.count).sum();
        if total == 0 {
            return Vec::new();
        }
        let mut candidates: Vec<(String, f64)> = self
            .utilities
            .iter()
            .filter(|(_, u)| u.count > 0)
            .map(|(name, _)| {
                let bonus = self.exploration_bonus(name);
                (name.clone(), bonus)
            })
            .filter(|(name, bonus)| *bonus > self.utility(name))
            .collect();
        candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        candidates
    }

    /// Flag tools with high usage but low utility.
    #[must_use]
    pub fn usage_suggestions(&self) -> Vec<UsageSuggestion> {
        let threshold = self.config.low_utility_threshold;
        self.utilities
            .iter()
            .filter(|(_, u)| u.count >= MIN_CALLS_FOR_SUGGESTION && u.value < threshold)
            .map(|(name, u)| UsageSuggestion {
                message: format!(
                    "{name}: {:.0}% success rate over {} calls — consider alternatives",
                    u.value * 100.0,
                    u.count
                ),
            })
            .collect()
    }

    /// Detect extreme usage imbalance (max/min > configured ratio).
    #[must_use]
    pub fn detect_drift(&self) -> Vec<String> {
        if self.utilities.is_empty() {
            return Vec::new();
        }
        let max_count = self.utilities.values().map(|u| u.count).max().unwrap_or(0);
        if max_count < MIN_CALLS_FOR_DRIFT {
            return Vec::new();
        }
        let min_count = self
            .utilities
            .values()
            .map(|u| u.count)
            .filter(|&c| c > 0)
            .min()
            .unwrap_or(1);
        let max_f = f64::from(u32::try_from(max_count).unwrap_or(u32::MAX));
        let min_f = f64::from(u32::try_from(min_count.max(1)).unwrap_or(u32::MAX));
        if max_f / min_f > self.config.drift_ratio_threshold {
            vec![format!(
                "tool usage drift: max={max_count}, min={min_count} (ratio {:.1}:1)",
                max_f / min_f
            )]
        } else {
            Vec::new()
        }
    }
}

impl Default for ToolUtilityTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initial_utility_neutral() {
        let t = ToolUtilityTracker::new();
        assert!((t.utility("read") - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn success_increases() {
        let mut t = ToolUtilityTracker::new();
        t.record("read", 1.0);
        assert!(t.utility("read") > 0.5);
    }

    #[test]
    fn failure_decreases() {
        let mut t = ToolUtilityTracker::new();
        t.record("read", 0.0);
        assert!(t.utility("read") < 0.5);
    }

    #[test]
    fn ewma_converges() {
        let mut t = ToolUtilityTracker::new();
        for _ in 0..20 {
            t.record("read", 1.0);
        }
        assert!(t.utility("read") > 0.9);
    }

    #[test]
    fn call_count_tracks() {
        let mut t = ToolUtilityTracker::new();
        t.record("read", 1.0);
        t.record("read", 1.0);
        assert_eq!(t.call_count("read"), 2);
    }

    #[test]
    fn exploration_candidates_empty() {
        let t = ToolUtilityTracker::new();
        assert!(t.exploration_candidates().is_empty());
    }

    #[test]
    fn exploration_candidates_low_use() {
        let mut t = ToolUtilityTracker::new();
        for _ in 0..30 {
            t.record("read", 1.0);
        }
        t.record("write", 1.0);
        let candidates = t.exploration_candidates();
        assert!(candidates.iter().any(|(name, _)| name == "write"));
    }

    #[test]
    fn drift_balanced_empty() {
        let mut t = ToolUtilityTracker::new();
        for _ in 0..10 {
            t.record("read", 1.0);
            t.record("write", 1.0);
        }
        assert!(t.detect_drift().is_empty());
    }

    #[test]
    fn drift_imbalanced() {
        let mut t = ToolUtilityTracker::new();
        for _ in 0..20 {
            t.record("read", 1.0);
        }
        t.record("write", 1.0);
        assert!(!t.detect_drift().is_empty());
    }

    #[test]
    fn drift_low_total() {
        let mut t = ToolUtilityTracker::new();
        for _ in 0..5 {
            t.record("read", 1.0);
        }
        t.record("write", 1.0);
        assert!(t.detect_drift().is_empty());
    }
}
