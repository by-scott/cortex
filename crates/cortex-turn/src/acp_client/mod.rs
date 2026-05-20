use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const ACP_PROTOCOL_VERSION: u32 = 1;
const JSONRPC_VERSION: &str = "2.0";
const DEFAULT_CLIENT_NAME: &str = "cortex";

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: &'static str,
    method: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcEnvelope {
    #[serde(default)]
    jsonrpc: String,
    #[serde(default)]
    id: serde_json::Value,
    #[serde(default)]
    method: String,
    #[serde(default)]
    params: serde_json::Value,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug)]
pub enum AcpClientError {
    InvalidLaunch(String),
    SpawnFailed(std::io::Error),
    IoError(std::io::Error),
    Timeout { method: String, timeout: Duration },
    ProtocolError(String),
    AgentError { code: i32, message: String },
}

impl std::fmt::Display for AcpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidLaunch(message) => write!(f, "invalid ACP launch: {message}"),
            Self::SpawnFailed(err) => write!(f, "failed to spawn ACP agent: {err}"),
            Self::IoError(err) => write!(f, "ACP I/O error: {err}"),
            Self::Timeout { method, timeout } => {
                write!(
                    f,
                    "ACP request '{method}' timed out after {}s",
                    timeout.as_secs()
                )
            }
            Self::ProtocolError(message) => write!(f, "ACP protocol error: {message}"),
            Self::AgentError { code, message } => {
                write!(f, "ACP agent error ({code}): {message}")
            }
        }
    }
}

impl std::error::Error for AcpClientError {}

#[derive(Debug, Clone)]
pub struct AcpLaunch {
    pub agent_id: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: HashMap<String, String>,
    pub initialize_format: AcpInitializeFormat,
    pub protocol_version: String,
    pub client_name: String,
    pub client_version: String,
    pub request_timeout: Duration,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcpInitializeFormat {
    Standard,
    Codex,
    Hybrid,
}

impl AcpInitializeFormat {
    #[must_use]
    pub fn from_config(value: &str) -> Self {
        match value.trim().to_ascii_lowercase().as_str() {
            "codex" | "legacy" => Self::Codex,
            "hybrid" | "both" => Self::Hybrid,
            _ => Self::Standard,
        }
    }
}

impl AcpLaunch {
    #[must_use]
    pub fn new(agent_id: impl Into<String>, command: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            command: command.into(),
            args: Vec::new(),
            cwd: None,
            env: HashMap::new(),
            initialize_format: AcpInitializeFormat::Standard,
            protocol_version: ACP_PROTOCOL_VERSION.to_string(),
            client_name: DEFAULT_CLIENT_NAME.to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            request_timeout: Duration::from_mins(2),
        }
    }

