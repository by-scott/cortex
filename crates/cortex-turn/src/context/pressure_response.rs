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
}

impl PressureAction {
    const fn label(&self) -> &'static str {
        match self {
            Self::AccelerateDecay => "accelerate_decay",
            Self::CompressHistory => "compress_history",
            Self::TrimWorkingMemory { .. } => "trim_working_memory",
            Self::ClearWorkingMemory => "clear_working_memory",
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
            actions.push(PressureAction::CompressHistory);
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
