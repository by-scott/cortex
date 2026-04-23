use std::sync::Arc;

use super::SkillRegistry;
use crate::tools::{Tool, ToolError, ToolResult};

/// `SkillTool` — Tool bridge for agent autonomous skill invocation.
pub struct SkillTool {
    registry: Arc<SkillRegistry>,
}

impl SkillTool {
    #[must_use]
    pub const fn new(registry: Arc<SkillRegistry>) -> Self {
        Self { registry }
    }
}

impl Tool for SkillTool {
    fn name(&self) -> &'static str {
        "skill"
    }

    fn description(&self) -> &'static str {
        "Activate a structured reasoning protocol.\n\n\
         Skills provide systematic cognitive strategies for problems that \
         defeat intuitive reasoning: complex decisions (/deliberate), \
         debugging (/diagnose), code review (/review), unfamiliar territory \
         (/orient), and task decomposition (/plan).\n\n\
         Some skills auto-activate on metacognitive alerts (doom loop, \
         frame anchoring). You can also invoke them proactively when you \
         recognize the cognitive demand before an alert fires.\n\n\
         The skill's structured output replaces ad-hoc reasoning with a \
         tested protocol. Use skills when the problem is important enough \
         to warrant systematic analysis rather than quick intuition."
    }

    fn input_schema(&self) -> serde_json::Value {
        let names: Vec<String> = self.registry.names();
        serde_json::json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "enum": names,
                    "description": "Skill name. Use without leading slash."
                },
                "args": {
                    "type": "string",
                    "description": "Problem statement or target to analyze."
                }
            },
            "required": ["skill"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let skill_name = input
            .get("skill")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing 'skill'".into()))?
            .trim()
            .trim_start_matches('/');
        let args = input.get("args").and_then(|v| v.as_str()).unwrap_or("");

        let Some(rendered) = self.registry.render(skill_name, args) else {
            return Ok(ToolResult::error(format!(
                "Unknown skill: '{skill_name}'. Available: {}",
                self.registry.names().join(", ")
            )));
        };
        let super::SkillContent::Markdown(content) = rendered.content;

        Ok(ToolResult::success(content))
    }
}
