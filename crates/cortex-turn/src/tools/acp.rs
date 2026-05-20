use super::{Tool, ToolError, ToolResult};
use crate::acp_client::{AcpClient, AcpInitializeFormat, AcpLaunch};
use cortex_types::config::{AcpClientConfig, AcpConfig};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

#[derive(Debug)]
struct ManagedAcpClient {
    config: AcpClientConfig,
    client: Option<AcpClient>,
}

#[derive(Debug, Clone)]
struct AcpConfigPersistence {
    config_path: PathBuf,
    write_lock: Arc<Mutex<()>>,
}

#[derive(Debug)]
pub struct AcpTool {
    pool: Arc<AcpConnectionPool>,
}

#[derive(Debug)]
struct AcpConnectionPool {
    clients: RwLock<HashMap<String, Arc<Mutex<ManagedAcpClient>>>>,
    persistence: Option<AcpConfigPersistence>,
    operation_lock: Mutex<()>,
    request_timeout: Duration,
}

impl AcpTool {
    #[must_use]
    pub fn new(config: AcpConfig) -> Self {
        Self {
            pool: Arc::new(AcpConnectionPool::new(config, None)),
        }
    }

    #[must_use]
    pub fn with_config_path(config: AcpConfig, config_path: PathBuf) -> Self {
        Self {
            pool: Arc::new(AcpConnectionPool::new(config, Some(config_path))),
        }
    }
}

