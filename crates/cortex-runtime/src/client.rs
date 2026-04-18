use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::rpc::RpcResponse;

// ── Stream Event Type ───────────────────────────────────────

/// Events emitted during a streaming turn execution.
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// Partial text content from the LLM.
    Text { content: String },
    /// Tool execution progress (started / completed / error).
    Tool { tool_name: String, status: String },
    /// Trace event from the turn tracer.
    Trace { category: String, message: String },
    /// Final event indicating turn completion.
    Done {
        session_id: String,
        response: String,
    },
    /// Error during turn execution.
    Error { message: String },
}

// ── Error Type ───────────────────────────────────────────────

/// Errors that can occur when communicating with a daemon.
#[derive(Debug)]
pub enum ClientError {
    /// Could not connect to the daemon (socket missing, connection refused, etc.).
    ConnectionFailed(String),
    /// The daemon returned a JSON-RPC error.
    RpcError { code: i32, message: String },
    /// Transport-level I/O error.
    Io(String),
    /// Response could not be parsed.
    ParseError(String),
}

impl std::fmt::Display for ClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "connection failed: {msg}"),
            Self::RpcError { code, message } => write!(f, "RPC error ({code}): {message}"),
            Self::Io(msg) => write!(f, "I/O error: {msg}"),
            Self::ParseError(msg) => write!(f, "parse error: {msg}"),
        }
    }
}

impl std::error::Error for ClientError {}

// ── Transport ────────────────────────────────────────────────

enum ClientTransport {
    Socket(PathBuf),
    Http(String),
}

// ── DaemonClient ─────────────────────────────────────────────

/// Client for communicating with a Cortex daemon via JSON-RPC 2.0.
///
/// Supports two transports:
/// - **Unix Socket**: for local connections (preferred)
/// - **HTTP**: for remote connections
pub struct DaemonClient {
    transport: ClientTransport,
    next_id: std::sync::atomic::AtomicU64,
}

impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let transport_name = match &self.transport {
            ClientTransport::Socket(p) => format!("Socket({})", p.display()),
            ClientTransport::Http(url) => format!("Http({url})"),
        };
        let id = self.next_id.load(std::sync::atomic::Ordering::Relaxed);
        f.debug_struct("DaemonClient")
            .field("transport", &transport_name)
            .field("next_id", &id)
            .finish()
    }
}

impl DaemonClient {
    /// Connect to a daemon via Unix Socket.
    ///
    /// Verifies connectivity by sending a `daemon/status` request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ConnectionFailed`] if the socket does not exist
    /// or the daemon is not responding.
    pub fn connect_socket(path: &Path) -> Result<Self, ClientError> {
        // Verify the socket is connectable
        let stream = UnixStream::connect(path)
            .map_err(|e| ClientError::ConnectionFailed(format!("{}: {e}", path.display())))?;
        drop(stream);

        let client = Self {
            transport: ClientTransport::Socket(path.to_path_buf()),
            next_id: std::sync::atomic::AtomicU64::new(1),
        };

        // Verify daemon is responding
        client
            .status()
            .map_err(|e| ClientError::ConnectionFailed(format!("daemon not responding: {e}")))?;

        Ok(client)
    }

    /// Connect to a daemon via HTTP.
    ///
    /// Verifies connectivity by sending a `daemon/status` request.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError::ConnectionFailed`] if the daemon is not reachable.
    pub fn connect_http(addr: &str) -> Result<Self, ClientError> {
        let base_url = if addr.starts_with("http://") || addr.starts_with("https://") {
            addr.to_string()
        } else {
            format!("http://{addr}")
        };

        let client = Self {
            transport: ClientTransport::Http(base_url),
            next_id: std::sync::atomic::AtomicU64::new(1),
        };

        // Verify daemon is responding
        client.status().map_err(|e| {
            ClientError::ConnectionFailed(format!("daemon not responding at {addr}: {e}"))
        })?;

        Ok(client)
    }

