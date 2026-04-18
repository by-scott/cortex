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

#[cfg(test)]
mod tests {
    use super::*;

    fn healthy_report() -> HealthReport {
        HealthReport {
            memory_fragmentation: 0.1,
            context_pressure_trend: 0.2,
            recall_degradation: 0.05,
            fatigue_trend: 0.1,
            overall_health: 0.9,
        }
    }

    fn default_health_config() -> HealthConfig {
        HealthConfig::default()
    }

    fn default_recovery_config() -> HealthRecoveryConfig {
        HealthRecoveryConfig::default()
    }

    #[test]
    fn no_actions_when_healthy() {
        let actions = HealthRecoveryEngine::evaluate(
            &healthy_report(),
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn consolidate_memory_on_high_fragmentation() {
        let report = HealthReport {
            memory_fragmentation: 0.85,
            ..healthy_report()
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.contains(&RecoveryAction::ConsolidateMemory));
    }

    #[test]
    fn compress_context_on_high_pressure() {
        let report = HealthReport {
            context_pressure_trend: 0.9,
            ..healthy_report()
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.contains(&RecoveryAction::CompressContext));
    }

    #[test]
    fn accelerate_decay_on_high_fatigue() {
        let report = HealthReport {
            fatigue_trend: 0.8,
            ..healthy_report()
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.contains(&RecoveryAction::AccelerateDecay));
    }

    #[test]
    fn checkpoint_session_on_low_overall_health() {
        let report = HealthReport {
            overall_health: 0.2,
            ..healthy_report()
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.contains(&RecoveryAction::CheckpointSession));
    }

    #[test]
    fn multiple_actions_for_multi_dimension_degradation() {
        let report = HealthReport {
            memory_fragmentation: 0.9,
            context_pressure_trend: 0.85,
            recall_degradation: 0.5,
            fatigue_trend: 0.8,
            overall_health: 0.15,
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert_eq!(actions.len(), 4);
        assert!(actions.contains(&RecoveryAction::ConsolidateMemory));
        assert!(actions.contains(&RecoveryAction::CompressContext));
        assert!(actions.contains(&RecoveryAction::AccelerateDecay));
        assert!(actions.contains(&RecoveryAction::CheckpointSession));
    }

    #[test]
    fn threshold_boundary_exactly_at_threshold_no_trigger() {
        let dim = default_recovery_config().dimension_threshold;
        let report = HealthReport {
            memory_fragmentation: dim,
            context_pressure_trend: dim,
            recall_degradation: 0.0,
            fatigue_trend: dim,
            overall_health: 0.3, // exactly at degraded_threshold should NOT trigger
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert!(actions.is_empty());
    }

    #[test]
    fn threshold_boundary_just_above_triggers() {
        let report = HealthReport {
            memory_fragmentation: 0.700_001,
            context_pressure_trend: 0.1,
            recall_degradation: 0.0,
            fatigue_trend: 0.1,
            overall_health: 0.9,
        };
        let actions = HealthRecoveryEngine::evaluate(
            &report,
            &default_health_config(),
            &default_recovery_config(),
        );
        assert_eq!(actions.len(), 1);
        assert!(actions.contains(&RecoveryAction::ConsolidateMemory));
    }

    #[test]
    fn format_actions_empty() {
        assert_eq!(HealthRecoveryEngine::format_actions(&[]), "");
    }

    #[test]
    fn format_actions_single() {
        let actions = vec![RecoveryAction::CompressContext];
        assert_eq!(
            HealthRecoveryEngine::format_actions(&actions),
            "CompressContext"
        );
    }

    #[test]
    fn format_actions_multiple() {
        let actions = vec![
            RecoveryAction::ConsolidateMemory,
            RecoveryAction::CompressContext,
            RecoveryAction::AccelerateDecay,
        ];
        assert_eq!(
            HealthRecoveryEngine::format_actions(&actions),
            "ConsolidateMemory, CompressContext, AccelerateDecay"
        );
    }
}
