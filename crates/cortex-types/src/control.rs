use serde::{Deserialize, Serialize};

use crate::{RetrievalDecision, SessionId, TurnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSignal {
    Continue,
    Retrieve,
    AskHuman,
    RequestPermission,
    CallTool,
    ConsolidateMemory,
    RepairDelivery,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSignal {
    pub support: f32,
    pub conflict: f32,
    pub risk: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Accumulator {
    pub drift: f32,
    pub boundary: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExpectedControlValue {
    pub benefit: f32,
    pub cost: f32,
    pub risk: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlDecision {
    pub signal: ControlSignal,
    pub confidence: f32,
    pub rationale: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
#[serde(rename_all = "snake_case")]
pub enum TurnTransitionError {
    IllegalTransition,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnFrontier {
    pub turn_id: TurnId,
    pub session_id: SessionId,
    pub state: TurnState,
    pub execution_version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProductionCondition {
    Always,
    TurnState { state: TurnState },
    Retrieval { decision: RetrievalDecision },
    Control { signal: ControlSignal },
    MinConfidence { threshold: f32 },
    All { conditions: Vec<Self> },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionContext {
    pub turn_state: TurnState,
    pub retrieval: RetrievalDecision,
    pub control: ControlSignal,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionRule {
    pub id: String,
    pub condition: ProductionCondition,
    pub action: ControlSignal,
    pub utility: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProductionSystem {
    pub rules: Vec<ProductionRule>,
}

impl Accumulator {
    #[must_use]
    pub const fn new(drift: f32, boundary: f32) -> Self {
        Self {
            drift,
            boundary,
            value: 0.0,
        }
    }

    #[must_use]
    pub fn step(mut self, evidence: EvidenceSignal) -> Self {
        let signed = evidence.support - evidence.conflict - evidence.risk;
        self.value = self
            .drift
            .mul_add(signed, self.value)
            .clamp(-self.boundary, self.boundary);
        self
    }

    #[must_use]
    pub fn confidence(&self) -> f32 {
        if self.boundary <= f32::EPSILON {
            return 0.0;
        }
        (self.value.abs() / self.boundary).clamp(0.0, 1.0)
    }
}

impl ExpectedControlValue {
    #[must_use]
    pub fn score(self) -> f32 {
        (self.benefit - self.cost - self.risk).clamp(-1.0, 1.0)
    }
}

impl ControlDecision {
    #[must_use]
    pub fn decide(accumulator: &Accumulator, value: ExpectedControlValue) -> Self {
        let score = value.score();
        let confidence = accumulator.confidence().max(score.abs());
        let signal = if value.risk >= 0.8 {
            ControlSignal::RequestPermission
        } else if accumulator.value <= -accumulator.boundary * 0.75 {
            ControlSignal::AskHuman
        } else if score > 0.35 {
            ControlSignal::Retrieve
        } else if score < -0.35 {
            ControlSignal::Stop
        } else {
            ControlSignal::Continue
        };
        Self {
            signal,
            confidence,
            rationale: format!("evc={score:.3},acc={:.3}", accumulator.value),
        }
    }
}

impl TurnState {
    #[must_use]
    pub const fn can_transition_to(self, next: Self) -> bool {
        matches!(
            (self, next),
            (
                Self::Idle | Self::Interrupted | Self::Suspended,
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
                    | Self::Suspended,
            ) | (
                Self::AwaitingToolResult | Self::AwaitingPermission | Self::AwaitingHumanInput,
                Self::Processing | Self::Interrupted | Self::Suspended,
            ) | (Self::Compacting, Self::Processing | Self::Interrupted)
                | (
                    Self::Consolidating,
                    Self::Processing | Self::Completed | Self::Interrupted,
                )
        )
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed)
    }
}

impl TurnFrontier {
    #[must_use]
    pub fn new(
        turn_id: TurnId,
        session_id: SessionId,
        execution_version: impl Into<String>,
    ) -> Self {
        Self {
            turn_id,
            session_id,
            state: TurnState::Idle,
            execution_version: execution_version.into(),
        }
    }

    /// # Errors
    /// Returns an error when the requested transition is not legal for the
    /// current turn state.
    pub const fn transition(&mut self, next: TurnState) -> Result<(), TurnTransitionError> {
        if self.state.can_transition_to(next) {
            self.state = next;
            Ok(())
        } else {
            Err(TurnTransitionError::IllegalTransition)
        }
    }
}

impl ProductionCondition {
    #[must_use]
    pub fn matches(&self, context: &ProductionContext) -> bool {
        match self {
            Self::Always => true,
            Self::TurnState { state } => context.turn_state == *state,
            Self::Retrieval { decision } => context.retrieval == *decision,
            Self::Control { signal } => context.control == *signal,
            Self::MinConfidence { threshold } => context.confidence >= *threshold,
            Self::All { conditions } => conditions
                .iter()
                .all(|condition| condition.matches(context)),
        }
    }
}

impl ProductionRule {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        condition: ProductionCondition,
        action: ControlSignal,
        utility: f32,
    ) -> Self {
        Self {
            id: id.into(),
            condition,
            action,
            utility,
        }
    }

    pub fn update_utility(&mut self, reward: f32, learning_rate: f32) {
        let rate = learning_rate.clamp(0.0, 1.0);
        self.utility += rate * (reward - self.utility);
        self.utility = self.utility.clamp(-1.0, 1.0);
    }
}

impl ProductionSystem {
    #[must_use]
    pub const fn new(rules: Vec<ProductionRule>) -> Self {
        Self { rules }
    }

    #[must_use]
    pub fn select<'a>(&'a self, context: &ProductionContext) -> Option<&'a ProductionRule> {
        self.rules
            .iter()
            .filter(|rule| rule.condition.matches(context))
            .max_by(|left, right| {
                left.utility
                    .total_cmp(&right.utility)
                    .then_with(|| right.id.cmp(&left.id))
            })
    }
}
