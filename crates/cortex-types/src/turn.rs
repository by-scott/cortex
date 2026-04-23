use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TurnState {
    Idle,
    Processing,
    AwaitingToolResult,
    AwaitingPermission,
    AwaitingHumanInput,
    Compacting,
    Consolidating,
    Completed,
    Interrupted,
    Suspended,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnPhase {
    Sn,
    Tpn,
    Dmn,
}

#[derive(Debug, Clone)]
pub struct TurnTransitionError {
    pub from: TurnState,
    pub to: TurnState,
}

impl TurnState {
    /// # Errors
    /// Returns `TurnTransitionError` if the transition is not valid.
    pub const fn try_transition(self, to: Self) -> Result<Self, TurnTransitionError> {
        let valid = matches!(
            (self, to),
            (
                Self::Idle
                    | Self::AwaitingToolResult
                    | Self::AwaitingPermission
                    | Self::AwaitingHumanInput
                    | Self::Compacting
                    | Self::Consolidating,
                Self::Processing
            ) | (
                Self::Processing,
                Self::AwaitingToolResult
                    | Self::AwaitingPermission
                    | Self::AwaitingHumanInput
                    | Self::Compacting
                    | Self::Consolidating
                    | Self::Completed
                    | Self::Interrupted
            ) | (Self::Suspended, Self::Idle)
        );

        if valid {
            Ok(to)
        } else {
            Err(TurnTransitionError { from: self, to })
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Interrupted)
    }
}

impl fmt::Display for TurnTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid turn state transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for TurnTransitionError {}

impl fmt::Display for TurnPhase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sn => write!(f, "SN"),
            Self::Tpn => write!(f, "TPN"),
            Self::Dmn => write!(f, "DMN"),
        }
    }
}
