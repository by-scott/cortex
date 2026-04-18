use cortex_types::config::{HealthConfig, HealthReport};
use cortex_types::{MemoryEntry, MemoryStatus};

/// Session-level health self-checker.
///
/// Periodically evaluates 4 dimensions of cognitive health and produces
/// a `HealthReport`.
pub struct HealthChecker {
    config: HealthConfig,
}

impl HealthChecker {
    #[must_use]
    pub const fn new(config: HealthConfig) -> Self {
        Self { config }
    }

    /// Whether a health check should run at the given turn count.
    #[must_use]
    pub const fn should_check(&self, turn_count: usize) -> bool {
        self.config.check_interval_turns > 0
            && turn_count > 0
            && turn_count.is_multiple_of(self.config.check_interval_turns)
    }

    /// Memory fragmentation: ratio of deprecated or very low-strength memories.
    ///
    /// Returns 0.0 (healthy) to 1.0 (heavily fragmented).
    #[must_use]
    pub fn memory_fragmentation_score(memories: &[MemoryEntry]) -> f64 {
        if memories.is_empty() {
            return 0.0;
        }
        let fragmented = u32::try_from(
            memories
                .iter()
                .filter(|m| m.status == MemoryStatus::Deprecated || m.strength < 0.3)
                .count(),
        )
        .unwrap_or(u32::MAX);
        let total = u32::try_from(memories.len()).unwrap_or(1);
        f64::from(fragmented) / f64::from(total)
    }

    /// Context pressure trend: sliding average of recent occupancy values.
    ///
    /// Returns 0.0 (low pressure) to 1.0 (sustained overload).
    #[must_use]
    pub fn context_pressure_score(occupancy_history: &[f64]) -> f64 {
        if occupancy_history.is_empty() {
            return 0.0;
        }
        let sum: f64 = occupancy_history.iter().sum();
        let len = u32::try_from(occupancy_history.len()).unwrap_or(1);
        (sum / f64::from(len)).clamp(0.0, 1.0)
    }

    /// Recall degradation: measures decline in precision over time.
    ///
    /// Compares first half average to second half average.
    /// Returns 0.0 (no degradation or improving) to 1.0 (severe degradation).
    #[must_use]
    pub fn recall_degradation_score(precision_history: &[f64]) -> f64 {
        if precision_history.len() < 2 {
            return 0.0;
        }
        let mid = precision_history.len() / 2;
        let mid_u32 = u32::try_from(mid).unwrap_or(1);
        let second_len = u32::try_from(precision_history.len() - mid).unwrap_or(1);
        let first_half: f64 = precision_history[..mid].iter().sum::<f64>() / f64::from(mid_u32);
        let second_half: f64 = precision_history[mid..].iter().sum::<f64>() / f64::from(second_len);
        // If second half is worse (lower precision), that's degradation
        let decline = first_half - second_half;
        decline.clamp(0.0, 1.0)
    }

    /// Fatigue score: current fatigue relative to threshold.
    ///
    /// Returns 0.0 (fresh) to 1.0 (at or above threshold).
    #[must_use]
    pub fn fatigue_score(fatigue_value: f64, threshold: f64) -> f64 {
        if threshold <= 0.0 {
            return 0.0;
        }
        (fatigue_value / threshold).clamp(0.0, 1.0)
    }

    /// Run a full health check and return the report.
    ///
    /// The `overall_health` is 1.0 minus the weighted average of problem scores.
    #[must_use]
    pub fn check(
        &self,
        memories: &[MemoryEntry],
        occupancy_history: &[f64],
        precision_history: &[f64],
        fatigue_value: f64,
        fatigue_threshold: f64,
    ) -> HealthReport {
        let mem_frag = Self::memory_fragmentation_score(memories);
        let ctx_pressure = Self::context_pressure_score(occupancy_history);
        let recall_deg = Self::recall_degradation_score(precision_history);
        let fatigue = Self::fatigue_score(fatigue_value, fatigue_threshold);

        let weights = self.normalized_weights();
        let problem_score = mem_frag.mul_add(
            weights[0],
            ctx_pressure.mul_add(
                weights[1],
                recall_deg.mul_add(weights[2], fatigue * weights[3]),
            ),
        );

        HealthReport {
            memory_fragmentation: mem_frag,
            context_pressure_trend: ctx_pressure,
            recall_degradation: recall_deg,
            fatigue_trend: fatigue,
            overall_health: (1.0 - problem_score).clamp(0.0, 1.0),
        }
    }

    /// Whether the report indicates degraded health.
    #[must_use]
    pub fn is_degraded(&self, report: &HealthReport) -> bool {
        report.overall_health < self.config.degraded_threshold
    }

