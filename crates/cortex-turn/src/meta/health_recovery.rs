use cortex_types::config::{HealthConfig, HealthRecoveryConfig, HealthReport};

/// Actions that can be triggered to recover from degraded health.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecoveryAction {
    /// Consolidate deprecated/weak memories to reduce fragmentation.
    ConsolidateMemory,
    /// Compress context window to reduce pressure.
    CompressContext,
    /// Create a session checkpoint for safety.
    CheckpointSession,
    /// Accelerate memory decay to shed cognitive load.
    AccelerateDecay,
}

impl RecoveryAction {
    /// Human-readable name of this action.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        match self {
            Self::ConsolidateMemory => "ConsolidateMemory",
            Self::CompressContext => "CompressContext",
            Self::CheckpointSession => "CheckpointSession",
            Self::AccelerateDecay => "AccelerateDecay",
        }
    }
}

/// Engine that evaluates a health report and determines recovery actions.
pub struct HealthRecoveryEngine;

impl HealthRecoveryEngine {
    /// Evaluate the health report against thresholds and return recovery actions.
    ///
    /// Per-dimension checks use the configured `dimension_threshold` (strictly greater-than).
    /// The overall health check uses the configured `degraded_threshold` (strictly less-than).
    #[must_use]
    pub fn evaluate(
        report: &HealthReport,
        config: &HealthConfig,
        recovery_config: &HealthRecoveryConfig,
    ) -> Vec<RecoveryAction> {
        let mut actions = Vec::new();
        let dim = recovery_config.dimension_threshold;

        if report.memory_fragmentation > dim {
            actions.push(RecoveryAction::ConsolidateMemory);
        }
        if report.context_pressure_trend > dim {
            actions.push(RecoveryAction::CompressContext);
        }
        if report.fatigue_trend > dim {
            actions.push(RecoveryAction::AccelerateDecay);
        }
        if report.overall_health < config.degraded_threshold {
            actions.push(RecoveryAction::CheckpointSession);
        }

        actions
    }

    /// Format recovery actions as a human-readable comma-separated string.
    #[must_use]
    pub fn format_actions(actions: &[RecoveryAction]) -> String {
        actions
            .iter()
            .map(RecoveryAction::name)
            .collect::<Vec<_>>()
            .join(", ")
    }
}
