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
