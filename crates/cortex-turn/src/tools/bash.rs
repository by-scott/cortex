use super::{Tool, ToolError, ToolResult};
use std::process::Command;

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a bash command for tests, builds, git, search, listings, processes, package tools, \
         and system operations without a dedicated tool. Prefer read/write/edit for file content \
         changes. Commands run with shell power and inherited environment; stdout/stderr are \
         captured and non-zero exit is an error. Keep commands targeted; high-impact operations \
         need clear authority."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Command passed to bash -c."
                }
            },
            "required": ["command"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing command".into()))?;

        match Command::new("bash").arg("-c").arg(command).output() {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    let mut result = stdout.trim().to_string();
                    if !stderr.is_empty() {
                        result.push_str("\n[stderr] ");
                        result.push_str(stderr.trim());
                    }
                    Ok(ToolResult::success(result))
                } else {
                    let msg = if stderr.is_empty() {
                        stdout.trim().to_string()
                    } else {
                        stderr.trim().to_string()
                    };
                    Ok(ToolResult::error(format!(
                        "exit code {}: {msg}",
                        output.status.code().unwrap_or(-1),
                    )))
                }
            }
            Err(e) => Ok(ToolResult::error(format!("failed to execute: {e}"))),
        }
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::RunProcess)
                .with_target("command"),
        )
    }
}
