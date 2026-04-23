use std::sync::Mutex;

use super::client::LlmClient;
use super::types::{LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};

/// Mock LLM client for testing. Responses are consumed FIFO.
/// Panics (via error) when no responses remain — making test bugs visible.
pub struct MockLlmClient {
    responses: Mutex<Vec<LlmResponse>>,
    requests: Mutex<Vec<MockLlmRequest>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockLlmRequest {
    pub has_images: bool,
    pub message_count: usize,
    pub tool_count: usize,
}

impl MockLlmClient {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            responses: Mutex::new(Vec::new()),
            requests: Mutex::new(Vec::new()),
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

    /// Snapshot of requests received so far.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    #[must_use]
    pub fn requests(&self) -> Vec<MockLlmRequest> {
        self.requests.lock().unwrap().clone()
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
        self.requests.lock().unwrap().push(MockLlmRequest {
            has_images: request
                .messages
                .iter()
                .any(cortex_types::Message::has_images),
            message_count: request.messages.len(),
            tool_count: request.tools.map_or(0, <[serde_json::Value]>::len),
        });

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
