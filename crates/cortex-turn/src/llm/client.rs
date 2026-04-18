use super::types::{LlmError, LlmRequest, LlmResponse};

/// Core trait for LLM provider backends.
#[async_trait::async_trait]
pub trait LlmClient: Send + Sync {
    /// Send a completion request and return the response.
    ///
    /// # Errors
    /// Returns `LlmError` on network failure, parse failure, or provider error.
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError>;
}
