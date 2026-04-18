use super::PressureLevel;
use cortex_types::Payload;

/// Actions that the orchestrator should execute in response to pressure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PressureAction {
    /// Accelerate working memory decay (multiply decay rate).
    AccelerateDecay,
    /// Compress conversation history.
    CompressHistory,
    /// Trim working memory to keep only top-N most relevant items.
    TrimWorkingMemory { keep: usize },
    /// Clear all working memory items.
    ClearWorkingMemory,
    /// Trim conversation history to keep only recent N rounds.
    TrimHistory { keep_rounds: usize },
}

impl PressureAction {
    const fn label(&self) -> &'static str {
        match self {
            Self::AccelerateDecay => "accelerate_decay",
            Self::CompressHistory => "compress_history",
            Self::TrimWorkingMemory { .. } => "trim_working_memory",
            Self::ClearWorkingMemory => "clear_working_memory",
            Self::TrimHistory { .. } => "trim_history",
        }
    }
}

/// Result of pressure response computation.
pub struct PressureResponse {
    pub actions: Vec<PressureAction>,
    pub events: Vec<Payload>,
}

/// Compute the response strategy for a given pressure level.
///
/// Returns actions the orchestrator should execute and events for journal audit.
/// Strategies are cumulative -- higher levels include lower-level actions.
#[must_use]
pub fn respond(level: PressureLevel) -> PressureResponse {
    let mut actions = Vec::new();

    match level {
        PressureLevel::Normal => {
            // L0: no action
        }
        PressureLevel::Alert => {
            actions.push(PressureAction::AccelerateDecay);
        }
        PressureLevel::Compress => {
            actions.push(PressureAction::AccelerateDecay);
            actions.push(PressureAction::CompressHistory);
        }
        PressureLevel::Urgent => {
            actions.push(PressureAction::AccelerateDecay);
            actions.push(PressureAction::CompressHistory);
            actions.push(PressureAction::TrimWorkingMemory { keep: 2 });
        }
        PressureLevel::Degrade => {
            actions.push(PressureAction::ClearWorkingMemory);
            actions.push(PressureAction::TrimHistory { keep_rounds: 1 });
        }
    }

    let events = if actions.is_empty() {
        Vec::new()
    } else {
        let action_names: Vec<String> = actions.iter().map(|a| a.label().to_string()).collect();
        vec![Payload::PressureResponseApplied {
            level: format!("{level:?}"),
            actions: action_names,
        }]
    };

    PressureResponse { actions, events }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_no_actions() {
        let resp = respond(PressureLevel::Normal);
        assert!(resp.actions.is_empty());
        assert!(resp.events.is_empty());
    }

    #[test]
    fn alert_accelerates_decay() {
        let resp = respond(PressureLevel::Alert);
        assert_eq!(resp.actions.len(), 1);
        assert_eq!(resp.actions[0], PressureAction::AccelerateDecay);
        assert_eq!(resp.events.len(), 1);
    }

    #[test]
    fn compress_includes_history_compression() {
        let resp = respond(PressureLevel::Compress);
        assert_eq!(resp.actions.len(), 2);
        assert!(resp.actions.contains(&PressureAction::CompressHistory));
    }

    #[test]
    fn urgent_trims_working_memory() {
        let resp = respond(PressureLevel::Urgent);
        assert_eq!(resp.actions.len(), 3);
        assert!(
            resp.actions
                .contains(&PressureAction::TrimWorkingMemory { keep: 2 })
        );
    }

    #[test]
    fn degrade_clears_everything() {
        let resp = respond(PressureLevel::Degrade);
        assert!(resp.actions.contains(&PressureAction::ClearWorkingMemory));
        assert!(
            resp.actions
                .contains(&PressureAction::TrimHistory { keep_rounds: 1 })
        );
    }

    #[test]
    fn actions_monotonically_increase() {
        let levels = [
            PressureLevel::Normal,
            PressureLevel::Alert,
            PressureLevel::Compress,
            PressureLevel::Urgent,
        ];
        let mut prev_count = 0;
        for level in &levels {
            let resp = respond(*level);
            assert!(
                resp.actions.len() >= prev_count,
                "{level:?} should have >= {prev_count} actions"
            );
            prev_count = resp.actions.len();
        }
    }

    #[test]
    fn event_records_level_and_actions() {
        let resp = respond(PressureLevel::Urgent);
        assert!(matches!(
            &resp.events[0],
            Payload::PressureResponseApplied { level, actions }
            if level == "Urgent" && actions.len() == 3
        ));
    }
}
