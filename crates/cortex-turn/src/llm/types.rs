use std::fmt;

use serde::{Deserialize, Serialize};

// Re-export config types used by the LLM layer
pub use cortex_types::config::{
    ApiEndpointConfig, AuthType, OpenAiImageInputMode, ProviderConfig, ProviderProtocol,
    ResolvedEndpoint,
};

#[derive(Debug)]
pub enum LlmError {
    RequestFailed(String),
    ParseError(String),
    ProviderNotFound(String),
}

impl fmt::Display for LlmError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestFailed(e) => write!(f, "LLM request failed: {e}"),
            Self::ParseError(e) => write!(f, "LLM parse error: {e}"),
            Self::ProviderNotFound(p) => write!(f, "LLM provider not found: {p}"),
        }
    }
}

impl std::error::Error for LlmError {}

#[derive(Debug, Clone)]
pub struct LlmResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<LlmToolCall>,
    pub usage: Usage,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: usize,
    pub output_tokens: usize,
}

/// Request to an LLM provider. Borrows message data to avoid cloning.
pub struct LlmRequest<'a> {
    pub system: Option<&'a str>,
    pub messages: &'a [cortex_types::Message],
    pub tools: Option<&'a [serde_json::Value]>,
    pub max_tokens: usize,
    pub transient_retries: usize,
    pub on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
}

impl Usage {
    #[must_use]
    pub const fn total(&self) -> usize {
        self.input_tokens + self.output_tokens
    }
}
