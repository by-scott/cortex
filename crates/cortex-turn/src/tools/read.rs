use super::{Tool, ToolError, ToolResult};

pub struct ReadTool;

impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        "Read a UTF-8 text file. Use before edit/write to ground changes in current state. \
         Prefer targeted reads for large files; use bash for directory listings or search. \
         Fails for missing paths, binary data, or unreadable files."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative file path."
                }
            },
            "required": ["file_path"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = input
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("file_path required".into()))?;
        std::fs::read_to_string(path)
            .map(ToolResult::success)
            .map_err(|e| ToolError::ExecutionFailed(format!("{path}: {e}")))
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::ReadFile)
                .with_target("file_path"),
        )
    }
}