    #[must_use]
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }

    #[must_use]
    pub fn with_env(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    #[must_use]
    pub const fn with_initialize_format(mut self, format: AcpInitializeFormat) -> Self {
        self.initialize_format = format;
        self
    }

    #[must_use]
    pub fn with_protocol_version(mut self, version: impl Into<String>) -> Self {
        let version = version.into();
        if !version.trim().is_empty() {
            self.protocol_version = version;
        }
        self
    }

    #[must_use]
    pub fn with_client_info(mut self, name: impl Into<String>, version: impl Into<String>) -> Self {
        let name = name.into();
        if !name.trim().is_empty() {
            self.client_name = name;
        }
        let version = version.into();
        if !version.trim().is_empty() {
            self.client_version = version;
        }
        self
    }

    #[must_use]
    pub const fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout;
        self
    }

    fn validate(&self) -> Result<(), AcpClientError> {
        if self.agent_id.trim().is_empty() {
            return Err(AcpClientError::InvalidLaunch(
                "agent_id must not be empty".to_string(),
            ));
        }
        if self.command.trim().is_empty() {
            return Err(AcpClientError::InvalidLaunch(
                "command must not be empty".to_string(),
            ));
        }
        if self.request_timeout.is_zero() {
            return Err(AcpClientError::InvalidLaunch(
                "request_timeout must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AcpPromptResponse {
    pub session_id: String,
    pub text: String,
    pub stop_reason: String,
    pub raw_result: serde_json::Value,
}

/// Stdio JSON-RPC client for external ACP agent processes.
///
/// The client owns a single child process, performs the ACP initialize and
/// session lifecycle, validates JSON-RPC response ids, handles interleaved
/// notifications, and applies a per-request timeout so agent subprocesses
/// cannot block the turn indefinitely.
pub struct AcpClient {
    child: Child,
    writer: BufWriter<std::process::ChildStdin>,
    lines: Receiver<String>,
    reader_thread: Option<JoinHandle<()>>,
    next_id: u64,
    session_id: Option<String>,
    initialized: bool,
    session_from_initialize: bool,
    initialize_format: AcpInitializeFormat,
    agent_id: String,
    protocol_version: String,
    client_name: String,
    client_version: String,
    request_timeout: Duration,
}

impl std::fmt::Debug for AcpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpClient")
            .field("agent_id", &self.agent_id)
            .field("session_id", &self.session_id)
            .field("initialized", &self.initialized)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl AcpClient {
    /// Spawn an external ACP agent process.
    ///
    /// # Errors
    /// Returns an error if launch settings are invalid, the process cannot be
    /// spawned, or stdio cannot be captured.
    pub fn spawn(launch: AcpLaunch) -> Result<Self, AcpClientError> {
        launch.validate()?;
        let default_cwd = std::env::temp_dir();
        let cwd = launch.cwd.as_deref().unwrap_or(default_cwd.as_path());
        let mut command = crate::process::command_with_policy(&launch.command, cwd);
        command
            .args(&launch.args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .envs(&launch.env);

        let mut child = command.spawn().map_err(AcpClientError::SpawnFailed)?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpClientError::ProtocolError("child stdin unavailable".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| AcpClientError::ProtocolError("child stdout unavailable".to_string()))?;
        let (tx, rx) = mpsc::channel();
        let reader_thread = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            child,
            writer: BufWriter::new(stdin),
            lines: rx,
            reader_thread: Some(reader_thread),
            next_id: 1,
            session_id: None,
            initialized: false,
            session_from_initialize: false,
            initialize_format: launch.initialize_format,
            agent_id: launch.agent_id,
            protocol_version: launch.protocol_version,
            client_name: launch.client_name,
            client_version: launch.client_version,
            request_timeout: launch.request_timeout,
        })
    }

    /// Return the configured agent id.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Return the currently active ACP session id, if one has been created.
    #[must_use]
    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    /// Check whether the child process is still running.
    ///
    /// # Errors
    /// Returns an I/O error if the child process status cannot be queried.
    pub fn is_alive(&mut self) -> Result<bool, AcpClientError> {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .map_err(AcpClientError::IoError)
    }

    /// Perform the ACP `initialize` handshake.
    ///
    /// # Errors
    /// Returns an error if the agent rejects the request, times out, or returns
    /// malformed JSON-RPC.
    pub fn initialize(&mut self) -> Result<serde_json::Value, AcpClientError> {
        let result = self.send_request("initialize", Some(self.initialize_params()), None)?;
        if let Some(session_id) = result
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
        {
            self.session_id = Some(session_id.to_string());
            self.session_from_initialize = true;
        }
        self.initialized = true;
        Ok(result)
    }

    /// Create a new ACP session rooted at `cwd`.
    ///
    /// # Errors
    /// Returns an error if initialization or `session/new` fails, or if the
    /// response does not contain `sessionId`.
    pub fn new_session(&mut self, cwd: &Path) -> Result<String, AcpClientError> {
        self.ensure_initialized()?;
        if self.session_from_initialize {
            return self.session_id.clone().ok_or_else(|| {
                AcpClientError::ProtocolError(
                    "ACP initialize response did not provide an active sessionId".to_string(),
                )
            });
        }
        let result = self.send_request(
            "session/new",
            Some(serde_json::json!({
                "cwd": cwd.to_string_lossy(),
                "mcpServers": [],
            })),
            None,
        )?;
        let session_id = result
            .get("sessionId")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| {
                AcpClientError::ProtocolError("session/new result missing sessionId".to_string())
            })?
            .to_string();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Send a prompt to the active ACP session and collect streamed text.
    ///
    /// # Errors
    /// Returns an error if no session exists, the request fails, or the agent
    /// returns malformed protocol data.
    pub fn prompt(&mut self, text: &str) -> Result<AcpPromptResponse, AcpClientError> {
        let session_id = self
            .session_id
            .clone()
            .ok_or_else(|| AcpClientError::ProtocolError("no active ACP session".to_string()))?;
        let mut transcript = String::new();
        let result = self.send_request(
            "session/prompt",
            Some(serde_json::json!({
                "sessionId": session_id,
                "prompt": [
                    {
                        "type": "text",
                        "text": text,
                    }
                ],
            })),
            Some(&mut transcript),
        )?;
        let stop_reason = result
            .get("stopReason")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        Ok(AcpPromptResponse {
            session_id,
            text: transcript,
            stop_reason,
            raw_result: result,
        })
    }

    fn ensure_initialized(&mut self) -> Result<(), AcpClientError> {
        if self.initialized {
            Ok(())
        } else {
            self.initialize().map(|_| ())
        }
    }

    fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
        mut transcript: Option<&mut String>,
    ) -> Result<serde_json::Value, AcpClientError> {
        let id = serde_json::Value::Number(self.next_id.into());
        self.next_id += 1;
        let request = JsonRpcRequest {
            jsonrpc: JSONRPC_VERSION,
            method: method.to_string(),
            id: id.clone(),
            params,
        };
        self.write_value(&request)?;
        let deadline = Instant::now() + self.request_timeout;

        loop {
            let line = self.read_line_until(method, deadline)?;
            let envelope: JsonRpcEnvelope = serde_json::from_str(&line).map_err(|err| {
                AcpClientError::ProtocolError(format!("invalid JSON-RPC message: {err}"))
            })?;
            if !envelope.jsonrpc.is_empty() && envelope.jsonrpc != JSONRPC_VERSION {
                return Err(AcpClientError::ProtocolError(
                    "invalid JSON-RPC version".to_string(),
                ));
            }
            if !envelope.method.is_empty() {
                self.handle_inbound_method(&envelope)?;
                if let Some(buffer) = transcript.as_deref_mut() {
                    append_notification_text(&envelope.params, buffer);
                }
                continue;
            }
            if envelope.id != id {
                return Err(AcpClientError::ProtocolError(format!(
                    "response id mismatch: expected {id}, got {}",
                    envelope.id
                )));
            }
            if let Some(err) = envelope.error {
                return Err(AcpClientError::AgentError {
                    code: err.code,
                    message: err.message,
                });
            }
            return envelope.result.ok_or_else(|| {
                AcpClientError::ProtocolError(format!("{method} response missing result"))
            });
        }
    }

    fn handle_inbound_method(&mut self, envelope: &JsonRpcEnvelope) -> Result<(), AcpClientError> {
        if envelope.id.is_null() {
            return Ok(());
        }
        let response = if envelope.method == "session/request_permission" {
            serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": envelope.id,
                "result": permission_response(&envelope.params),
            })
        } else {
            serde_json::json!({
                "jsonrpc": JSONRPC_VERSION,
                "id": envelope.id,
                "error": {
                    "code": -32601,
                    "message": format!("Cortex ACP client does not implement '{}'", envelope.method),
                },
            })
        };
        self.write_json_value(&response)
    }

    fn read_line_until(&self, method: &str, deadline: Instant) -> Result<String, AcpClientError> {
        loop {
            let now = Instant::now();
            if now >= deadline {
                return Err(AcpClientError::Timeout {
                    method: method.to_string(),
                    timeout: self.request_timeout,
                });
            }
            match self.lines.recv_timeout(deadline - now) {
                Ok(line) if line.trim().is_empty() => {}
                Ok(line) => return Ok(line),
                Err(RecvTimeoutError::Timeout) => {
                    return Err(AcpClientError::Timeout {
                        method: method.to_string(),
                        timeout: self.request_timeout,
                    });
                }
                Err(RecvTimeoutError::Disconnected) => {
                    return Err(AcpClientError::ProtocolError(
                        "ACP agent stdout closed".to_string(),
                    ));
                }
            }
        }
    }

    fn write_value(&mut self, request: &JsonRpcRequest) -> Result<(), AcpClientError> {
        let json = serde_json::to_string(request)
            .map_err(|err| AcpClientError::ProtocolError(err.to_string()))?;
        writeln!(self.writer, "{json}").map_err(AcpClientError::IoError)?;
        self.writer.flush().map_err(AcpClientError::IoError)
    }

    fn write_json_value(&mut self, value: &serde_json::Value) -> Result<(), AcpClientError> {
        let json = serde_json::to_string(value)
            .map_err(|err| AcpClientError::ProtocolError(err.to_string()))?;
        writeln!(self.writer, "{json}").map_err(AcpClientError::IoError)?;
        self.writer.flush().map_err(AcpClientError::IoError)
    }

    fn initialize_params(&self) -> serde_json::Value {
        let protocol_version = protocol_version_value(&self.protocol_version);
        let standard = serde_json::json!({
            "protocolVersion": protocol_version,
            "clientCapabilities": {},
            "clientInfo": {
                "name": self.client_name,
                "version": self.client_version,
            },
        });
        let codex = serde_json::json!({
            "protocolVersion": protocol_version_value(&self.protocol_version),
            "capabilities": {},
            "clientName": self.client_name,
            "clientVersion": self.client_version,
        });
        match self.initialize_format {
            AcpInitializeFormat::Standard => standard,
            AcpInitializeFormat::Codex => codex,
            AcpInitializeFormat::Hybrid => {
                let mut value = standard;
                if let Some(object) = value.as_object_mut() {
                    object.insert("capabilities".to_string(), serde_json::json!({}));
                    object.insert(
                        "clientName".to_string(),
                        serde_json::Value::String(self.client_name.clone()),
                    );
                    object.insert(
                        "clientVersion".to_string(),
                        serde_json::Value::String(self.client_version.clone()),
                    );
                }
                value
            }
        }
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader_thread) = self.reader_thread.take() {
            let _ = reader_thread.join();
        }
    }
}