impl AcpConnectionPool {
    #[must_use]
    fn new(config: AcpConfig, config_path: Option<PathBuf>) -> Self {
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
            clients: RwLock::new(clients),
            persistence: config_path.map(|config_path| AcpConfigPersistence {
                config_path,
                write_lock: Arc::new(Mutex::new(())),
            }),
            operation_lock: Mutex::new(()),
            request_timeout: Duration::from_secs(config.request_timeout_secs.max(1)),
        }
    }

    #[must_use]
    fn list(&self) -> serde_json::Value {
        let mut agents: Vec<serde_json::Value> = self
            .clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(id, client)| {
                let managed = client
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                serde_json::json!({
                    "agent_id": id,
                    "ssh_host": empty_as_none(&managed.config.ssh_host),
                    "initialize_format": managed.config.initialize_format,
                    "protocol_version": managed.config.protocol_version,
                    "client_name": managed.config.client_name,
                    "client_version": empty_as_none(&managed.config.client_version),
                    "command": managed.config.command,
                    "args": managed.config.args,
                    "cwd": managed.config.cwd,
                    "env": sorted_env_keys(&managed.config.env),
                    "connected": managed.client.is_some(),
                    "session_id": managed.client.as_ref().and_then(AcpClient::session_id),
                })
            })
            .collect();
        agents.sort_by(|left, right| {
            left.get("agent_id")
                .and_then(serde_json::Value::as_str)
                .cmp(&right.get("agent_id").and_then(serde_json::Value::as_str))
        });
        serde_json::json!({ "agents": agents })
    }

    fn status(&self, agent_id: Option<&str>) -> Result<serde_json::Value, ToolError> {
        if let Some(agent_id) = agent_id {
            return self.status_one(agent_id);
        }
        let mut statuses = Vec::new();
        for id in self.agent_ids() {
            statuses.push(self.status_one(&id)?);
        }
        Ok(serde_json::json!({ "agents": statuses }))
    }

    fn add(&self, config: AcpClientConfig) -> Result<serde_json::Value, ToolError> {
        validate_client_config(&config)?;
        let agent_id = config.id.clone();
        let _operation = self.operation_guard()?;
        let snapshot = self.snapshot_with_upsert(config.clone());
        self.persist_clients(&snapshot)?;
        self.clients
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                agent_id.clone(),
                Arc::new(Mutex::new(ManagedAcpClient {
                    config,
                    client: None,
                })),
            );
        Ok(serde_json::json!({
            "agent_id": agent_id,
            "configured": true,
            "connected": false,
            "persistent": self.persistence.is_some(),
        }))
    }

    fn remove(&self, agent_id: &str) -> Result<serde_json::Value, ToolError> {
        let _operation = self.operation_guard()?;
        if !self.has_agent(agent_id) {
            let ids = self.agent_ids();
            return Err(unknown_agent(agent_id, &ids));
        }
        let snapshot = self.snapshot_without(agent_id);
        self.persist_clients(&snapshot)?;
        self.clients
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(agent_id);
        Ok(serde_json::json!({
            "agent_id": agent_id,
            "configured": false,
            "connected": false,
            "persistent": self.persistence.is_some(),
        }))
    }

    fn connect(&self, agent_id: &str, new_session: bool) -> Result<serde_json::Value, ToolError> {
        let client_lock = self.client_lock(agent_id)?;
        let mut managed = client_lock
            .lock()
            .map_err(|err| ToolError::ExecutionFailed(format!("ACP client lock failed: {err}")))?;
        let session_id = ensure_connected(&mut managed, self.request_timeout, new_session)?;
        drop(managed);
        Ok(serde_json::json!({
            "agent_id": agent_id,
            "connected": true,
            "session_id": session_id,
        }))
    }

    fn disconnect(&self, agent_id: Option<&str>) -> Result<serde_json::Value, ToolError> {
        if let Some(agent_id) = agent_id {
            let client_lock = self.client_lock(agent_id)?;
            let mut managed = client_lock.lock().map_err(|err| {
                ToolError::ExecutionFailed(format!("ACP client lock failed: {err}"))
            })?;
            managed.client = None;
            drop(managed);
            return Ok(serde_json::json!({
                "agent_id": agent_id,
                "connected": false,
            }));
        }

        let clients: Vec<Arc<Mutex<ManagedAcpClient>>> = self
            .clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .cloned()
            .collect();
        for client in clients {
            if let Ok(mut managed) = client.lock() {
                managed.client = None;
            }
        }
        Ok(serde_json::json!({ "connected": false, "scope": "all" }))
    }

    fn prompt(
        &self,
        agent_id: &str,
        prompt: &str,
        new_session: bool,
    ) -> Result<ToolResult, ToolError> {
        let client_lock = self.client_lock(agent_id)?;

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

    fn status_one(&self, agent_id: &str) -> Result<serde_json::Value, ToolError> {
        let client_lock = self.client_lock(agent_id)?;
        let mut managed = client_lock
            .lock()
            .map_err(|err| ToolError::ExecutionFailed(format!("ACP client lock failed: {err}")))?;
        let alive = managed
            .client
            .as_mut()
            .is_some_and(|client| client.is_alive().unwrap_or(false));
        let session_id = managed
            .client
            .as_ref()
            .and_then(AcpClient::session_id)
            .map(str::to_string);
        Ok(serde_json::json!({
            "agent_id": agent_id,
            "configured": true,
            "connected": alive,
            "session_id": session_id,
            "ssh_host": empty_as_none(&managed.config.ssh_host),
            "initialize_format": managed.config.initialize_format,
            "protocol_version": managed.config.protocol_version,
            "client_name": managed.config.client_name,
            "client_version": empty_as_none(&managed.config.client_version),
            "command": managed.config.command,
            "args": managed.config.args,
            "cwd": managed.config.cwd,
            "env": sorted_env_keys(&managed.config.env),
        }))
    }

    fn client_lock(&self, agent_id: &str) -> Result<Arc<Mutex<ManagedAcpClient>>, ToolError> {
        let clients = self
            .clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(client) = clients.get(agent_id).cloned() {
            return Ok(client);
        }
        drop(clients);
        let ids = self.agent_ids();
        Err(unknown_agent(agent_id, &ids))
    }

    fn agent_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self
            .clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .keys()
            .cloned()
            .collect();
        ids.sort();
        ids
    }

    fn has_agent(&self, agent_id: &str) -> bool {
        self.clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(agent_id)
    }

    fn operation_guard(&self) -> Result<std::sync::MutexGuard<'_, ()>, ToolError> {
        self.operation_lock
            .lock()
            .map_err(|err| ToolError::ExecutionFailed(format!("ACP operation lock failed: {err}")))
    }

    fn snapshot_with_upsert(&self, config: AcpClientConfig) -> Vec<AcpClientConfig> {
        let mut clients = self.snapshot_configs();
        if let Some(existing) = clients.iter_mut().find(|client| client.id == config.id) {
            *existing = config;
        } else {
            clients.push(config);
        }
        clients.sort_by(|left, right| left.id.cmp(&right.id));
        clients
    }

    fn snapshot_without(&self, agent_id: &str) -> Vec<AcpClientConfig> {
        self.snapshot_configs()
            .into_iter()
            .filter(|client| client.id != agent_id)
            .collect()
    }

    fn snapshot_configs(&self) -> Vec<AcpClientConfig> {
        let mut clients: Vec<AcpClientConfig> = self
            .clients
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .values()
            .map(|client| {
                client
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .config
                    .clone()
            })
            .collect();
        clients.sort_by(|left, right| left.id.cmp(&right.id));
        clients
    }

    fn persist_clients(&self, clients: &[AcpClientConfig]) -> Result<(), ToolError> {
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };
        let _write = persistence.write_lock.lock().map_err(|err| {
            ToolError::ExecutionFailed(format!("ACP config persistence lock failed: {err}"))
        })?;
        cortex_kernel::update_acp_clients(&persistence.config_path, clients)
            .map_err(ToolError::ExecutionFailed)
    }
}

