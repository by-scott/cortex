use super::{Tool, ToolError, ToolResult};
use crate::acp_client::{AcpClient, AcpLaunch};
use cortex_types::config::{AcpClientConfig, AcpConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug)]
struct ManagedAcpClient {
    config: AcpClientConfig,
    client: Option<AcpClient>,
}

#[derive(Debug)]
pub struct AcpAgentTool {
    clients: HashMap<String, Arc<Mutex<ManagedAcpClient>>>,
    request_timeout: Duration,
}

impl AcpAgentTool {
    #[must_use]
    pub fn new(config: AcpConfig) -> Self {
        let clients = config
            .clients
            .into_iter()
            .filter(|client| !client.id.trim().is_empty())
            .map(|client| {
                (
                    client.id.clone(),
                    Arc::new(Mutex::new(ManagedAcpClient {
                        config: client,
                        client: None,
                    })),
                )
            })
            .collect();
        Self {
            clients,
            request_timeout: Duration::from_secs(config.request_timeout_secs.max(1)),
        }
    }

    #[must_use]
    pub fn is_configured(&self) -> bool {
        !self.clients.is_empty()
    }

    fn execute_prompt(
        &self,
        agent_id: &str,
        prompt: &str,
        new_session: bool,
    ) -> Result<ToolResult, ToolError> {
        let Some(client_lock) = self.clients.get(agent_id).cloned() else {
            let mut ids: Vec<String> = self.clients.keys().cloned().collect();
            ids.sort();
            return Err(ToolError::InvalidInput(format!(
                "unknown ACP agent '{agent_id}'; configured agents: {}",
                ids.join(", ")
            )));
        };

        let response = {
            let mut managed = client_lock.lock().map_err(|err| {
                ToolError::ExecutionFailed(format!("ACP client lock failed: {err}"))
            })?;
            let response =
                prompt_managed_client(&mut managed, self.request_timeout, prompt, new_session)?;
            drop(managed);
            response
        };
        let output = serde_json::json!({
            "agent_id": agent_id,
            "session_id": response.session_id,
            "stop_reason": response.stop_reason,
            "text": response.text,
        })
        .to_string();
        Ok(ToolResult::success(output))
    }
}

impl Tool for AcpAgentTool {
    fn name(&self) -> &'static str {
        "acp_agent"
    }

    fn description(&self) -> &'static str {
        "Call a configured external ACP agent through a bounded stdio JSON-RPC session. Use this \
         only when another agent process is explicitly configured for a task; provide a compact \
         prompt, choose the agent_id, and request a new_session only when prior ACP context should \
         not be reused."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "agent_id": {
                    "type": "string",
                    "description": "Configured ACP agent id from [acp].clients."
                },
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task or question to send to the ACP agent."
                },
                "new_session": {
                    "type": "boolean",
                    "default": false,
                    "description": "Create a fresh ACP session before sending this prompt."
                }
            },
            "required": ["agent_id", "prompt"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let agent_id = input
            .get("agent_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ToolError::InvalidInput("missing agent_id".to_string()))?;
        let prompt = input
            .get("prompt")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ToolError::InvalidInput("missing prompt".to_string()))?;
        let new_session = input
            .get("new_session")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        self.execute_prompt(agent_id, prompt, new_session)
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::DelegateWork)
                .with_target("acp_agent"),
        )
    }
}

fn prompt_managed_client(
    managed: &mut ManagedAcpClient,
    request_timeout: Duration,
    prompt: &str,
    new_session: bool,
) -> Result<crate::acp_client::AcpPromptResponse, ToolError> {
    if managed.client.as_mut().is_none_or(|client| {
        client
            .is_alive()
            .map_or(true, |alive| !alive || client.session_id().is_none())
    }) {
        managed.client = Some(spawn_configured_client(&managed.config, request_timeout)?);
    }

    let cwd = resolve_cwd(&managed.config.cwd)?;
    let client = managed
        .client
        .as_mut()
        .ok_or_else(|| ToolError::ExecutionFailed("ACP client failed to initialize".to_string()))?;
    if new_session || client.session_id().is_none() {
        client
            .new_session(&cwd)
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
    }
    client
        .prompt(prompt)
        .map_err(|err| ToolError::ExecutionFailed(err.to_string()))
}

