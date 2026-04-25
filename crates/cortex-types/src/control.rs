use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    ContinueTurn,
    Retrieve,
    Rerank,
    AskHuman,
    RequestPermission,
    CallTool,
    CompactContext,
    ConsolidateMemory,
    RetryDelivery,
    Suspend,
    Interrupt,
    Deny,
    Finish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Conflict {
    ContradictoryEvidence,
    PolicyConflict,
    ActorAmbiguity,
    ToolRisk,
    LowRetrievalSupport,
    RenderFailure,
    ProviderTruncation,
    TransportDeliveryFailure,
    RepeatedFailure,
    StaleMemory,
    BudgetExhaustion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImpasseKind {
    NoApplicableAction,
    ConflictingActions,
    MissingInformation,
    PermissionRequired,
    ToolUnavailable,
    PolicyDenied,
    RenderBlocked,
    DeliveryFailed,
    ResourceExhausted,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub signal: Signal,
    pub rationale: String,
    pub confidence: f32,
    pub expected_benefit: f32,
    pub expected_cost: f32,
    pub risk: f32,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Impasse {
    pub id: String,
    pub kind: ImpasseKind,
    pub owner_actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub summary: String,
    pub conflicts: Vec<Conflict>,
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subgoal {
    pub id: String,
    pub impasse_id: String,
    pub owner_actor: String,
    pub strategy: Signal,
    pub objective: String,
    pub created_at: DateTime<Utc>,
}

impl Signal {
    #[must_use]
    pub const fn requires_external_wait(self) -> bool {
        matches!(
            self,
            Self::AskHuman | Self::RequestPermission | Self::CallTool | Self::RetryDelivery
        )
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Deny | Self::Finish | Self::Interrupt | Self::Suspend
        )
    }
}

impl Decision {
    #[must_use]
    pub fn new(signal: Signal, rationale: impl Into<String>) -> Self {
        Self {
            signal,
            rationale: rationale.into(),
            confidence: 0.0,
            expected_benefit: 0.0,
            expected_cost: 0.0,
            risk: 0.0,
            decided_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn with_scores(
        mut self,
        confidence: f32,
        benefit: f32,
        cost: f32,
        risk: f32,
    ) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self.expected_benefit = benefit.clamp(0.0, 1.0);
        self.expected_cost = cost.clamp(0.0, 1.0);
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub fn expected_value(&self) -> f32 {
        (self.expected_benefit - self.expected_cost - self.risk).clamp(-1.0, 1.0)
    }
}

impl Impasse {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        kind: ImpasseKind,
        owner_actor: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            kind,
            owner_actor: owner_actor.into(),
            session_id: None,
            summary: summary.into(),
            conflicts: Vec::new(),
            created_at: Utc::now(),
            resolved_at: None,
        }
    }

    #[must_use]
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    pub fn push_conflict(&mut self, conflict: Conflict) {
        if !self.conflicts.contains(&conflict) {
            self.conflicts.push(conflict);
        }
    }

    pub fn resolve(&mut self) {
        self.resolved_at = Some(Utc::now());
    }

    #[must_use]
    pub const fn is_resolved(&self) -> bool {
        self.resolved_at.is_some()
    }
}

impl Subgoal {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        impasse_id: impl Into<String>,
        owner_actor: impl Into<String>,
        strategy: Signal,
        objective: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            impasse_id: impasse_id.into(),
            owner_actor: owner_actor.into(),
            strategy,
            objective: objective.into(),
            created_at: Utc::now(),
        }
    }
}