impl Tool for AcpTool {
    fn name(&self) -> &'static str {
        "acp"
    }

    fn description(&self) -> &'static str {
        "Manage and talk to configured external ACP agents. Use list/status to inspect configured \
         agents, connect to actively start an ACP agent session, disconnect to stop one, and prompt \
         to send a compact task or question to an external agent."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["add", "remove", "list", "status", "connect", "disconnect", "prompt"],
                    "description": "ACP client action to run."
                },
                "agent_id": {
                    "type": "string",
                    "description": "ACP agent id from configured clients or a prior add action. Required for connect and prompt; optional for status and disconnect."
                },
                "prompt": {
                    "type": "string",
                    "description": "Self-contained task or question to send to the ACP agent. Required for prompt."
                },
                "command": {
                    "type": "string",
                    "description": "ACP command to launch for add, for example 'codex'."
                },
                "args": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Command arguments for add."
                },
                "cwd": {
                    "type": "string",
                    "description": "Local cwd for add, or remote cwd when ssh_host is set."
                },
                "env": {
                    "type": "object",
                    "additionalProperties": {"type": "string"},
                    "description": "Environment variables for a local ACP process. With ssh_host these are exported in the remote shell command."
                },
                "ssh_host": {
                    "type": "string",
                    "description": "Optional SSH target. Supports host, user@host, host:port, user@host:port, and [ipv6]:port. When set, add launches ssh and executes the ACP command remotely over stdio."
                },
                "initialize_format": {
                    "type": "string",
                    "enum": ["standard", "codex", "hybrid"],
                    "description": "Initialize parameter shape. standard uses clientCapabilities/clientInfo; codex uses clientName/clientVersion; hybrid sends both."
                },
                "protocol_version": {
                    "type": "string",
                    "description": "ACP protocol version to send in initialize. Numeric strings are sent as numbers; other values are sent as strings."
                },
                "client_name": {
                    "type": "string",
                    "description": "Client name to advertise during initialize."
                },
                "client_version": {
                    "type": "string",
                    "description": "Client version to advertise during initialize."
                },
                "new_session": {
                    "type": "boolean",
                    "default": false,
                    "description": "Create a fresh ACP session before connect or prompt."
                }
            },
            "required": ["action"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ToolError::InvalidInput("missing action".to_string()))?;
        let agent_id = input.get("agent_id").and_then(serde_json::Value::as_str);
        let new_session = input
            .get("new_session")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        match action {
            "add" => {
                let config = parse_add_config(&input)?;
                self.pool
                    .add(config)
                    .map(|status| ToolResult::success(status.to_string()))
            }
            "remove" => {
                let agent_id = required_agent_id(agent_id)?;
                self.pool
                    .remove(agent_id)
                    .map(|status| ToolResult::success(status.to_string()))
            }
            "list" => Ok(ToolResult::success(self.pool.list().to_string())),
            "status" => self
                .pool
                .status(agent_id)
                .map(|status| ToolResult::success(status.to_string())),
            "connect" => {
                let agent_id = required_agent_id(agent_id)?;
                self.pool
                    .connect(agent_id, new_session)
                    .map(|status| ToolResult::success(status.to_string()))
            }
            "disconnect" => self
                .pool
                .disconnect(agent_id)
                .map(|status| ToolResult::success(status.to_string())),
            "prompt" => {
                let agent_id = required_agent_id(agent_id)?;
                let prompt = input
                    .get("prompt")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .ok_or_else(|| ToolError::InvalidInput("missing prompt".to_string()))?;
                self.pool.prompt(agent_id, prompt, new_session)
            }
            other => Err(ToolError::InvalidInput(format!(
                "unknown ACP action '{other}'"
            ))),
        }
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effects([
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::DelegateWork)
                .with_target("acp"),
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::RunProcess).with_target("acp"),
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::NetworkRequest)
                .with_target("ssh"),
        ])
    }
}

