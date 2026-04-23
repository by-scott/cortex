use std::sync::Arc;

use crate::tools::{Tool, ToolError, ToolResult, block_on_tool_future};

use super::McpTransport;

pub struct McpToolBridge {
    prefixed_name: &'static str,
    tool_name: String,
    description: &'static str,
    input_schema: serde_json::Value,
    transport: Arc<dyn McpTransport>,
}

impl McpToolBridge {
    /// Create a new MCP tool bridge for a specific server and tool.
    pub fn new(
        server_name: &str,
        tool_name: &str,
        description: &str,
        input_schema: serde_json::Value,
        transport: Arc<dyn McpTransport>,
    ) -> Self {
        let prefixed = format!("mcp_{server_name}_{tool_name}");
        Self {
            prefixed_name: Box::leak(prefixed.into_boxed_str()),
            tool_name: tool_name.to_string(),
            description: Box::leak(description.to_string().into_boxed_str()),
            input_schema,
            transport,
        }
    }
}

impl Tool for McpToolBridge {
    fn name(&self) -> &'static str {
        self.prefixed_name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let params = serde_json::json!({
            "name": self.tool_name,
            "arguments": input,
        });

        let response = block_on_tool_future(async {
            self.transport
                .send_request("tools/call", params)
                .await
                .map_err(|e| ToolError::ExecutionFailed(format!("MCP transport: {e}")))
        })?;

        if let Some(error) = &response.error {
            return Ok(ToolResult::error(format!(
                "MCP error ({}): {}",
                error.code, error.message
            )));
        }

        let result = response.result.unwrap_or_else(|| serde_json::json!({}));
        let is_error = result
            .get("isError")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let content = extract_content(&result);
        if is_error {
            Ok(ToolResult::error(content))
        } else {
            Ok(ToolResult::success(content))
        }
    }
}

fn extract_content(result: &serde_json::Value) -> String {
    result
        .get("content")
        .and_then(|c| c.as_array())
        .map_or_else(
            || result.to_string(),
            |content| {
                content
                    .iter()
                    .filter_map(|item| {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            item.get("text").and_then(|t| t.as_str()).map(String::from)
                        } else {
                            Some(item.to_string())
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            },
        )
}