    fn normalized_weights(&self) -> [f64; 4] {
        let w = &self.config.weights;
        if w.len() < 4 {
            return [0.25; 4];
        }
        let sum: f64 = w[..4].iter().sum();
        if sum <= 0.0 {
            return [0.25; 4];
        }
        [w[0] / sum, w[1] / sum, w[2] / sum, w[3] / sum]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::{MemoryKind, MemoryType};

    fn make_memory(status: MemoryStatus, strength: f64) -> MemoryEntry {
        let mut m = MemoryEntry::new("content", "desc", MemoryType::User, MemoryKind::Semantic);
        m.status = status;
        m.strength = strength;
        m
    }

    #[test]
    fn default_config_values() {
        let cfg = HealthConfig::default();
        assert_eq!(cfg.check_interval_turns, 10);
        assert!((cfg.degraded_threshold - 0.3).abs() < f64::EPSILON);
        assert_eq!(cfg.weights.len(), 4);
    }

    #[test]
    fn should_check_at_interval() {
        let checker = HealthChecker::new(HealthConfig::default());
        assert!(!checker.should_check(0));
        assert!(!checker.should_check(1));
        assert!(!checker.should_check(5));
        assert!(checker.should_check(10));
        assert!(checker.should_check(20));
        assert!(!checker.should_check(15));
    }

    #[test]
    fn memory_fragmentation_empty() {
        assert!(HealthChecker::memory_fragmentation_score(&[]).abs() < f64::EPSILON);
    }

    #[test]
    fn memory_fragmentation_all_healthy() {
        let memories = vec![
            make_memory(MemoryStatus::Stabilized, 0.8),
            make_memory(MemoryStatus::Materialized, 0.5),
        ];
        assert!(HealthChecker::memory_fragmentation_score(&memories).abs() < f64::EPSILON);
    }

    #[test]
    fn memory_fragmentation_all_deprecated() {
        let memories = vec![
            make_memory(MemoryStatus::Deprecated, 0.1),
            make_memory(MemoryStatus::Deprecated, 0.2),
        ];
        assert!((HealthChecker::memory_fragmentation_score(&memories) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn memory_fragmentation_mixed() {
        let memories = vec![
            make_memory(MemoryStatus::Stabilized, 0.8),
            make_memory(MemoryStatus::Deprecated, 0.1),
            make_memory(MemoryStatus::Captured, 0.2), // low strength counts
            make_memory(MemoryStatus::Materialized, 0.9),
        ];
        // 2 out of 4 are fragmented
        assert!((HealthChecker::memory_fragmentation_score(&memories) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn context_pressure_empty() {
        assert!(HealthChecker::context_pressure_score(&[]).abs() < f64::EPSILON);
    }

    #[test]
    fn context_pressure_average() {
        let history = vec![0.5, 0.7, 0.9];
        let expected = (0.5 + 0.7 + 0.9) / 3.0;
        assert!((HealthChecker::context_pressure_score(&history) - expected).abs() < 0.001);
    }

    #[test]
    fn recall_degradation_improving() {
        // First half worse than second half = no degradation
        let history = vec![0.3, 0.4, 0.7, 0.8];
        assert!(HealthChecker::recall_degradation_score(&history).abs() < f64::EPSILON);
    }

    #[test]
    fn recall_degradation_declining() {
        // First half 0.8, second half 0.3 = degradation 0.5
        let history = vec![0.8, 0.8, 0.3, 0.3];
        assert!((HealthChecker::recall_degradation_score(&history) - 0.5).abs() < 0.001);
    }

    #[test]
    fn fatigue_score_zero() {
        assert!(HealthChecker::fatigue_score(0.0, 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn fatigue_score_half() {
        assert!((HealthChecker::fatigue_score(0.4, 0.8) - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn fatigue_score_clamped() {
        assert!((HealthChecker::fatigue_score(1.5, 0.8) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn fatigue_score_zero_threshold() {
        assert!(HealthChecker::fatigue_score(0.5, 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn overall_health_perfect() {
        let checker = HealthChecker::new(HealthConfig::default());
        let report = checker.check(
            &[make_memory(MemoryStatus::Stabilized, 0.9)],
            &[0.1, 0.2],
            &[0.9, 0.9],
            0.0,
            0.8,
        );
        assert!(report.overall_health > 0.8);
        assert!(!checker.is_degraded(&report));
    }

    #[test]
    fn overall_health_degraded() {
        let checker = HealthChecker::new(HealthConfig::default());
        let report = checker.check(
            &[
                make_memory(MemoryStatus::Deprecated, 0.1),
                make_memory(MemoryStatus::Deprecated, 0.1),
            ],
            &[0.9, 0.95, 0.98],
            &[0.9, 0.8, 0.2, 0.1],
            0.75,
            0.8,
        );
        assert!(report.overall_health < 0.3);
        assert!(checker.is_degraded(&report));
    }

    #[test]
    fn health_report_serializes() {
        let report = HealthReport {
            memory_fragmentation: 0.1,
            context_pressure_trend: 0.2,
            recall_degradation: 0.05,
            fatigue_trend: 0.3,
            overall_health: 0.85,
        };
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"overall_health\":0.85"));
    }

    #[test]
    fn health_config_toml_roundtrip() {
        let toml_str = r"
[health]
check_interval_turns = 5
degraded_threshold = 0.4
weights = [0.3, 0.2, 0.3, 0.2]
";
        let config: cortex_types::config::CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.health.check_interval_turns, 5);
        assert!((config.health.degraded_threshold - 0.4).abs() < f64::EPSILON);
        assert_eq!(config.health.weights, vec![0.3, 0.2, 0.3, 0.2]);
    }
}
