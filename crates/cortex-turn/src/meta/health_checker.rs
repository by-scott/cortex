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