fn parse_add_config(input: &serde_json::Value) -> Result<AcpClientConfig, ToolError> {
    let id = required_string(input, "agent_id")?;
    validate_agent_id(id)?;
    let command = required_string(input, "command")?;
    let args = parse_string_array(input.get("args"))?;
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    let env = parse_env(input.get("env"))?;

    let ssh_host = input
        .get("ssh_host")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("");
    let initialize_format = input
        .get("initialize_format")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map_or_else(|| infer_initialize_format(command, &args), str::to_string);
    let protocol_version = input
        .get("protocol_version")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("1");
    let client_name = input
        .get("client_name")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("cortex");
    let client_version = input
        .get("client_version")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(env!("CARGO_PKG_VERSION"));

    Ok(AcpClientConfig {
        id: id.to_string(),
        ssh_host: ssh_host.to_string(),
        initialize_format,
        protocol_version: protocol_version.to_string(),
        client_name: client_name.to_string(),
        client_version: client_version.to_string(),
        command: command.to_string(),
        args,
        cwd: cwd.to_string(),
        env,
    })
}

fn required_agent_id(agent_id: Option<&str>) -> Result<&str, ToolError> {
    agent_id
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::InvalidInput("missing agent_id".to_string()))
}

fn required_string<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::InvalidInput(format!("missing {key}")))
}

fn parse_string_array(value: Option<&serde_json::Value>) -> Result<Vec<String>, ToolError> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let array = value
        .as_array()
        .ok_or_else(|| ToolError::InvalidInput("args must be an array of strings".to_string()))?;
    array
        .iter()
        .map(|item| {
            item.as_str().map(str::to_string).ok_or_else(|| {
                ToolError::InvalidInput("args must contain only strings".to_string())
            })
        })
        .collect()
}

fn parse_env(value: Option<&serde_json::Value>) -> Result<HashMap<String, String>, ToolError> {
    let Some(value) = value else {
        return Ok(HashMap::new());
    };
    let object = value.as_object().ok_or_else(|| {
        ToolError::InvalidInput("env must be an object with string values".to_string())
    })?;
    let mut env = HashMap::new();
    for (key, value) in object {
        if key.trim().is_empty() {
            return Err(ToolError::InvalidInput(
                "env keys must not be empty".to_string(),
            ));
        }
        validate_env_key(key)?;
        let Some(value) = value.as_str() else {
            return Err(ToolError::InvalidInput(format!(
                "env value for '{key}' must be a string"
            )));
        };
        env.insert(key.clone(), value.to_string());
    }
    Ok(env)
}

fn validate_env_key(key: &str) -> Result<(), ToolError> {
    let mut chars = key.chars();
    let Some(first) = chars.next() else {
        return Err(ToolError::InvalidInput(
            "env keys must not be empty".to_string(),
        ));
    };
    if !(first == '_' || first.is_ascii_alphabetic()) {
        return Err(ToolError::InvalidInput(format!(
            "env key '{key}' must start with an ASCII letter or underscore"
        )));
    }
    if !chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric()) {
        return Err(ToolError::InvalidInput(format!(
            "env key '{key}' must contain only ASCII letters, digits, or underscores"
        )));
    }
    Ok(())
}

fn validate_client_config(config: &AcpClientConfig) -> Result<(), ToolError> {
    validate_agent_id(&config.id)?;
    if config.command.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "ACP command must not be empty".to_string(),
        ));
    }
    if config.client_name.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "ACP client_name must not be empty".to_string(),
        ));
    }
    if config.protocol_version.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "ACP protocol_version must not be empty".to_string(),
        ));
    }
    Ok(())
}

