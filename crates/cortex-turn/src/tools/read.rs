use super::{Tool, ToolError, ToolResult};

pub struct ReadTool;

impl Tool for ReadTool {
    fn name(&self) -> &'static str {
        "read"
    }
    fn description(&self) -> &'static str {
        "Read a file and return its contents.\n\n\
         Use this tool — not bash cat/head/tail — whenever you need file contents. \
         Always read a file before modifying it with edit or write; this confirms \
         current state and prevents blind overwrites.\n\n\
         For large files, read targeted sections rather than the entire file. \
         For directory listings, use bash ls instead.\n\n\
         Supports text files of any encoding that can be represented as UTF-8. \
         Returns an error for binary files or paths that do not exist."
    }
    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read."
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
}
