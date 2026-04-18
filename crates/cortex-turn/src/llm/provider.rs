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
        )),
        ProviderProtocol::OpenAI => Box::new(OpenAIClient::new(
            &endpoint.base_url,
            &endpoint.api_key,
            &endpoint.model,
            endpoint.auth_type.clone(),
        )),
        ProviderProtocol::Ollama => {
            Box::new(OllamaClient::new(&endpoint.base_url, &endpoint.model))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::types::AuthType;
    use super::*;

    fn make_endpoint(protocol: ProviderProtocol) -> ResolvedEndpoint {
        ResolvedEndpoint {
            base_url: "http://localhost".into(),
            protocol,
            auth_type: AuthType::None,
            api_key: "test-key".into(),
            model: "test-model".into(),
            max_tokens: cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK,
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