fn validate_agent_id(id: &str) -> Result<(), ToolError> {
    if id.trim().is_empty() {
        return Err(ToolError::InvalidInput(
            "ACP agent_id must not be empty".to_string(),
        ));
    }
    if id.len() > 64 {
        return Err(ToolError::InvalidInput(
            "ACP agent_id must not exceed 64 characters".to_string(),
        ));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    {
        return Err(ToolError::InvalidInput(
            "ACP agent_id must contain only alphanumeric characters, dots, hyphens, or underscores"
                .to_string(),
        ));
    }
    Ok(())
}

fn unknown_agent(agent_id: &str, ids: &[String]) -> ToolError {
    ToolError::InvalidInput(format!(
        "unknown ACP agent '{agent_id}'; configured agents: {}",
        ids.join(", ")
    ))
}

fn remote_shell_command(
    cwd: Option<&str>,
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> String {
    let mut exec = String::new();
    for key in sorted_env_keys(env) {
        if let Some(value) = env.get(&key) {
            exec.push_str(&key);
            exec.push('=');
            exec.push_str(&shell_quote(value));
            exec.push(' ');
        }
    }
    exec.push_str("exec ");
    exec.push_str(&shell_quote(command));
    for arg in args {
        exec.push(' ');
        exec.push_str(&shell_quote(arg));
    }

    if let Some(cwd) = cwd.filter(|value| !value.trim().is_empty() && *value != ".") {
        format!("cd {} && {exec}", shell_quote(cwd))
    } else {
        exec
    }
}

fn ssh_args_for_host(ssh_host: &str) -> Result<Vec<String>, ToolError> {
    let ssh_host = ssh_host.trim();
    if ssh_host.is_empty() {
        return Err(ToolError::InvalidInput(
            "ACP ssh_host must not be empty".to_string(),
        ));
    }
    if let Some((host, port)) = split_ssh_host_port(ssh_host)? {
        Ok(vec!["-p".to_string(), port, host])
    } else {
        Ok(vec![ssh_host.to_string()])
    }
}

fn split_ssh_host_port(ssh_host: &str) -> Result<Option<(String, String)>, ToolError> {
    if let Some((host, port)) = split_bracketed_ssh_host_port(ssh_host)? {
        return Ok(Some((host, port)));
    }
    if ssh_host.matches(':').count() != 1 {
        return Ok(None);
    }
    let Some((host, port)) = ssh_host.rsplit_once(':') else {
        return Ok(None);
    };
    if host.is_empty() || port.is_empty() {
        return Ok(None);
    }
    parse_ssh_port(port).map(|port| Some((host.to_string(), port)))
}

fn split_bracketed_ssh_host_port(ssh_host: &str) -> Result<Option<(String, String)>, ToolError> {
    let Some((host, port)) = ssh_host.rsplit_once("]:") else {
        return Ok(None);
    };
    let Some(open_bracket) = host.rfind('[') else {
        return Ok(None);
    };
    let mut unwrapped = String::with_capacity(host.len().saturating_sub(2));
    unwrapped.push_str(&host[..open_bracket]);
    unwrapped.push_str(&host[open_bracket + 1..]);
    parse_ssh_port(port).map(|port| Some((unwrapped, port)))
}

fn parse_ssh_port(port: &str) -> Result<String, ToolError> {
    if port.is_empty() || !port.chars().all(|ch| ch.is_ascii_digit()) {
        return Err(ToolError::InvalidInput(format!(
            "ACP ssh_host port must be numeric: {port}"
        )));
    }
    let parsed = port.parse::<u16>().map_err(|_| {
        ToolError::InvalidInput(format!("ACP ssh_host port is out of range: {port}"))
    })?;
    if parsed == 0 {
        return Err(ToolError::InvalidInput(
            "ACP ssh_host port must be greater than zero".to_string(),
        ));
    }
    Ok(parsed.to_string())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".to_string();
    }
    let mut quoted = String::from("'");
    for ch in value.chars() {
        if ch == '\'' {
            quoted.push_str("'\\''");
        } else {
            quoted.push(ch);
        }
    }
    quoted.push('\'');
    quoted
}

fn prompt_managed_client(
    managed: &mut ManagedAcpClient,
    request_timeout: Duration,
    prompt: &str,
    new_session: bool,
) -> Result<crate::acp_client::AcpPromptResponse, ToolError> {
    ensure_connected(managed, request_timeout, new_session)?;
    let client = managed
        .client
        .as_mut()
        .ok_or_else(|| ToolError::ExecutionFailed("ACP client failed to initialize".to_string()))?;
    client
        .prompt(prompt)
        .map_err(|err| ToolError::ExecutionFailed(err.to_string()))
}

fn ensure_connected(
    managed: &mut ManagedAcpClient,
    request_timeout: Duration,
    new_session: bool,
) -> Result<String, ToolError> {
    if (new_session && session_from_initialize(&managed.config))
        || managed.client.as_mut().is_none_or(|client| {
            client
                .is_alive()
                .map_or(true, |alive| !alive || client.session_id().is_none())
        })
    {
        managed.client = Some(spawn_configured_client(&managed.config, request_timeout)?);
    }

    let cwd = session_cwd(&managed.config)?;
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
        .session_id()
        .map(str::to_string)
        .ok_or_else(|| ToolError::ExecutionFailed("ACP client has no active session".to_string()))
}

fn session_from_initialize(config: &AcpClientConfig) -> bool {
    matches!(
        effective_initialize_format(config),
        AcpInitializeFormat::Codex | AcpInitializeFormat::Hybrid
    )
}

fn spawn_configured_client(
    config: &AcpClientConfig,
    request_timeout: Duration,
) -> Result<AcpClient, ToolError> {
    let launch = launch_for_config(config, request_timeout)?;
    AcpClient::spawn(launch).map_err(|err| ToolError::ExecutionFailed(err.to_string()))
}

fn launch_for_config(
    config: &AcpClientConfig,
    request_timeout: Duration,
) -> Result<AcpLaunch, ToolError> {
    if config.ssh_host.trim().is_empty() {
        return Ok(AcpLaunch::new(config.id.clone(), config.command.clone())
            .with_args(config.args.clone())
            .with_cwd(resolve_cwd(&config.cwd)?)
            .with_env(config.env.clone())
            .with_initialize_format(effective_initialize_format(config))
            .with_protocol_version(&config.protocol_version)
            .with_client_info(&config.client_name, resolved_client_version(config))
            .with_request_timeout(request_timeout));
    }

    let remote_command = remote_shell_command(
        Some(&config.cwd),
        &config.command,
        &config.args,
        &config.env,
    );
    let mut ssh_args = ssh_args_for_host(&config.ssh_host)?;
    ssh_args.push(remote_command);
    Ok(AcpLaunch::new(config.id.clone(), "ssh")
        .with_args(ssh_args)
        .with_cwd(resolve_cwd(".")?)
        .with_initialize_format(effective_initialize_format(config))
        .with_protocol_version(&config.protocol_version)
        .with_client_info(&config.client_name, resolved_client_version(config))
        .with_request_timeout(request_timeout))
}

fn infer_initialize_format(command: &str, args: &[String]) -> String {
    if is_codex_exec_server(command, args) {
        "codex".to_string()
    } else {
        "standard".to_string()
    }
}

fn effective_initialize_format(config: &AcpClientConfig) -> AcpInitializeFormat {
    let configured = AcpInitializeFormat::from_config(&config.initialize_format);
    if configured == AcpInitializeFormat::Standard
        && is_codex_exec_server(&config.command, &config.args)
    {
        AcpInitializeFormat::Codex
    } else {
        configured
    }
}

fn is_codex_exec_server(command: &str, args: &[String]) -> bool {
    let command = command.to_ascii_lowercase();
    let exec_server =
        command.contains("exec-server") || args.iter().any(|arg| arg == "exec-server");
    command.contains("codex") && exec_server
}

fn resolved_client_version(config: &AcpClientConfig) -> &str {
    if config.client_version.trim().is_empty() {
        env!("CARGO_PKG_VERSION")
    } else {
        &config.client_version
    }
}

fn session_cwd(config: &AcpClientConfig) -> Result<PathBuf, ToolError> {
    if config.ssh_host.trim().is_empty() {
        resolve_cwd(&config.cwd)
    } else if config.cwd.trim().is_empty() {
        Ok(PathBuf::from("."))
    } else {
        Ok(PathBuf::from(&config.cwd))
    }
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

fn sorted_env_keys(env: &HashMap<String, String>) -> Vec<String> {
    let mut keys: Vec<String> = env.keys().cloned().collect();
    keys.sort();
    keys
}

fn empty_as_none(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() { None } else { Some(value) }
}