    /// Check if a daemon is running at the given home directory.
    ///
    /// Attempts to connect to `{home}/cortex.sock` and send a status request.
    #[must_use]
    pub fn is_daemon_running(home: &Path) -> bool {
        let socket_path = home.join("data/cortex.sock");
        if !socket_path.exists() {
            return false;
        }
        // Try to actually connect -- a stale socket file will fail here
        Self::connect_socket(&socket_path).is_ok()
    }

    fn next_request_id(&self) -> u64 {
        self.next_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    }

    /// Send a raw JSON-RPC request to the daemon.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or protocol errors.
    pub fn send_rpc(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let id = self.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "id": id,
            "params": params,
        });

        let response_json = match &self.transport {
            ClientTransport::Socket(path) => Self::send_via_socket(path, &request)?,
            ClientTransport::Http(base_url) => Self::send_via_http(base_url, &request)?,
        };

        let response: RpcResponse = serde_json::from_value(response_json)
            .map_err(|e| ClientError::ParseError(format!("invalid RPC response: {e}")))?;

        if let Some(err) = response.error {
            return Err(ClientError::RpcError {
                code: err.code,
                message: err.message,
            });
        }

        Ok(response.result.unwrap_or(serde_json::Value::Null))
    }

    fn send_via_socket(
        path: &Path,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let mut stream = UnixStream::connect(path)
            .map_err(|e| ClientError::Io(format!("socket connect: {e}")))?;

        stream
            .set_read_timeout(Some(Duration::from_mins(5)))
            .map_err(|e| ClientError::Io(format!("set timeout: {e}")))?;

        let mut line = serde_json::to_string(request)
            .map_err(|e| ClientError::ParseError(format!("serialize request: {e}")))?;
        line.push('\n');

        stream
            .write_all(line.as_bytes())
            .map_err(|e| ClientError::Io(format!("write: {e}")))?;
        stream
            .flush()
            .map_err(|e| ClientError::Io(format!("flush: {e}")))?;

        let mut reader = BufReader::with_capacity(64 * 1024, stream);
        let mut response_line = String::new();
        reader
            .read_line(&mut response_line)
            .map_err(|e| ClientError::Io(format!("read: {e}")))?;

        serde_json::from_str::<serde_json::Value>(&response_line)
            .map_err(|e| ClientError::ParseError(format!("parse response: {e}")))
    }

    fn send_via_http(
        base_url: &str,
        request: &serde_json::Value,
    ) -> Result<serde_json::Value, ClientError> {
        let body = serde_json::to_string(request)
            .map_err(|e| ClientError::ParseError(format!("serialize request: {e}")))?;

        // Parse host:port from base_url (strip http:// prefix)
        let addr = base_url
            .strip_prefix("http://")
            .or_else(|| base_url.strip_prefix("https://"))
            .unwrap_or(base_url);

        let mut stream = std::net::TcpStream::connect(addr)
            .map_err(|e| ClientError::Io(format!("HTTP connect to {addr}: {e}")))?;

        stream
            .set_read_timeout(Some(Duration::from_mins(5)))
            .map_err(|e| ClientError::Io(format!("set timeout: {e}")))?;

        let http_request = format!(
            "POST /api/rpc HTTP/1.1\r\n\
             Host: {addr}\r\n\
             Content-Type: application/json\r\n\
             Content-Length: {len}\r\n\
             Connection: close\r\n\
             \r\n\
             {body}",
            len = body.len(),
        );

        stream
            .write_all(http_request.as_bytes())
            .map_err(|e| ClientError::Io(format!("HTTP write: {e}")))?;
        stream
            .flush()
            .map_err(|e| ClientError::Io(format!("HTTP flush: {e}")))?;

        // Read full response
        let mut response_bytes = Vec::new();
        std::io::Read::read_to_end(&mut stream, &mut response_bytes)
            .map_err(|e| ClientError::Io(format!("HTTP read: {e}")))?;

        let response_str = String::from_utf8_lossy(&response_bytes);

        // Extract body after \r\n\r\n
        let body_start = response_str.find("\r\n\r\n").map_or(0, |pos| pos + 4);
        let response_body = &response_str[body_start..];

        serde_json::from_str::<serde_json::Value>(response_body)
            .map_err(|e| ClientError::ParseError(format!("parse HTTP response: {e}")))
    }

    // ── High-Level Methods ───────────────────────────────────

    /// Send a prompt to a session and return the response text.
    ///
    /// When `on_event` is `Some`, the Socket transport streams events
    /// line-by-line and invokes the callback for each [`StreamEvent`].
    /// The HTTP transport ignores the callback (synchronous RPC).
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn prompt(
        &self,
        session_id: &str,
        text: &str,
        on_event: Option<&mut dyn FnMut(&StreamEvent)>,
    ) -> Result<String, ClientError> {
        let params = serde_json::json!({
            "session_id": session_id,
            "prompt": text,
        });

        match (&self.transport, on_event) {
            (ClientTransport::Socket(path), Some(cb)) => {
                self.prompt_streaming_socket(path, &params, cb)
            }
            (ClientTransport::Socket(path), None) => {
                let mut ignore = |_event: &StreamEvent| {};
                self.prompt_streaming_socket(path, &params, &mut ignore)
            }
            (ClientTransport::Http(_), _) => {
                let result = self.send_rpc("session/prompt", &params)?;
                Ok(result
                    .get("response")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string())
            }
        }
    }

    /// Socket streaming: send the RPC request and read back event lines
    /// until a `done` or `error` event arrives.
    fn prompt_streaming_socket(
        &self,
        path: &Path,
        params: &serde_json::Value,
        on_event: &mut dyn FnMut(&StreamEvent),
    ) -> Result<String, ClientError> {
        let id = self.next_request_id();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/prompt",
            "id": id,
            "params": params,
        });

        let mut stream = UnixStream::connect(path)
            .map_err(|e| ClientError::Io(format!("socket connect: {e}")))?;
        stream
            .set_read_timeout(Some(Duration::from_mins(5)))
            .map_err(|e| ClientError::Io(format!("set timeout: {e}")))?;

        let mut line = serde_json::to_string(&request)
            .map_err(|e| ClientError::ParseError(format!("serialize request: {e}")))?;
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .map_err(|e| ClientError::Io(format!("write: {e}")))?;
        stream
            .flush()
            .map_err(|e| ClientError::Io(format!("flush: {e}")))?;

        let mut reader = BufReader::with_capacity(64 * 1024, stream);
        let mut response_line = String::new();

        loop {
            response_line.clear();
            let n = reader
                .read_line(&mut response_line)
                .map_err(|e| ClientError::Io(format!("read: {e}")))?;
            if n == 0 {
                return Err(ClientError::Io(
                    "connection closed before done event".into(),
                ));
            }
            let trimmed = response_line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a stream event first
            if let Some(event) = Self::parse_stream_event(trimmed) {
                match &event {
                    StreamEvent::Done { response, .. } => {
                        let resp = response.clone();
                        on_event(&event);
                        return Ok(resp);
                    }
                    StreamEvent::Error { message } => {
                        let msg = message.clone();
                        on_event(&event);
                        return Err(ClientError::RpcError {
                            code: -1,
                            message: msg,
                        });
                    }
                    _ => {
                        on_event(&event);
                    }
                }
                continue;
            }

            // Fallback: parse as a standard JSON-RPC response (non-streaming)
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(trimmed) {
                let rpc: RpcResponse = serde_json::from_value(val)
                    .map_err(|e| ClientError::ParseError(format!("invalid RPC response: {e}")))?;
                if let Some(err) = rpc.error {
                    return Err(ClientError::RpcError {
                        code: err.code,
                        message: err.message,
                    });
                }
                let result = rpc.result.unwrap_or(serde_json::Value::Null);
                return Ok(result
                    .get("response")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string());
            }
        }
    }

    /// Parse a JSON line as a stream event object (`{"event":"...","data":{...}}`).
    fn parse_stream_event(line: &str) -> Option<StreamEvent> {
        let val: serde_json::Value = serde_json::from_str(line).ok()?;
        let event = val.get("event")?.as_str()?;
        let data = val.get("data")?;
        match event {
            "text" => Some(StreamEvent::Text {
                content: data
                    .get("content")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            "tool" => Some(StreamEvent::Tool {
                tool_name: data
                    .get("tool_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                status: data
                    .get("status")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            "trace" => Some(StreamEvent::Trace {
                category: data
                    .get("category")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                message: data
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            "done" => Some(StreamEvent::Done {
                session_id: data
                    .get("session_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                response: data
                    .get("response")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            "error" => Some(StreamEvent::Error {
                message: data
                    .get("message")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            }),
            _ => None,
        }
    }

    /// Create a new session and return its ID.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn new_session(&self) -> Result<String, ClientError> {
        let params = serde_json::json!({});
        let result = self.send_rpc("session/new", &params)?;
        Ok(result
            .get("session_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string())
    }

    /// List all sessions.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn list_sessions(&self) -> Result<serde_json::Value, ClientError> {
        let params = serde_json::json!({});
        self.send_rpc("session/list", &params)
    }

    /// End a session.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn end_session(&self, session_id: &str) -> Result<(), ClientError> {
        let params = serde_json::json!({ "session_id": session_id });
        self.send_rpc("session/end", &params)?;
        Ok(())
    }

    /// Dispatch a slash command and return the output.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn dispatch_command(&self, command: &str) -> Result<String, ClientError> {
        let params = serde_json::json!({ "command": command });
        let result = self.send_rpc("command/dispatch", &params)?;
        Ok(result
            .get("output")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string())
    }

    /// Get daemon status.
    ///
    /// # Errors
    ///
    /// Returns [`ClientError`] on transport or daemon errors.
    pub fn status(&self) -> Result<serde_json::Value, ClientError> {
        let params = serde_json::json!({});
        self.send_rpc("daemon/status", &params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_error_display() {
        let err = ClientError::ConnectionFailed("socket not found".into());
        assert!(err.to_string().contains("connection failed"));

        let err = ClientError::RpcError {
            code: -32_601,
            message: "Method not found".into(),
        };
        assert!(err.to_string().contains("-32601"));
        assert!(err.to_string().contains("Method not found"));

        let err = ClientError::Io("broken pipe".into());
        assert!(err.to_string().contains("broken pipe"));

        let err = ClientError::ParseError("invalid json".into());
        assert!(err.to_string().contains("invalid json"));
    }

    #[test]
    fn is_daemon_running_false_when_no_socket() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(!DaemonClient::is_daemon_running(tmp.path()));
    }

    #[test]
    fn is_daemon_running_false_when_stale_socket_file() {
        let tmp = tempfile::tempdir().unwrap();
        // Create a regular file pretending to be a socket
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        std::fs::write(tmp.path().join("data/cortex.sock"), "").unwrap();
        assert!(!DaemonClient::is_daemon_running(tmp.path()));
    }

    #[test]
    fn connect_socket_fails_on_nonexistent_path() {
        let result = DaemonClient::connect_socket(Path::new("/tmp/nonexistent-cortex-test.sock"));
        assert!(result.is_err());
        match result.unwrap_err() {
            ClientError::ConnectionFailed(_) => {}
            other => panic!("expected ConnectionFailed, got: {other}"),
        }
    }

    #[test]
    fn connect_http_fails_on_unreachable_addr() {
        let result = DaemonClient::connect_http("127.0.0.1:19999");
        assert!(result.is_err());
        match result.unwrap_err() {
            ClientError::ConnectionFailed(_) => {}
            other => panic!("expected ConnectionFailed, got: {other}"),
        }
    }

    #[test]
    fn rpc_request_serialization() {
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "session/prompt",
            "id": 1,
            "params": { "session_id": "abc", "prompt": "hello" },
        });
        let serialized = serde_json::to_string(&request).unwrap();
        assert!(serialized.contains("session/prompt"));
        assert!(serialized.contains("hello"));
    }
}
