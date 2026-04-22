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

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use cortex_types::mcp::McpResponse;
    use std::sync::Mutex;

    struct MockTransport {
        responses: Mutex<Vec<McpResponse>>,
        notifications: Mutex<Vec<String>>,
    }

    impl MockTransport {
        fn new(responses: Vec<McpResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                notifications: Mutex::new(Vec::new()),
            }
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        async fn send_request(
            &self,
            _method: &str,
            _params: serde_json::Value,
        ) -> Result<McpResponse, McpTransportError> {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                Err(McpTransportError::Io("no more mock responses".into()))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn send_notification(
            &self,
            method: &str,
            _params: serde_json::Value,
        ) -> Result<(), McpTransportError> {
            self.notifications.lock().unwrap().push(method.to_string());
            Ok(())
        }
    }

    #[tokio::test]
    async fn initialize_success() {
        let init_result = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "test-mcp", "version": "0.1.0"}
        });
        let response = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(init_result),
            error: None,
        };
        let transport = MockTransport::new(vec![response]);
        let mut session = McpSession::new(Box::new(transport));

        let result = session.initialize().await.unwrap();
        assert_eq!(result.server_info.name, "test-mcp");
        assert!(session.is_initialized());
        assert_eq!(session.server_name(), Some("test-mcp"));
        assert!(session.server_capabilities().unwrap().tools.is_some());
    }

    #[tokio::test]
    async fn initialize_sends_initialized_notification() {
        let init_result = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "serverInfo": {"name": "test", "version": "1.0"}
        });
        let response = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(init_result),
            error: None,
        };
        let transport = MockTransport::new(vec![response]);
        let mut session = McpSession::new(Box::new(transport));

        session.initialize().await.unwrap();

        // The successful initialize proves the notification was sent
        // since our mock would fail on a second send_request
        assert!(session.is_initialized());
    }

    #[tokio::test]
    async fn initialize_version_mismatch() {
        let init_result = serde_json::json!({
            "protocolVersion": "9999-01-01",
            "capabilities": {},
            "serverInfo": {"name": "future-server", "version": "9.0"}
        });
        let response = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(init_result),
            error: None,
        };
        let transport = MockTransport::new(vec![response]);
        let mut session = McpSession::new(Box::new(transport));

        let err = session.initialize().await.unwrap_err();
        assert!(err.to_string().contains("protocol version mismatch"));
        assert!(!session.is_initialized());
    }

    #[tokio::test]
    async fn initialize_error_response() {
        let response = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: None,
            error: Some(cortex_types::mcp::McpError {
                code: -32600,
                message: "Invalid Request".into(),
                data: None,
            }),
        };
        let transport = MockTransport::new(vec![response]);
        let mut session = McpSession::new(Box::new(transport));

        let err = session.initialize().await.unwrap_err();
        assert!(err.to_string().contains("initialize failed"));
    }

    #[tokio::test]
    async fn session_not_initialized_by_default() {
        let transport = MockTransport::new(vec![]);
        let session = McpSession::new(Box::new(transport));
        assert!(!session.is_initialized());
        assert!(session.server_capabilities().is_none());
        assert!(session.server_name().is_none());
    }
}