fn append_notification_text(params: &serde_json::Value, transcript: &mut String) {
    if let Some(update) = params.get("update") {
        if append_acp_session_update(update, transcript) {
            return;
        }
        append_text_fragments(update, transcript);
    } else {
        append_text_fragments(params, transcript);
    }
}

fn permission_response(params: &serde_json::Value) -> serde_json::Value {
    selected_permission_option(params).map_or_else(
        || serde_json::json!({ "outcome": { "outcome": "cancelled" } }),
        |option_id| {
            serde_json::json!({
                "outcome": {
                    "outcome": "selected",
                    "optionId": option_id,
                },
            })
        },
    )
}

fn selected_permission_option(params: &serde_json::Value) -> Option<String> {
    let options = params.get("options")?.as_array()?;
    find_permission_option(options, "allow_once")
        .or_else(|| find_permission_option(options, "allow_always"))
        .or_else(|| find_named_permission_option(options))
        .or_else(|| first_permission_option(options))
}

fn find_permission_option(options: &[serde_json::Value], kind: &str) -> Option<String> {
    options.iter().find_map(|option| {
        (option.get("kind").and_then(serde_json::Value::as_str) == Some(kind))
            .then(|| permission_option_id(option))
            .flatten()
    })
}

fn find_named_permission_option(options: &[serde_json::Value]) -> Option<String> {
    options.iter().find_map(|option| {
        let id = permission_option_id(option)?;
        matches!(id.as_str(), "approved" | "approve" | "yes").then_some(id)
    })
}

