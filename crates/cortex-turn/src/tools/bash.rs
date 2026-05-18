use super::{Tool, ToolError, ToolResult};
use std::process::Stdio;
use std::time::Duration;

const BASH_TIMEOUT: Duration = Duration::from_mins(2);

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run a bash command for tests, builds, git, search, listings, processes, package tools, \
         file operations, and system operations. Commands run with shell power from the current \
         working directory under a curated environment, captured stdout/stderr, and a fixed \
         timeout. Non-zero exit is an error. Keep commands targeted; high-impact operations need \
         clear authority."
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

        let cwd = std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir());
        let mut process = crate::process::command_with_policy("bash", &cwd);
        process.arg("-c").arg(command).stdin(Stdio::null());

        match crate::process::run_captured(&mut process, BASH_TIMEOUT) {
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
            Err(crate::process::ProcessError::Timeout {
                timeout,
                stdout,
                stderr,
                ..
            }) => {
                let stdout = String::from_utf8_lossy(&stdout);
                let stderr = String::from_utf8_lossy(&stderr);
                let output = if stderr.is_empty() {
                    stdout.trim().to_string()
                } else {
                    stderr.trim().to_string()
                };
                Ok(ToolResult::error(format!(
                    "timed out after {}s: {output}",
                    timeout.as_secs()
                )))
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
