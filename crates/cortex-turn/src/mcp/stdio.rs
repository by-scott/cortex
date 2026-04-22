use async_trait::async_trait;
use cortex_types::mcp::{McpNotification, McpRequest, McpResponse};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

use super::{McpTransport, McpTransportError};

pub struct StdioTransport {
    child: Mutex<Child>,
    stdin: Mutex<tokio::process::ChildStdin>,
    reader: Mutex<BufReader<tokio::process::ChildStdout>>,
    next_id: AtomicU64,
}

impl StdioTransport {
    /// Spawn a child process and set up stdio-based MCP transport.
    ///
    /// # Errors
    /// Returns `McpTransportError` if the process cannot be spawned.
    pub fn new(
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<Self, McpTransportError> {
        let mut cmd = Command::new(command);
        cmd.args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            // Sandbox: clear inherited environment to prevent credential leakage
            .env_clear()
            // Sandbox: run in temp directory, not project directory
            .current_dir(std::env::temp_dir());

        // Pass through essential environment variables for child process.
        // PATH: locate binaries; HOME: resolve ~; CORTEX_HOME: find instances.
        for var in ["PATH", "HOME", "CORTEX_HOME"] {
            if let Ok(val) = std::env::var(var) {
                cmd.env(var, val);
            }
        }
        // Only pass explicitly configured environment variables
        for (k, v) in env {
            cmd.env(k, v);
        }

        let mut child = cmd
            .spawn()
            .map_err(|e| McpTransportError::Io(format!("failed to spawn {command}: {e}")))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| McpTransportError::Io("no stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| McpTransportError::Io("no stdout".into()))?;

        Ok(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            reader: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicU64::new(1),
        })
    }

    /// Shut down the child process.
    ///
    /// # Errors
    /// Returns `McpTransportError::Io` if the kill fails.
    pub async fn shutdown(&self) -> Result<(), McpTransportError> {
        let mut child = self.child.lock().await;
        child
            .kill()
            .await
            .map_err(|e| McpTransportError::Io(format!("kill failed: {e}")))
    }

    async fn write_message(&self, data: &[u8]) -> Result<(), McpTransportError> {
        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(data)
            .await
            .map_err(|e| McpTransportError::Io(format!("write failed: {e}")))?;
        stdin
            .write_all(b"\n")
            .await
            .map_err(|e| McpTransportError::Io(format!("write newline failed: {e}")))?;
        stdin
            .flush()
            .await
            .map_err(|e| McpTransportError::Io(format!("flush failed: {e}")))?;
        drop(stdin);
        Ok(())
    }

    async fn read_response(&self) -> Result<McpResponse, McpTransportError> {
        let mut reader = self.reader.lock().await;
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .await
            .map_err(|e| McpTransportError::Io(format!("read failed: {e}")))?;
        drop(reader);
        if line.is_empty() {
            return Err(McpTransportError::Io("child process closed stdout".into()));
        }
        serde_json::from_str(line.trim())
            .map_err(|e| McpTransportError::Protocol(format!("invalid JSON-RPC response: {e}")))
    }
}

#[async_trait]
impl McpTransport for StdioTransport {
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<McpResponse, McpTransportError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = McpRequest::new(id, method, params);
        let data = serde_json::to_vec(&request)
            .map_err(|e| McpTransportError::Protocol(format!("serialize failed: {e}")))?;
        self.write_message(&data).await?;
        self.read_response().await
    }

    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpTransportError> {
        let notif = McpNotification::new(method, params);
        let data = serde_json::to_vec(&notif)
            .map_err(|e| McpTransportError::Protocol(format!("serialize failed: {e}")))?;
        self.write_message(&data).await
    }
}
