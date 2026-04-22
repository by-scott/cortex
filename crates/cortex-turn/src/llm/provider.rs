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

#[cfg(test)]
mod tests {
    use super::super::types::AuthType;
    use super::*;

    fn make_endpoint(protocol: ProviderProtocol) -> ResolvedEndpoint {
        ResolvedEndpoint {
            provider: "test".into(),
            base_url: "http://localhost".into(),
            protocol,
            auth_type: AuthType::None,
            api_key: "test-key".into(),
            model: "test-model".into(),
            max_tokens: cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK,
            vision_max_output_tokens: cortex_types::config::DEFAULT_VISION_MAX_OUTPUT_TOKENS,
            image_input_mode: cortex_types::config::OpenAiImageInputMode::DataUrl,
            files_base_url: String::new(),
            openai_stream_options: false,
            capability_cache_path: String::new(),
            capability_cache_ttl_hours: 0,
        }
    }

    #[test]
    fn create_anthropic() {
        let _ = create_llm_client(&make_endpoint(ProviderProtocol::Anthropic));
    }

    #[test]
    fn create_openai() {
        let _ = create_llm_client(&make_endpoint(ProviderProtocol::OpenAI));
    }

    #[test]
    fn create_ollama() {
        let _ = create_llm_client(&make_endpoint(ProviderProtocol::Ollama));
    }
}
