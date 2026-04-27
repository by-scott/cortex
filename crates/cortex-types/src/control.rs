use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

use crate::EffectReversibility;

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
    #[serde(default)]
    pub reversibility: Option<EffectReversibility>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub candidate_actions: Vec<ActionCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rejected_alternatives: Vec<RejectedAlternative>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_evidence: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub blocking_uncertainty: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub risk_boundary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub fallback_plan: String,
    pub decided_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActionCandidate {
    pub signal: Signal,
    pub rationale: String,
    pub confidence: f32,
    pub expected_benefit: f32,
    pub expected_cost: f32,
    pub risk: f32,
    pub reversibility: EffectReversibility,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_evidence: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RejectedAlternative {
    pub signal: Signal,
    pub reason: String,
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
            reversibility: None,
            candidate_actions: Vec::new(),
            rejected_alternatives: Vec::new(),
            required_evidence: Vec::new(),
            blocking_uncertainty: String::new(),
            risk_boundary: String::new(),
            fallback_plan: String::new(),
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
    pub const fn with_reversibility(mut self, reversibility: EffectReversibility) -> Self {
        self.reversibility = Some(reversibility);
        self
    }

    #[must_use]
    pub fn with_candidate(mut self, candidate: ActionCandidate) -> Self {
        self.candidate_actions.push(candidate);
        self
    }

    #[must_use]
    pub fn with_rejected_alternative(mut self, signal: Signal, reason: impl Into<String>) -> Self {
        self.rejected_alternatives
            .push(RejectedAlternative::new(signal, reason));
        self
    }

    #[must_use]
    pub fn with_required_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.required_evidence.push(evidence.into());
        self
    }

    #[must_use]
    pub fn with_blocking_uncertainty(mut self, uncertainty: impl Into<String>) -> Self {
        self.blocking_uncertainty = uncertainty.into();
        self
    }

    #[must_use]
    pub fn with_risk_boundary(mut self, boundary: impl Into<String>) -> Self {
        self.risk_boundary = boundary.into();
        self
    }

    #[must_use]
    pub fn with_fallback_plan(mut self, plan: impl Into<String>) -> Self {
        self.fallback_plan = plan.into();
        self
    }

    #[must_use]
    pub fn expected_value(&self) -> f32 {
        (self.expected_benefit - self.expected_cost - self.risk).clamp(-1.0, 1.0)
    }

    #[must_use]
    pub fn permission_explanation(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "selected action: {:?}", self.signal);
        let _ = writeln!(out, "reason: {}", self.rationale);
        let _ = writeln!(
            out,
            "scores: confidence={:.2}, benefit={:.2}, cost={:.2}, risk={:.2}, expected_value={:.2}",
            self.confidence,
            self.expected_benefit,
            self.expected_cost,
            self.risk,
            self.expected_value()
        );
        if let Some(reversibility) = self.reversibility {
            let _ = writeln!(out, "reversibility: {reversibility:?}");
        }
        if !self.risk_boundary.is_empty() {
            let _ = writeln!(out, "risk boundary: {}", self.risk_boundary);
        }
        if !self.blocking_uncertainty.is_empty() {
            let _ = writeln!(out, "blocking uncertainty: {}", self.blocking_uncertainty);
        }
        if !self.required_evidence.is_empty() {
            let _ = writeln!(
                out,
                "required evidence: {}",
                self.required_evidence.join("; ")
            );
        }
        if !self.candidate_actions.is_empty() {
            let _ = writeln!(out, "candidate actions:");
            for candidate in &self.candidate_actions {
                let _ = writeln!(
                    out,
                    "- {:?}: value={:.2}, confidence={:.2}, risk={:.2}, reversibility={:?}; {}",
                    candidate.signal,
                    candidate.expected_value(),
                    candidate.confidence,
                    candidate.risk,
                    candidate.reversibility,
                    candidate.rationale
                );
            }
        }
        if !self.rejected_alternatives.is_empty() {
            let _ = writeln!(out, "rejected alternatives:");
            for alternative in &self.rejected_alternatives {
                let _ = writeln!(out, "- {:?}: {}", alternative.signal, alternative.reason);
            }
        }
        if !self.fallback_plan.is_empty() {
            let _ = writeln!(out, "fallback: {}", self.fallback_plan);
        }
        out
    }
}

impl ActionCandidate {
    #[must_use]
    pub fn new(signal: Signal, rationale: impl Into<String>) -> Self {
        Self {
            signal,
            rationale: rationale.into(),
            confidence: 0.0,
            expected_benefit: 0.0,
            expected_cost: 0.0,
            risk: 0.0,
            reversibility: EffectReversibility::PartiallyReversible,
            required_evidence: Vec::new(),
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
    pub const fn with_reversibility(mut self, reversibility: EffectReversibility) -> Self {
        self.reversibility = reversibility;
        self
    }

    #[must_use]
    pub fn with_required_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.required_evidence.push(evidence.into());
        self
    }

    #[must_use]
    pub fn expected_value(&self) -> f32 {
        (self.expected_benefit - self.expected_cost - self.risk).clamp(-1.0, 1.0)
    }
}

impl RejectedAlternative {
    #[must_use]
    pub fn new(signal: Signal, reason: impl Into<String>) -> Self {
        Self {
            signal,
            reason: reason.into(),
        }
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