fn spawn_configured_client(
    config: &AcpClientConfig,
    request_timeout: Duration,
) -> Result<AcpClient, ToolError> {
    let launch = AcpLaunch::new(config.id.clone(), config.command.clone())
        .with_args(config.args.clone())
        .with_cwd(resolve_cwd(&config.cwd)?)
        .with_env(config.env.clone())
        .with_request_timeout(request_timeout);
    AcpClient::spawn(launch).map_err(|err| ToolError::ExecutionFailed(err.to_string()))
}

fn resolve_cwd(cwd: &str) -> Result<PathBuf, ToolError> {
    let path = if cwd.trim().is_empty() {
        PathBuf::from(".")
    } else {
        PathBuf::from(cwd)
    };
    if path.is_absolute() {
        Ok(path)
    } else {
        std::env::current_dir()
            .map(|current| current.join(path))
            .map_err(|err| ToolError::ExecutionFailed(format!("resolve ACP cwd: {err}")))
    }
}

#[cfg(test)]
mod tests {
    use super::AcpAgentTool;
    use crate::tools::Tool;
    use cortex_types::config::{AcpClientConfig, AcpConfig};
    use std::time::Duration;

    #[test]
    fn acp_agent_tool_registers_only_configured_clients() {
        let tool = AcpAgentTool::new(AcpConfig {
            clients: vec![AcpClientConfig {
                id: "worker".to_string(),
                command: "/bin/sh".to_string(),
                args: Vec::new(),
                cwd: ".".to_string(),
                env: std::collections::HashMap::new(),
            }],
            request_timeout_secs: 5,
        });
        assert!(tool.is_configured());
        assert_eq!(tool.name(), "acp_agent");
        assert!(tool.input_schema()["required"].as_array().is_some());
        assert!(tool.capabilities().effects.iter().any(|effect| {
            effect.kind == cortex_sdk::ToolEffectKind::DelegateWork && effect.target == "acp_agent"
        }));
    }

    #[test]
    fn acp_agent_tool_runs_configured_agent() {
        let temp = tempfile::tempdir().unwrap_or_else(|err| panic!("tempdir: {err}"));
        let script_path = temp.path().join("agent.sh");
        std::fs::write(
            &script_path,
            r#"#!/bin/sh
i=0
while IFS= read -r line; do
  i=$((i + 1))
  if [ "$i" -eq 1 ]; then
    printf '%s\n' '{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":1}}'
  elif [ "$i" -eq 2 ]; then
    printf '%s\n' '{"jsonrpc":"2.0","id":2,"result":{"sessionId":"tool-session"}}'
  elif [ "$i" -eq 3 ]; then
    printf '%s\n' '{"jsonrpc":"2.0","method":"session/update","params":{"update":{"text":"delegated"}}}'
    printf '%s\n' '{"jsonrpc":"2.0","id":3,"result":{"stopReason":"end_turn"}}'
  fi
done
"#,
        )
        .unwrap_or_else(|err| panic!("write script: {err}"));
        let tool = AcpAgentTool::new(AcpConfig {
            clients: vec![AcpClientConfig {
                id: "worker".to_string(),
                command: "/bin/sh".to_string(),
                args: vec![script_path.to_string_lossy().to_string()],
                cwd: temp.path().to_string_lossy().to_string(),
                env: std::collections::HashMap::new(),
            }],
            request_timeout_secs: Duration::from_secs(5).as_secs(),
        });
        let result = tool
            .execute(serde_json::json!({
                "agent_id": "worker",
                "prompt": "inspect",
            }))
            .unwrap_or_else(|err| panic!("execute acp tool: {err}"));
        assert!(!result.is_error);
        assert!(result.output.contains(r#""session_id":"tool-session""#));
        assert!(result.output.contains(r#""text":"delegated""#));
    }
}
