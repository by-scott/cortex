use super::{Tool, ToolError, ToolResult};

pub struct WriteTool;

impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }
    fn description(&self) -> &'static str {
        "Create a file or replace an entire file. Use for new files and intentional full rewrites. \
         For partial changes use edit, which verifies old content. Read existing files first; \
         do not write secrets or credentials."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "File path. Parent directories are created if missing."
                },
                "content": {
                    "type": "string",
                    "description": "Complete file content; replaces existing content."
                }
            },
            "required": ["file_path", "content"]
        })
    }
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = input
            .get("file_path")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("file_path required".into()))?;
        let content = input
            .get("content")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("content required".into()))?;
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ToolError::ExecutionFailed(format!("create dirs: {e}")))?;
        }
        std::fs::write(path, content)
            .map(|()| ToolResult::success(format!("Wrote {} bytes to {path}", content.len())))
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::WriteFile)
                .with_target("file_path")
                .with_dry_run(cortex_sdk::DryRunSupport::Supported),
        )
    }
}
