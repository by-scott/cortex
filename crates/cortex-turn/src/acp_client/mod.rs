use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::process::{Child, Command, Stdio};

/// JSON-RPC 2.0 request for ACP Client outbound communication.
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

/// JSON-RPC 2.0 response from external ACP Agent.
#[derive(Debug, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
}

#[derive(Debug)]
pub enum AcpClientError {
    SpawnFailed(std::io::Error),
    IoError(std::io::Error),
    ProtocolError(String),
    AgentError { code: i32, message: String },
}

impl std::fmt::Display for AcpClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::SpawnFailed(e) => write!(f, "failed to spawn agent: {e}"),
            Self::IoError(e) => write!(f, "I/O error: {e}"),
            Self::ProtocolError(e) => write!(f, "protocol error: {e}"),
            Self::AgentError { code, message } => {
                write!(f, "agent error ({code}): {message}")
            }
        }
    }
}

impl std::error::Error for AcpClientError {}

/// ACP Client for communicating with external ACP Agent processes.
///
/// Spawns a child process and communicates via JSON-RPC 2.0 over stdin/stdout.
/// Implements the client side of the ACP session lifecycle:
/// `initialize` then `session/new` then `session/prompt`.
pub struct AcpClient {
    child: Child,
    writer: BufWriter<std::process::ChildStdin>,
    reader: BufReader<std::process::ChildStdout>,
    next_id: u64,
    session_id: Option<String>,
    agent_id: String,
}

impl std::fmt::Debug for AcpClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpClient")
            .field("agent_id", &self.agent_id)
            .field("session_id", &self.session_id)
            .field("next_id", &self.next_id)
            .finish_non_exhaustive()
    }
}

