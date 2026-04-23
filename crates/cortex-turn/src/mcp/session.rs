use cortex_types::mcp::{
    MCP_PROTOCOL_VERSION, McpClientCapabilities, McpClientInfo, McpInitializeParams,
    McpInitializeResult, McpServerCapabilities,
};

use super::{McpTransport, McpTransportError};

pub struct McpSession {
    transport: Box<dyn McpTransport>,
    server_capabilities: Option<McpServerCapabilities>,
    server_name: Option<String>,
    initialized: bool,
}

impl McpSession {
    #[must_use]
    pub fn new(transport: Box<dyn McpTransport>) -> Self {
        Self {
            transport,
            server_capabilities: None,
            server_name: None,
            initialized: false,
        }
    }

    /// Perform the MCP `initialize` handshake.
    ///
    /// # Errors
    /// Returns `McpTransportError` if the initialize request or response parsing fails.
    pub async fn initialize(&mut self) -> Result<McpInitializeResult, McpTransportError> {
        let params = McpInitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: McpClientCapabilities {
                roots: Some(serde_json::json!({})),
            },
            client_info: McpClientInfo {
                name: "cortex".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        };

        let params_json = serde_json::to_value(&params).map_err(|e| {
            McpTransportError::Protocol(format!("serialize initialize params: {e}"))
        })?;

        let response = self
            .transport
            .send_request("initialize", params_json)
            .await?;

        if let Some(error) = &response.error {
            return Err(McpTransportError::Protocol(format!(
                "initialize failed: {} (code {})",
                error.message, error.code
            )));
        }

        let result_value = response.result.ok_or_else(|| {
            McpTransportError::Protocol("initialize response has no result".into())
        })?;

        let result: McpInitializeResult = serde_json::from_value(result_value)
            .map_err(|e| McpTransportError::Protocol(format!("parse initialize result: {e}")))?;

        if result.protocol_version != MCP_PROTOCOL_VERSION {
            return Err(McpTransportError::Protocol(format!(
                "protocol version mismatch: server={}, client={}",
                result.protocol_version, MCP_PROTOCOL_VERSION
            )));
        }

        self.transport
            .send_notification("notifications/initialized", serde_json::json!({}))
            .await?;

        self.server_capabilities = Some(result.capabilities.clone());
        self.server_name = Some(result.server_info.name.clone());
        self.initialized = true;

        Ok(result)
    }

    #[must_use]
    pub const fn is_initialized(&self) -> bool {
        self.initialized
    }

    #[must_use]
    pub const fn server_capabilities(&self) -> Option<&McpServerCapabilities> {
        self.server_capabilities.as_ref()
    }

    #[must_use]
    pub fn server_name(&self) -> Option<&str> {
        self.server_name.as_deref()
    }

    #[must_use]
    pub fn transport(&self) -> &dyn McpTransport {
        self.transport.as_ref()
    }

    #[must_use]
    pub fn into_transport(self) -> Box<dyn McpTransport> {
        self.transport
    }
}
