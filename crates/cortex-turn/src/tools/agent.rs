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
        "Delegate a bounded worker turn. Use readonly for investigation, full for isolated \
         implementation, fork when parent history is required, teammate for coordinated parallel \
         work. Prompts must be self-contained with scope, deliverable, verification, and limits. \
         Workers do not inherit context unless forked; maximum nesting depth is 3."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Self-contained worker task with deliverable and verification."
                },
                "description": {
                    "type": "string",
                    "description": "Short label for tracking (e.g. 'auth-search', 'test-runner')."
                },
                "mode": {
                    "type": "string",
                    "enum": ["readonly", "full", "fork", "teammate"],
                    "default": "readonly",
                    "description": "readonly, full, fork, or teammate."
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
                    "description": "Tool names the worker may use. Empty means no tools."
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
                    "description": "Whether worker may inherit parent authority; still requires explicit grants."
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
