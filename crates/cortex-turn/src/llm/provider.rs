use super::anthropic::AnthropicClient;
use super::client::LlmClient;
use super::ollama::OllamaClient;
use super::openai::OpenAIClient;
use super::types::{ProviderProtocol, ResolvedEndpoint};

/// Create an LLM client from a resolved endpoint configuration.
#[must_use]
pub fn create_llm_client(endpoint: &ResolvedEndpoint) -> Box<dyn LlmClient> {
    match endpoint.protocol {
        ProviderProtocol::Anthropic => Box::new(AnthropicClient::new(
            &endpoint.base_url,
            &endpoint.api_key,
            &endpoint.model,
            endpoint.vision_max_output_tokens,
            &endpoint.capability_cache_path,
            endpoint.capability_cache_ttl_hours,
        )),
        ProviderProtocol::OpenAI => Box::new(OpenAIClient::new(endpoint)),
        ProviderProtocol::Ollama => Box::new(OllamaClient::new(
            &endpoint.base_url,
            &endpoint.model,
            endpoint.vision_max_output_tokens,
        )),
    }
}
