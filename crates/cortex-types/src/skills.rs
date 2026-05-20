use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use chrono::{DateTime, Utc};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionMode {
    #[default]
    Inline,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SkillSource {
    System,
    Instance,
    Plugin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InvocationTrigger {
    SlashCommand,
    NaturalLanguage,
    AgentAutonomous,
    ChainedFromSkill(String),
    MetacognitiveAlert(String),
    Lifecycle(String),
    ApiRpc,
    McpProtocol,
    SignalDriven(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillParameter {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillMetadata {
    pub source: SkillSource,
    pub version: Option<String>,
    pub tags: Vec<String>,
    pub user_invocable: bool,
    pub agent_invocable: bool,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillSummary {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillHealthState {
    Strong,
    Healthy,
    NeedsReview,
    Quarantined,
    Deprecated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillHealth {
    pub name: String,
    pub state: SkillHealthState,
    pub score: f64,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_skill: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillEvolutionRelation {
    NewPattern,
    Improves,
    AlternativeTo,
    CandidateReplacement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillEvolutionProposalStatus {
    Proposed,
    Accepted,
    Rejected,
    Superseded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillEvolutionProposal {
    pub id: String,
    pub relation: SkillEvolutionRelation,
    pub candidate_skill: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_skill: Option<String>,
    pub reason: String,
    pub evidence: Vec<String>,
    pub status: SkillEvolutionProposalStatus,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillManifest {
    pub name: String,
    pub description: String,
    pub version: Option<String>,
    pub source: SkillSource,
    pub preconditions: Vec<String>,
    pub inputs: Vec<SkillParameter>,
    pub outputs: Vec<String>,
    pub effects: Vec<String>,
    pub required_tools: Vec<String>,
    pub risk: f32,
    pub expected_duration_secs: Option<u64>,
    pub success_criteria: Vec<String>,
    pub fallback: Option<String>,
    pub observability: Vec<String>,
    pub user_invocable: bool,
    pub agent_invocable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillTraceStatus {
    Started,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SkillExecutionTrace {
    pub trace_id: String,
    pub skill_name: String,
    pub trigger: String,
    pub execution_mode: ExecutionMode,
    pub status: SkillTraceStatus,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completed_at: Option<DateTime<Utc>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    pub input_summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_summary: Option<String>,
    pub required_tools: Vec<String>,
    pub effects: Vec<String>,
    pub risk: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SkillActivation {
    #[serde(default)]
    pub input_patterns: Vec<String>,
    pub pressure_above: Option<String>,
    #[serde(default)]
    pub alert_kinds: Vec<String>,
    #[serde(default)]
    pub event_kinds: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SkillInvocation {
    pub skill_name: String,
    pub arguments: HashMap<String, serde_json::Value>,
    pub trigger: InvocationTrigger,
}

impl Default for SkillMetadata {
    fn default() -> Self {
        Self {
            source: SkillSource::Instance,
            version: None,
            tags: Vec::new(),
            user_invocable: true,
            agent_invocable: true,
            path: None,
        }
    }
}

impl fmt::Display for InvocationTrigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlashCommand => write!(f, "slash_command"),
            Self::NaturalLanguage => write!(f, "natural_language"),
            Self::AgentAutonomous => write!(f, "agent_autonomous"),
            Self::ChainedFromSkill(s) => write!(f, "chained:{s}"),
            Self::MetacognitiveAlert(s) => write!(f, "metacognitive:{s}"),
            Self::Lifecycle(s) => write!(f, "lifecycle:{s}"),
            Self::ApiRpc => write!(f, "api_rpc"),
            Self::McpProtocol => write!(f, "mcp"),
            Self::SignalDriven(s) => write!(f, "signal:{s}"),
        }
    }
}

impl SkillManifest {
    #[must_use]
    pub fn basic(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            version: None,
            source: SkillSource::Instance,
            preconditions: Vec::new(),
            inputs: Vec::new(),
            outputs: vec!["markdown_context".to_string()],
            effects: vec!["context_injection".to_string()],
            required_tools: Vec::new(),
            risk: 0.05,
            expected_duration_secs: None,
            success_criteria: vec!["skill content rendered without error".to_string()],
            fallback: Some("continue without this skill and rely on base protocol".to_string()),
            observability: vec![
                "SkillInvoked".to_string(),
                "SkillCompleted".to_string(),
                "utility_ewma".to_string(),
            ],
            user_invocable: true,
            agent_invocable: true,
        }
    }
}

impl SkillHealth {
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            state: SkillHealthState::Healthy,
            score: 0.5,
            consecutive_failures: 0,
            consecutive_successes: 0,
            reason: "not enough execution history".to_string(),
            related_skill: None,
            updated_at: Utc::now(),
        }
    }
}

impl SkillHealthState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Strong => "strong",
            Self::Healthy => "healthy",
            Self::NeedsReview => "needs_review",
            Self::Quarantined => "quarantined",
            Self::Deprecated => "deprecated",
        }
    }

    #[must_use]
    pub const fn allows_automatic_activation(self) -> bool {
        matches!(self, Self::Strong | Self::Healthy | Self::NeedsReview)
    }
}

impl SkillEvolutionProposal {
    #[must_use]
    pub fn new(
        relation: SkillEvolutionRelation,
        candidate_skill: impl Into<String>,
        target_skill: Option<String>,
        reason: impl Into<String>,
        evidence: Vec<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            relation,
            candidate_skill: candidate_skill.into(),
            target_skill,
            reason: reason.into(),
            evidence,
            status: SkillEvolutionProposalStatus::Proposed,
            created_at: Utc::now(),
        }
    }
}

impl SkillEvolutionRelation {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::NewPattern => "new_pattern",
            Self::Improves => "improves",
            Self::AlternativeTo => "alternative_to",
            Self::CandidateReplacement => "candidate_replacement",
        }
    }
}

impl SkillEvolutionProposalStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Proposed => "proposed",
            Self::Accepted => "accepted",
            Self::Rejected => "rejected",
            Self::Superseded => "superseded",
        }
    }
}

impl SkillExecutionTrace {
    #[must_use]
    pub fn started(
        trace_id: impl Into<String>,
        skill_name: impl Into<String>,
        trigger: impl Into<String>,
        execution_mode: ExecutionMode,
        input_summary: impl Into<String>,
    ) -> Self {
        Self {
            trace_id: trace_id.into(),
            skill_name: skill_name.into(),
            trigger: trigger.into(),
            execution_mode,
            status: SkillTraceStatus::Started,
            started_at: Utc::now(),
            completed_at: None,
            duration_ms: None,
            input_summary: input_summary.into(),
            output_summary: None,
            error_summary: None,
            required_tools: Vec::new(),
            effects: Vec::new(),
            risk: 0.0,
        }
    }

    #[must_use]
    pub fn with_manifest(mut self, manifest: &SkillManifest) -> Self {
        self.required_tools.clone_from(&manifest.required_tools);
        self.effects.clone_from(&manifest.effects);
        self.risk = manifest.risk;
        self
    }

    #[must_use]
    pub fn complete(
        mut self,
        success: bool,
        duration_ms: u64,
        output_summary: impl Into<String>,
    ) -> Self {
        self.status = if success {
            SkillTraceStatus::Succeeded
        } else {
            SkillTraceStatus::Failed
        };
        let summary = output_summary.into();
        if success {
            self.output_summary = Some(summary);
        } else {
            self.error_summary = Some(summary);
        }
        self.duration_ms = Some(duration_ms);
        self.completed_at = Some(Utc::now());
        self
    }
}
