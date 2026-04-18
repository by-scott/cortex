use super::{Tool, ToolError, ToolResult};

pub struct WriteTool;

impl Tool for WriteTool {
    fn name(&self) -> &'static str {
        "write"
    }
    fn description(&self) -> &'static str {
        "Create a new file or completely replace an existing file's contents.\n\n\
         Use for: creating files that do not yet exist, or full rewrites where \
         the entire file changes. Parent directories are created automatically.\n\n\
         Do NOT use for partial modifications — use the edit tool instead. Edit \
         is safer for surgical changes because it verifies the old content matches \
         before replacing, catching stale-state errors that write cannot detect.\n\n\
         Always read an existing file before overwriting it to confirm you understand \
         what will be lost. Never write files containing secrets or credentials."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative path. Parent directories created if missing."
                },
                "content": {
                    "type": "string",
                    "description": "Complete file content. Replaces everything if the file exists."
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
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn write_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("out.txt");
        let r = WriteTool
            .execute(serde_json::json!({"file_path": path.to_str().unwrap(), "content": "hello"}))
            .unwrap();
        assert!(!r.is_error);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello");
    }
}
