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
        "Start a delegated worker turn in an isolated context.\n\n\
         Modes:\n\
         - readonly: Investigation only — read, search, analyze. No file mutations. \
         Use for research, codebase exploration, gathering information.\n\
         - full: Complete tool access. Use for independent implementation tasks \
         that do not need parent context.\n\
         - fork: Inherits parent conversation history. Use when the worker \
         needs full context to continue work accurately.\n\
         - teammate: Parallel coordination via messaging. Use for decomposing \
         large tasks across multiple workers running simultaneously.\n\n\
         Each delegated worker is a full cognitive turn — treat delegation as an \
         investment. Write clear, self-contained prompts with specific \
         deliverables. The worker does not share your context unless \
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
                    "description": "Complete task description for the delegated worker. Must be self-contained."
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
                },
                "scope": {
                    "type": "string",
                    "description": "Boundary of work the delegated worker may perform."
                },
                "allowed_tools": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Explicit tool names the worker may use. Empty means no tools."
                },
                "forbidden_actions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Tool or action names the worker must not perform."
                },
                "token_budget": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum worker output/input budget enforced by the delegation contract."
                },
                "iteration_budget": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum worker LLM-tool loop iterations."
                },
                "evidence_budget": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Maximum evidence items the worker may consume."
                },
                "allowed_evidence": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Evidence ids, paths, or citation keys explicitly visible to the worker."
                },
                "expected_artifact": {
                    "type": "string",
                    "description": "Artifact the worker must return for merge review."
                },
                "merge_verifier": {
                    "type": "string",
                    "description": "Verifier or review rule used before parent merge."
                },
                "review_required": {
                    "type": "boolean",
                    "default": true,
                    "description": "Whether parent review is required before applying the result."
                },
                "inherit_parent_authority": {
                    "type": "boolean",
                    "default": false,
                    "description": "Whether the worker may inherit parent authority. This requires explicit tool grants."
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
            .unwrap_or("delegated worker");

        // This execute is a fallback -- the orchestrator intercepts delegation calls
        // and runs delegated turns directly. This path is only reached if called outside
        // the orchestrator (e.g., direct Tool::execute tests).
        Ok(ToolResult::success(format!(
            "[Worker '{description}' ({mode:?} mode)] Task: {prompt}. \
             (Direct execution -- orchestrator handles delegated turn execution)"
        )))
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::DelegateWork)
                .with_target("prompt"),
        )
    }
}
