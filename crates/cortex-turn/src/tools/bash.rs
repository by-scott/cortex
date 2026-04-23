use super::{Tool, ToolError, ToolResult};
use std::process::Command;

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Execute a shell command via bash -c.\n\n\
         Use for: running tests, git operations, package management, directory \
         listings (ls), process management, piped commands, and any system \
         operation without a dedicated tool.\n\n\
         Do NOT use for reading files (use read), writing files (use write), or \
         editing files (use edit). Dedicated tools are safer and produce better \
         structured output.\n\n\
         Commands run as the process user with inherited environment. Both stdout \
         and stderr are captured. Non-zero exit codes are reported as errors.\n\n\
         Security: commands execute with full shell capabilities. Prefer targeted, \
         specific commands. Avoid destructive operations (rm -rf, force push, drop \
         tables) unless the collaborator explicitly requests them. State intent \
         before executing high-impact commands."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command string passed to bash -c."
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
}
