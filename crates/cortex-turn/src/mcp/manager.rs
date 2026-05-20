use std::sync::Arc;

use cortex_types::config::{McpConfig, McpServerConfig, McpTransportType};
use cortex_types::mcp::{McpResponse, McpToolInfo};

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

        let tools = parse_tools_response(&config.name, response)?;

        Ok((config.name.clone(), session, tools))
    }
}

fn parse_tools_response(
    server_name: &str,
    response: McpResponse,
) -> Result<Vec<McpToolInfo>, McpTransportError> {
    if let Some(error) = response.error {
        return Err(McpTransportError::Protocol(format!(
            "tools/list failed for {server_name}: {} (code {})",
            error.message, error.code
        )));
    }

    let Some(result) = response.result else {
        return Ok(Vec::new());
    };
    let Some(raw_tools) = result.get("tools").and_then(serde_json::Value::as_array) else {
        return Ok(Vec::new());
    };

    let mut tools = Vec::with_capacity(raw_tools.len());
    for raw_tool in raw_tools {
        let Some(name) = raw_tool
            .get("name")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty())
        else {
            tracing::warn!(server = server_name, tool = %raw_tool, "MCP tool missing name");
            continue;
        };
        let description = raw_tool
            .get("description")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_string();
        let input_schema = raw_tool
            .get("inputSchema")
            .or_else(|| raw_tool.get("input_schema"))
            .cloned()
            .unwrap_or_else(|| serde_json::json!({"type": "object"}));
        tools.push(McpToolInfo {
            name: name.to_string(),
            description,
            input_schema,
        });
    }

    Ok(tools)
}

impl Default for McpManager {
    fn default() -> Self {
        Self
    }
}
