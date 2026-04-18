use std::sync::Mutex;

use super::client::LlmClient;
use super::types::{LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};

/// Mock LLM client for testing. Responses are consumed FIFO.
/// Panics (via error) when no responses remain — making test bugs visible.
pub struct MockLlmClient {
    responses: Mutex<Vec<LlmResponse>>,
}

impl MockLlmClient {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
        }
    }

    /// Enqueue a full response.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn push_response(&self, response: LlmResponse) {
        self.responses.lock().unwrap().push(response);
    }

    /// Convenience: enqueue a text-only response.
    pub fn push_text(&self, text: &str) {
        self.push_response(LlmResponse {
            text: Some(text.to_string()),
            tool_calls: Vec::new(),
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
            model: "mock".into(),
        });
    }

    /// Convenience: enqueue a tool call response.
    pub fn push_tool_call(&self, id: &str, name: &str, input: serde_json::Value) {
        self.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: id.into(),
                name: name.into(),
                input,
            }],
            usage: Usage {
                input_tokens: 10,
                output_tokens: 5,
            },
            model: "mock".into(),
        });
    }
}

impl Default for MockLlmClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl LlmClient for MockLlmClient {
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let response = {
            let mut responses = self.responses.lock().unwrap();
            if responses.is_empty() {
                return Err(LlmError::RequestFailed(
                    "no mock responses remaining".into(),
                ));
            }
            responses.remove(0)
        };

        // Invoke streaming callback if both text and callback are present
        if let Some(text) = &response.text
            && let Some(cb) = request.on_text
        {
            cb(text);
        }

        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fifo_order() {
        let mock = MockLlmClient::new();
        mock.push_text("first");
        mock.push_text("second");
        let req = LlmRequest {
            system: None,
            messages: &[],
            tools: None,
            max_tokens: 100,
            on_text: None,
        };
        let r1 = mock.complete(req).await.unwrap();
        assert_eq!(r1.text.as_deref(), Some("first"));
        let req2 = LlmRequest {
            system: None,
            messages: &[],
            tools: None,
            max_tokens: 100,
            on_text: None,
        };
        let r2 = mock.complete(req2).await.unwrap();
        assert_eq!(r2.text.as_deref(), Some("second"));
    }

    #[tokio::test]
    async fn tool_call() {
        let mock = MockLlmClient::new();
        mock.push_tool_call("t1", "read", serde_json::json!({"path": "/tmp"}));
        let req = LlmRequest {
            system: None,
            messages: &[],
            tools: None,
            max_tokens: 100,
            on_text: None,
        };
        let resp = mock.complete(req).await.unwrap();
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "read");
    }

    #[tokio::test]
    async fn empty_returns_error() {
        let mock = MockLlmClient::new();
        let req = LlmRequest {
            system: None,
            messages: &[],
            tools: None,
            max_tokens: 100,
            on_text: None,
        };
        assert!(mock.complete(req).await.is_err());
    }
}
