use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillParameter {
    pub name: String,
    pub description: String,
    pub required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_mode_default() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Inline);
    }

    #[test]
    fn trigger_display() {
        assert_eq!(InvocationTrigger::SlashCommand.to_string(), "slash_command");
        assert_eq!(
            InvocationTrigger::ChainedFromSkill("review".into()).to_string(),
            "chained:review"
        );
    }
}
