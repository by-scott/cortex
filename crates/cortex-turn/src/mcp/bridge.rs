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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpTransport, McpTransportError};
    use async_trait::async_trait;
    use cortex_types::mcp::McpResponse;
    use std::sync::Mutex;

    struct MockTransport {
        responses: Mutex<Vec<McpResponse>>,
    }

    impl MockTransport {
        fn with_responses(responses: Vec<McpResponse>) -> Arc<Self> {
            Arc::new(Self {
                responses: Mutex::new(responses),
            })
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
                Err(McpTransportError::Io("no more responses".into()))
            } else {
                Ok(responses.remove(0))
            }
        }

        async fn send_notification(
            &self,
            _method: &str,
            _params: serde_json::Value,
        ) -> Result<(), McpTransportError> {
            Ok(())
        }
    }

    #[test]
    fn tool_name_has_server_prefix() {
        let transport = MockTransport::with_responses(vec![]);
        let bridge = McpToolBridge::new(
            "fs",
            "read_file",
            "Read a file",
            serde_json::json!({}),
            transport,
        );
        assert_eq!(bridge.name(), "mcp_fs_read_file");
    }

    #[test]
    fn execute_success() {
        let resp = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(serde_json::json!({
                "content": [{"type": "text", "text": "file contents here"}]
            })),
            error: None,
        };
        let transport = MockTransport::with_responses(vec![resp]);
        let bridge = McpToolBridge::new(
            "fs",
            "read_file",
            "Read a file",
            serde_json::json!({}),
            transport,
        );
        let result = bridge
            .execute(serde_json::json!({"path": "/tmp/test"}))
            .unwrap();
        assert!(!result.is_error);
        assert_eq!(result.output, "file contents here");
    }

    #[test]
    fn execute_mcp_error_flag() {
        let resp = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: Some(serde_json::json!({
                "isError": true,
                "content": [{"type": "text", "text": "file not found"}]
            })),
            error: None,
        };
        let transport = MockTransport::with_responses(vec![resp]);
        let bridge = McpToolBridge::new(
            "fs",
            "read_file",
            "Read a file",
            serde_json::json!({}),
            transport,
        );
        let result = bridge.execute(serde_json::json!({"path": "/no"})).unwrap();
        assert!(result.is_error);
        assert_eq!(result.output, "file not found");
    }

    #[test]
    fn execute_rpc_error() {
        let resp = McpResponse {
            jsonrpc: "2.0".into(),
            id: Some(1),
            result: None,
            error: Some(cortex_types::mcp::McpError {
                code: -32601,
                message: "Method not found".into(),
                data: None,
            }),
        };
        let transport = MockTransport::with_responses(vec![resp]);
        let bridge = McpToolBridge::new(
            "fs",
            "read_file",
            "Read a file",
            serde_json::json!({}),
            transport,
        );
        let result = bridge.execute(serde_json::json!({})).unwrap();
        assert!(result.is_error);
        assert!(result.output.contains("Method not found"));
    }

    #[test]
    fn extract_content_text_array() {
        let result = serde_json::json!({
            "content": [
                {"type": "text", "text": "line 1"},
                {"type": "text", "text": "line 2"}
            ]
        });
        assert_eq!(extract_content(&result), "line 1\nline 2");
    }

    #[test]
    fn extract_content_fallback() {
        let result = serde_json::json!({"data": 42});
        assert_eq!(extract_content(&result), r#"{"data":42}"#);
    }
}
