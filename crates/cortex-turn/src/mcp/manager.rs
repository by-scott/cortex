use std::sync::Arc;

use cortex_types::config::{McpConfig, McpServerConfig, McpTransportType};
use cortex_types::mcp::McpToolInfo;

use super::bridge::McpToolBridge;
use super::session::McpSession;
use super::sse::SseTransport;
use super::stdio::StdioTransport;
use super::{McpTransport, McpTransportError};
use crate::tools::ToolRegistry;

pub struct McpManager;

impl McpManager {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub async fn connect_and_register(
        &self,
        config: &McpConfig,
        registry: &mut ToolRegistry,
    ) -> Vec<String> {
        let mut warnings = Vec::new();

        for server_config in &config.servers {
            match self.connect_server(server_config).await {
                Ok((name, session, tools)) => {
                    let transport: Arc<dyn McpTransport> = Arc::from(session.into_transport());

                    for tool_info in &tools {
                        let bridge = McpToolBridge::new(
                            &name,
                            &tool_info.name,
                            &tool_info.description,
                            tool_info.input_schema.clone(),
                            transport.clone(),
                        );
                        registry.register(Box::new(bridge));
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "MCP server '{}' connection failed: {}",
                        server_config.name, e
                    ));
                }
            }
        }

        warnings
    }

    pub async fn connect_and_register_live(
        &self,
        config: &McpConfig,
        registry: &ToolRegistry,
    ) -> Vec<String> {
        let mut warnings = Vec::new();

        for server_config in &config.servers {
            match self.connect_server(server_config).await {
                Ok((name, session, tools)) => {
                    let transport: Arc<dyn McpTransport> = Arc::from(session.into_transport());

                    for tool_info in &tools {
                        let bridge = McpToolBridge::new(
                            &name,
                            &tool_info.name,
                            &tool_info.description,
                            tool_info.input_schema.clone(),
                            transport.clone(),
                        );
                        registry.register_live(Box::new(bridge));
                    }
                }
                Err(e) => {
                    warnings.push(format!(
                        "MCP server '{}' connection failed: {}",
                        server_config.name, e
                    ));
                }
            }
        }

        warnings
    }

    async fn connect_server(
        &self,
        config: &McpServerConfig,
    ) -> Result<(String, McpSession, Vec<McpToolInfo>), McpTransportError> {
        let transport: Box<dyn McpTransport> = match config.transport {
            McpTransportType::Stdio => Box::new(StdioTransport::new(
                &config.command,
                &config.args,
                &config.env,
            )?),
            McpTransportType::Sse => Box::new(SseTransport::new(&config.url, &config.headers)),
        };

        let mut session = McpSession::new(transport);
        session.initialize().await?;

        let response = session
            .transport()
            .send_request("tools/list", serde_json::json!({}))
            .await?;

        let tools = response.result.map_or_else(Vec::new, |result| {
            let tools_value = result
                .get("tools")
                .cloned()
                .unwrap_or(serde_json::json!([]));
            serde_json::from_value::<Vec<McpToolInfo>>(tools_value).unwrap_or_default()
        });

        Ok((config.name.clone(), session, tools))
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self
    }
}