impl AcpClient {
    /// Spawn an external ACP Agent process.
    ///
    /// # Errors
    /// Returns `AcpClientError` if the process cannot be spawned or its I/O streams cannot be captured.
    pub fn spawn(
        command: &str,
        args: &[&str],
        agent_id: impl Into<String>,
    ) -> Result<Self, AcpClientError> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(AcpClientError::SpawnFailed)?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| AcpClientError::ProtocolError("failed to capture child stdin".into()))?;
        let stdout = child.stdout.take().ok_or_else(|| {
            AcpClientError::ProtocolError("failed to capture child stdout".into())
        })?;

        Ok(Self {
            child,
            writer: BufWriter::new(stdin),
            reader: BufReader::new(stdout),
            next_id: 1,
            session_id: None,
            agent_id: agent_id.into(),
        })
    }

    /// Send a JSON-RPC request and read the response.
    ///
    /// # Errors
    /// Returns `AcpClientError` if the request cannot be sent, the response is invalid, or the agent returns an error.
    pub fn send_request(
        &mut self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<JsonRpcResponse, AcpClientError> {
        let id = serde_json::Value::Number(self.next_id.into());
        self.next_id += 1;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: method.into(),
            id,
            params,
        };

        let json = serde_json::to_string(&request)
            .map_err(|e| AcpClientError::ProtocolError(e.to_string()))?;

        writeln!(self.writer, "{json}").map_err(AcpClientError::IoError)?;
        self.writer.flush().map_err(AcpClientError::IoError)?;

        let mut line = String::new();
        self.reader
            .read_line(&mut line)
            .map_err(AcpClientError::IoError)?;

        if line.trim().is_empty() {
            return Err(AcpClientError::ProtocolError(
                "empty response from agent".into(),
            ));
        }

        let response: JsonRpcResponse = serde_json::from_str(line.trim())
            .map_err(|e| AcpClientError::ProtocolError(format!("invalid JSON response: {e}")))?;

        if response.jsonrpc != "2.0" {
            return Err(AcpClientError::ProtocolError(
                "invalid JSON-RPC version".into(),
            ));
        }

        if let Some(err) = &response.error {
            return Err(AcpClientError::AgentError {
                code: err.code,
                message: err.message.clone(),
            });
        }

        Ok(response)
    }

    /// Send session/initialize and get agent capabilities.
    ///
    /// # Errors
    /// Returns `AcpClientError` if the initialize request fails or returns no result.
    pub fn initialize(&mut self) -> Result<serde_json::Value, AcpClientError> {
        let resp = self.send_request("session/initialize", None)?;
        resp.result
            .ok_or_else(|| AcpClientError::ProtocolError("no result in initialize response".into()))
    }

    /// Send session/new and get a `session_id`.
    ///
    /// # Errors
    /// Returns `AcpClientError` if the request fails or the response is missing a session ID.
    pub fn new_session(&mut self) -> Result<String, AcpClientError> {
        let resp = self.send_request("session/new", None)?;
        let result = resp.result.ok_or_else(|| {
            AcpClientError::ProtocolError("no result in new_session response".into())
        })?;
        let session_id = result
            .get("session_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpClientError::ProtocolError("missing session_id in response".into()))?
            .to_string();
        self.session_id = Some(session_id.clone());
        Ok(session_id)
    }

    /// Send session/prompt and collect the response text.
    ///
    /// # Errors
    /// Returns `AcpClientError` if the request fails or the response is missing.
    pub fn prompt(&mut self, text: &str) -> Result<String, AcpClientError> {
        let params = serde_json::json!({
            "prompt": text,
            "session_id": self.session_id,
        });
        let resp = self.send_request("session/prompt", Some(params))?;
        let result = resp
            .result
            .ok_or_else(|| AcpClientError::ProtocolError("no result in prompt response".into()))?;

        // Extract response text from result
        Ok(result
            .get("response")
            .and_then(|v| v.as_str())
            .or_else(|| result.as_str())
            .map_or_else(|| result.to_string(), ToString::to_string))
    }

    /// Get the agent ID.
    #[must_use]
    pub fn agent_id(&self) -> &str {
        &self.agent_id
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}

impl Drop for AcpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Build a JSON-RPC request string (for testing/external use).
///
/// Returns an empty string if serialization fails (should never happen with valid input).
#[must_use]
pub fn build_request(method: &str, id: u64, params: Option<serde_json::Value>) -> String {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".into(),
        method: method.into(),
        id: serde_json::Value::Number(id.into()),
        params,
    };
    serde_json::to_string(&request).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_format() {
        let req = build_request("session/initialize", 1, None);
        let parsed: serde_json::Value = serde_json::from_str(&req).unwrap();
        assert_eq!(parsed["jsonrpc"], "2.0");
        assert_eq!(parsed["method"], "session/initialize");
        assert_eq!(parsed["id"], 1);
    }

    #[test]
    fn build_request_with_params() {
        let params = serde_json::json!({"prompt": "hello"});
        let req = build_request("session/prompt", 2, Some(params));
        let parsed: serde_json::Value = serde_json::from_str(&req).unwrap();
        assert_eq!(parsed["params"]["prompt"], "hello");
    }

    #[test]
    fn spawn_invalid_command_fails() {
        let result = AcpClient::spawn("__nonexistent_command_xyz__", &[], "test");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            AcpClientError::SpawnFailed(_)
        ));
    }

    #[test]
    fn json_rpc_response_deserialize() {
        let json = r#"{"jsonrpc":"2.0","id":1,"result":{"session_id":"abc"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_none());
        assert_eq!(
            resp.result
                .unwrap()
                .get("session_id")
                .unwrap()
                .as_str()
                .unwrap(),
            "abc"
        );
    }

    #[test]
    fn json_rpc_error_response_deserialize() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"not found"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json).unwrap();
        assert!(resp.result.is_none());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -32601);
        assert_eq!(err.message, "not found");
    }

    #[test]
    fn acp_client_error_display() {
        let err = AcpClientError::ProtocolError("test".into());
        assert_eq!(format!("{err}"), "protocol error: test");

        let err = AcpClientError::AgentError {
            code: -32601,
            message: "not found".into(),
        };
        assert_eq!(format!("{err}"), "agent error (-32601): not found");
    }
}
