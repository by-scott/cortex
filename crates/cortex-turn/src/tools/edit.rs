use super::{Tool, ToolError, ToolResult};

pub struct EditTool;

impl Tool for EditTool {
    fn name(&self) -> &'static str {
        "edit"
    }

    fn description(&self) -> &'static str {
        "Exact search-and-replace in an existing file. Preferred for surgical edits because \
         old_string must match current content, catching stale reads. Read first, include enough \
         context for uniqueness, preserve whitespace exactly. If old_string is not found, re-read \
         and retry. Use replace_all only for intentional repeated replacements."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "File path to modify."
                },
                "old_string": {
                    "type": "string",
                    "description": "Exact current text, including whitespace."
                },
                "new_string": {
                    "type": "string",
                    "description": "Replacement text; must differ from old_string."
                },
                "replace_all": {
                    "type": "boolean",
                    "default": false,
                    "description": "Replace all occurrences instead of the first."
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

        match cortex_kernel::atomic_write(std::path::Path::new(file_path), new_content.as_bytes()) {
            Ok(()) => Ok(ToolResult::success("edit applied")),
            Err(e) => Ok(ToolResult::error(format!(
                "failed to write {file_path}: {e}"
            ))),
        }
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::WriteFile)
                .with_target("file_path")
                .with_dry_run(cortex_sdk::DryRunSupport::Supported),
        )
    }
}
