use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub claim_id: String,
    pub content: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub strength: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: u32,
    #[serde(default = "default_memory_owner_actor")]
    pub owner_actor: String,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconsolidation_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source: MemorySource,
    #[serde(default, skip_serializing_if = "MemoryClaim::is_empty")]
    pub claim: MemoryClaim,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence_events: Vec<MemoryEvidence>,
    #[serde(default)]
    pub confirmed_by_user: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub contradicted_by: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supersedes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_from: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub risk_if_wrong: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub usage_outcomes: Vec<MemoryUsageOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryType {
    #[serde(alias = "user")]
    User,
    #[serde(alias = "feedback")]
    Feedback,
    #[serde(alias = "project")]
    Project,
    #[serde(alias = "reference")]
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    Episodic,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemorySource {
    UserInput,
    ToolOutput,
    #[default]
    LlmGenerated,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    Untrusted,
    Verified,
    Trusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryStatus {
    Captured,
    Materialized,
    Stabilized,
    Deprecated,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryClaim {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub predicate: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub object: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scope: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryEvidence {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub event_id: String,
    #[serde(default)]
    pub source_type: MemorySource,
    #[serde(default)]
    pub confidence: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryUsageOutcomeKind {
    Helped,
    Harmed,
    Neutral,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MemoryUsageOutcome {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub turn_id: String,
    #[serde(default)]
    pub outcome: MemoryUsageOutcomeKind,
    #[serde(default)]
    pub impact: f64,
    #[serde(default = "default_memory_timestamp")]
    pub recorded_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

fn default_memory_owner_actor() -> String {
    "local:default".to_string()
}

fn default_memory_timestamp() -> DateTime<Utc> {
    Utc::now()
}

#[derive(Debug, Clone)]
pub struct MemoryStatusError {
    pub from: MemoryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    pub metadata: Option<String>,
}

impl MemoryEntry {
    #[must_use]
    pub fn new(
        content: impl Into<String>,
        description: impl Into<String>,
        memory_type: MemoryType,
        kind: MemoryKind,
    ) -> Self {
        let now = Utc::now();
        let id = uuid::Uuid::now_v7().to_string();
        Self {
            claim_id: id.clone(),
            id,
            content: content.into(),
            description: description.into(),
            memory_type,
            kind,
            status: MemoryStatus::Captured,
            strength: 1.0,
            created_at: now,
            updated_at: now,
            access_count: 0,
            owner_actor: default_memory_owner_actor(),
            instance_id: String::new(),
            reconsolidation_until: None,
            source: MemorySource::LlmGenerated,
            claim: MemoryClaim::default(),
            evidence_events: Vec::new(),
            confirmed_by_user: false,
            contradicted_by: Vec::new(),
            supersedes: Vec::new(),
            valid_from: None,
            valid_until: None,
            risk_if_wrong: String::new(),
            usage_outcomes: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_claim(
        mut self,
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        self.claim = MemoryClaim::new(subject, predicate, object, scope);
        self
    }

    pub fn add_evidence(&mut self, evidence: MemoryEvidence) {
        self.evidence_events.push(evidence);
        self.updated_at = Utc::now();
    }

    pub fn confirm_by_user(&mut self) {
        self.confirmed_by_user = true;
        self.updated_at = Utc::now();
    }

    pub fn add_contradiction(&mut self, claim_id: impl Into<String>) {
        self.contradicted_by.push(claim_id.into());
        self.updated_at = Utc::now();
    }

    pub fn record_usage_outcome(&mut self, outcome: MemoryUsageOutcome) {
        self.usage_outcomes.push(outcome);
        self.updated_at = Utc::now();
    }

    #[must_use]
    pub const fn has_supporting_evidence(&self) -> bool {
        self.confirmed_by_user || !self.evidence_events.is_empty()
    }

    #[must_use]
    pub const fn has_contradictions(&self) -> bool {
        !self.contradicted_by.is_empty()
    }

    #[must_use]
    pub fn stabilization_readiness_score(&self) -> f64 {
        let source_score = match self.source.trust_level() {
            TrustLevel::Trusted => 0.35,
            TrustLevel::Verified => 0.25,
            TrustLevel::Untrusted => 0.05,
        };
        let evidence_score = average_evidence_confidence(&self.evidence_events) * 0.25;
        let confirmation_score = if self.confirmed_by_user { 0.25 } else { 0.0 };
        let usage_score = average_usage_impact(&self.usage_outcomes) * 0.15;
        let contradiction_penalty =
            f64::from(u32::try_from(self.contradicted_by.len()).unwrap_or(u32::MAX)) * 0.20;
        (source_score + evidence_score + confirmation_score + usage_score - contradiction_penalty)
            .clamp(0.0, 1.0)
    }

    #[must_use]
    pub fn can_stabilize_as_belief(&self) -> bool {
        self.has_supporting_evidence()
            && !self.has_contradictions()
            && self.stabilization_readiness_score() >= 0.60
    }
}

impl MemoryClaim {
    #[must_use]
    pub fn new(
        subject: impl Into<String>,
        predicate: impl Into<String>,
        object: impl Into<String>,
        scope: impl Into<String>,
    ) -> Self {
        Self {
            subject: subject.into(),
            predicate: predicate.into(),
            object: object.into(),
            scope: scope.into(),
        }
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.subject.is_empty()
            && self.predicate.is_empty()
            && self.object.is_empty()
            && self.scope.is_empty()
    }
}

impl MemoryEvidence {
    #[must_use]
    pub fn new(
        event_id: impl Into<String>,
        source_type: MemorySource,
        confidence: f64,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            event_id: event_id.into(),
            source_type,
            confidence: confidence.clamp(0.0, 1.0),
            summary: summary.into(),
        }
    }
}

impl MemoryUsageOutcome {
    #[must_use]
    pub fn new(
        turn_id: impl Into<String>,
        outcome: MemoryUsageOutcomeKind,
        impact: f64,
        note: impl Into<String>,
    ) -> Self {
        Self {
            turn_id: turn_id.into(),
            outcome,
            impact: impact.clamp(-1.0, 1.0),
            recorded_at: Utc::now(),
            note: note.into(),
        }
    }
}

fn average_evidence_confidence(evidence: &[MemoryEvidence]) -> f64 {
    if evidence.is_empty() {
        return 0.0;
    }
    let sum: f64 = evidence.iter().map(|item| item.confidence).sum();
    sum / f64::from(u32::try_from(evidence.len()).unwrap_or(u32::MAX))
}

fn average_usage_impact(outcomes: &[MemoryUsageOutcome]) -> f64 {
    if outcomes.is_empty() {
        return 0.0;
    }
    let sum: f64 = outcomes.iter().map(|item| item.impact.max(0.0)).sum();
    sum / f64::from(u32::try_from(outcomes.len()).unwrap_or(u32::MAX))
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Feedback => f.write_str("feedback"),
            Self::Project => f.write_str("project"),
            Self::Reference => f.write_str("ref"),
        }
    }
}

impl MemorySource {
    #[must_use]
    pub const fn trust_level(self) -> TrustLevel {
        match self {
            Self::UserInput => TrustLevel::Trusted,
            Self::ToolOutput | Self::LlmGenerated => TrustLevel::Verified,
            Self::Network => TrustLevel::Untrusted,
        }
    }
}

impl MemoryStatus {
    /// # Errors
    /// Returns `MemoryStatusError` if the status cannot advance (terminal states).
    pub const fn try_advance(self) -> Result<Self, MemoryStatusError> {
        match self {
            Self::Captured => Ok(Self::Materialized),
            Self::Materialized => Ok(Self::Stabilized),
            Self::Stabilized | Self::Deprecated => Err(MemoryStatusError { from: self }),
        }
    }

    #[must_use]
    pub const fn deprecate(self) -> Self {
        Self::Deprecated
    }
}

impl fmt::Display for MemoryStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot advance memory status from {:?}", self.from)
    }
}

impl std::error::Error for MemoryStatusError {}

impl MemoryRelation {
    #[must_use]
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        relation_type: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            target_id: target_id.into(),
            relation_type: relation_type.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}
