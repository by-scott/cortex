pub mod bridge;
pub mod manager;
pub mod server;
mod session;
mod sse;
mod stdio;

pub use bridge::McpToolBridge;
pub use manager::McpManager;
pub use server::McpServer;
pub use session::McpSession;
pub use sse::SseTransport;
pub use stdio::StdioTransport;

use async_trait::async_trait;
use cortex_types::mcp::McpResponse;

#[derive(Debug)]
pub enum McpTransportError {
    Io(String),
    Protocol(String),
    Timeout(String),
}

impl std::fmt::Display for McpTransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "MCP I/O error: {e}"),
            Self::Protocol(e) => write!(f, "MCP protocol error: {e}"),
            Self::Timeout(e) => write!(f, "MCP timeout: {e}"),
        }
    }
}

impl std::error::Error for McpTransportError {}

#[async_trait]
pub trait McpTransport: Send + Sync {
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<McpResponse, McpTransportError>;

    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpTransportError>;
}
