use super::{Tool, ToolError, ToolResult};

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Perform exact search-and-replace within a file.\n\n\
         Preferred tool for modifying existing files. Safer than write because it \
         verifies old_string exists before replacing — a failed match signals that \
         the file has changed since you last read it.\n\n\
         Critical: old_string must match the file content exactly, including \
         whitespace and indentation. Always read the file first to get the precise \
         text. Include enough surrounding context in old_string to make the match \
         unique; if multiple matches exist and replace_all is false, only the first \
         is replaced.\n\n\
         Common failure: old_string not found. Causes: stale content (file changed), \
         whitespace mismatch (tabs vs spaces), encoding differences. Fix: re-read \
         the file and retry with exact content.\n\n\
         Use replace_all: true for renaming a variable or string across the file."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "Path to the file to modify."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact text to find. Must match file content verbatim including whitespace."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text. Must differ from old_string."
                },
                "replace_all": {
                    "type": "boolean",
                    "default": false,
                    "description": "Replace all occurrences (true) or only the first (false)."
                }
            },
            "required": ["file_path", "old_string", "new_string"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let file_path = input
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing file_path".into()))?;
        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing old_string".into()))?;
        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing new_string".into()))?;
        let replace_all = input
            .get("replace_all")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                return Ok(ToolResult::error(format!(
                    "failed to read {file_path}: {e}"
                )));
            }
        };

        if !content.contains(old_string) {
            return Ok(ToolResult::error(format!(
                "old_string not found in {file_path}"
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        match std::fs::write(file_path, &new_content) {
            Ok(()) => Ok(ToolResult::success("edit applied")),
            Err(e) => Ok(ToolResult::error(format!(
                "failed to write {file_path}: {e}"
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_replace_once() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "foo bar foo").unwrap();

        let result = EditTool
            .execute(serde_json::json!({
                "file_path": path.to_str().unwrap(),
                "old_string": "foo",
                "new_string": "baz"
            }))
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "baz bar foo");
    }

    #[test]
    fn edit_replace_all() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "foo bar foo").unwrap();

        let result = EditTool
            .execute(serde_json::json!({
                "file_path": path.to_str().unwrap(),
                "old_string": "foo",
                "new_string": "baz",
                "replace_all": true
            }))
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "baz bar baz");
    }

    #[test]
    fn edit_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.txt");
        std::fs::write(&path, "hello").unwrap();

        let result = EditTool
            .execute(serde_json::json!({
                "file_path": path.to_str().unwrap(),
                "old_string": "xyz",
                "new_string": "abc"
            }))
            .unwrap();
        assert!(result.is_error);
    }
}