fn first_permission_option(options: &[serde_json::Value]) -> Option<String> {
    options.iter().find_map(permission_option_id)
}

fn permission_option_id(option: &serde_json::Value) -> Option<String> {
    option
        .get("optionId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn append_acp_session_update(update: &serde_json::Value, transcript: &mut String) -> bool {
    let Some(kind) = update
        .get("sessionUpdate")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    if kind != "agent_message_chunk" {
        return true;
    }
    if let Some(content) = update.get("content") {
        append_content_block_text(content, transcript);
    }
    true
}

fn append_content_block_text(content: &serde_json::Value, transcript: &mut String) {
    match content {
        serde_json::Value::String(text) => transcript.push_str(text),
        serde_json::Value::Object(map) => {
            if let Some(text) = map.get("text").and_then(serde_json::Value::as_str) {
                transcript.push_str(text);
                return;
            }
            if let Some(text_block) = map.get("Text").or_else(|| map.get("text")) {
                append_content_block_text(text_block, transcript);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                append_content_block_text(item, transcript);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn append_text_fragments(value: &serde_json::Value, transcript: &mut String) {
    match value {
        serde_json::Value::String(text) => transcript.push_str(text),
        serde_json::Value::Array(items) => {
            for item in items {
                append_text_fragments(item, transcript);
            }
        }
        serde_json::Value::Object(map) => {
            for key in ["text", "chunk", "delta"] {
                if let Some(text) = map.get(key).and_then(serde_json::Value::as_str) {
                    transcript.push_str(text);
                    return;
                }
            }
            if let Some(content) = map.get("content") {
                append_text_fragments(content, transcript);
            }
        }
        serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {}
    }
}

fn protocol_version_value(version: &str) -> serde_json::Value {
    let value = version.trim();
    if value.is_empty() {
        return serde_json::Value::Number(ACP_PROTOCOL_VERSION.into());
    }
    value.parse::<u64>().map_or_else(
        |_| serde_json::Value::String(value.to_string()),
        |number| serde_json::Value::Number(number.into()),
    )
}
