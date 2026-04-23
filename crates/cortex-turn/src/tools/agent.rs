use super::{Tool, ToolError, ToolResult};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentMode {
    Readonly,
    Full,
    Fork,
    Teammate,
}

pub struct AgentTool;

impl Tool for AgentTool {
    fn name(&self) -> &'static str {
        "agent"
    }

    fn description(&self) -> &'static str {
        "Spawn a sub-agent to handle a task in an isolated context.\n\n\
         Modes:\n\
         - readonly: Investigation only — read, search, analyze. No file mutations. \
         Use for research, codebase exploration, gathering information.\n\
         - full: Complete tool access. Use for independent implementation tasks \
         that do not need parent context.\n\
         - fork: Inherits parent conversation history. Use when the sub-agent \
         needs full context to continue work accurately.\n\
         - teammate: Parallel coordination via messaging. Use for decomposing \
         large tasks across multiple agents working simultaneously.\n\n\
         Each sub-agent is a full cognitive turn — treat delegation as an \
         investment. Write clear, self-contained prompts with specific \
         deliverables. The sub-agent does not share your context unless \
         mode is fork.\n\n\
         Maximum nesting depth: 3 levels. Prefer readonly when mutation is \
         not required — it is cheaper and safer."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Complete task description for the sub-agent. Must be self-contained."
                },
                "description": {
                    "type": "string",
                    "description": "Short label for tracking (e.g. 'auth-search', 'test-runner')."
                },
                "mode": {
                    "type": "string",
                    "enum": ["readonly", "full", "fork", "teammate"],
                    "default": "readonly",
                    "description": "Capability level: readonly (investigate), full (implement), fork (with context), teammate (parallel)."
                },
                "team_name": {
                    "type": "string",
                    "description": "Coordination group name. Required for teammate mode."
                }
            },
            "required": ["prompt"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing prompt".into()))?;

        let mode_str = input
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("readonly");

        let mode: AgentMode = match mode_str {
            "readonly" => AgentMode::Readonly,
            "full" => AgentMode::Full,
            "fork" => AgentMode::Fork,
            "teammate" => AgentMode::Teammate,
            _ => {
                return Err(ToolError::InvalidInput(format!("unknown mode: {mode_str}")));
            }
        };

        let description = input
            .get("description")
            .and_then(|v| v.as_str())
            .unwrap_or("sub-agent");

        // This execute is a fallback -- the orchestrator intercepts agent tool calls
        // and runs sub-Turns directly. This path is only reached if called outside
        // the orchestrator (e.g., direct Tool::execute tests).
        Ok(ToolResult::success(format!(
            "[Agent '{description}' ({mode:?} mode)] Task: {prompt}. \
             (Direct execution -- orchestrator handles sub-Turn execution)"
        )))
    }
}
