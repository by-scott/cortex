use crate::tools::ToolRegistry;
use cortex_types::mcp::{
    MCP_PROTOCOL_VERSION, McpError, McpInitializeResult, McpRequest, McpResponse,
    McpServerCapabilities, McpServerInfo,
};

/// MCP server that exposes cortex tools to external MCP clients.
pub struct McpServer<'a> {
    tools: &'a ToolRegistry,
}

impl<'a> McpServer<'a> {
    #[must_use]
    pub const fn new(tools: &'a ToolRegistry) -> Self {
        Self { tools }
    }

    /// Handle an incoming JSON-RPC request and return a response.
    #[must_use]
    pub fn handle_request(&self, request: &McpRequest) -> McpResponse {
        match request.method.as_str() {
            "initialize" => self.handle_initialize(request.id),
            "notifications/initialized" | "initialized" => {
                // Notification -- no response needed, but if sent as request, ack it
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(request.id),
                    result: Some(serde_json::json!({})),
                    error: None,
                }
            }
            "tools/list" => self.handle_tools_list(request.id),
            "tools/call" => self.handle_tools_call(request.id, &request.params),
            _ => McpResponse {
                jsonrpc: "2.0".into(),
                id: Some(request.id),
                result: None,
                error: Some(McpError {
                    code: -32601,
                    message: format!("Method not found: {}", request.method),
                    data: None,
                }),
            },
        }
    }

    fn handle_initialize(&self, id: u64) -> McpResponse {
        let has_tools = !self.tools.tool_names().is_empty();
        let result = McpInitializeResult {
            protocol_version: MCP_PROTOCOL_VERSION.into(),
            capabilities: McpServerCapabilities {
                tools: if has_tools {
                    Some(serde_json::json!({}))
                } else {
                    None
                },
                resources: None,
                prompts: None,
            },
            server_info: McpServerInfo {
                name: "cortex".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
        };
        McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::to_value(result).unwrap_or_default()),
            error: None,
        }
    }

    fn handle_tools_list(&self, id: u64) -> McpResponse {
        let tools: Vec<cortex_types::mcp::McpToolInfo> = self
            .tools
            .tool_names()
            .iter()
            .filter_map(|name| self.tools.get(name))
            .map(|tool| cortex_types::mcp::McpToolInfo {
                name: tool.name().to_string(),
                description: tool.description().to_string(),
                input_schema: tool.input_schema(),
            })
            .collect();

        McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(id),
            result: Some(serde_json::json!({ "tools": tools })),
            error: None,
        }
    }

    fn handle_tools_call(&self, id: u64, params: &serde_json::Value) -> McpResponse {
        let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");

        let arguments = params
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));

        let Some(tool) = self.tools.get(tool_name) else {
            return McpResponse {
                jsonrpc: "2.0".into(),
                id: Some(id),
                result: None,
                error: Some(McpError {
                    code: -32602,
                    message: format!("Tool not found: {tool_name}"),
                    data: None,
                }),
            };
        };

        match tool.execute(arguments) {
            Ok(result) => {
                let content = serde_json::json!([{
                    "type": "text",
                    "text": result.output,
                }]);
                McpResponse {
                    jsonrpc: "2.0".into(),
                    id: Some(id),
                    result: Some(serde_json::json!({
                        "content": content,
                        "isError": result.is_error,
                    })),
                    error: None,
                }
            }
            Err(e) => McpResponse {
                jsonrpc: "2.0".into(),
                id: Some(id),
                result: Some(serde_json::json!({
                    "content": [{"type": "text", "text": e.to_string()}],
                    "isError": true,
                })),
                error: None,
            },
        }
    }
}
