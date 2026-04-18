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

        let Some(content) = self.registry.with_skill(skill_name, |s| {
            let super::SkillContent::Markdown(c) = s.content(args);
            c
        }) else {
            return Ok(ToolResult::error(format!(
                "Unknown skill: '{skill_name}'. Available: {}",
                self.registry.names().join(", ")
            )));
        };

        Ok(ToolResult::success(content))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills::defaults::ensure_system_skills;
    use crate::skills::loader::load_skills;
    use cortex_types::SkillSource;

    fn setup() -> Arc<SkillRegistry> {
        let dir = tempfile::tempdir().unwrap();
        let sys = dir.path().join("system");
        ensure_system_skills(&sys);
        let reg = SkillRegistry::new();
        for s in load_skills(&sys, &SkillSource::System) {
            reg.register(s);
        }
        std::mem::forget(dir);
        Arc::new(reg)
    }

    #[test]
    fn returns_content() {
        let tool = SkillTool::new(setup());
        let r = tool
            .execute(serde_json::json!({"skill": "deliberate", "args": "design a cache"}))
            .unwrap();
        assert!(!r.is_error);
        assert!(r.output.contains("design a cache"));
    }

    #[test]
    fn unknown_skill() {
        let tool = SkillTool::new(Arc::new(SkillRegistry::new()));
        let r = tool.execute(serde_json::json!({"skill": "nope"})).unwrap();
        assert!(r.is_error);
    }

    #[test]
    fn strips_slash() {
        let tool = SkillTool::new(setup());
        let r = tool
            .execute(serde_json::json!({"skill": "/deliberate"}))
            .unwrap();
        assert!(!r.is_error);
    }
}
