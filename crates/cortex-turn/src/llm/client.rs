use super::types::{LlmError, LlmRequest, LlmResponse};

/// Core trait for LLM provider backends.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Whether this provider accepts tool definitions in requests that also
    /// contain image content blocks.
    fn supports_tools_with_images(&self) -> bool {
        true
    }

    /// Send a completion request and return the response.
    ///
    /// # Errors
    /// Returns `LlmError` on network failure, parse failure, or provider error.
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError>;
}
