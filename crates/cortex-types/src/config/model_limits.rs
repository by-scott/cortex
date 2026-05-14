use super::{DEFAULT_MAX_TOKENS_FALLBACK, ModelTokenLimits, ProviderProtocol};

const DEFAULT_INFERRED_CONTEXT_TOKENS: usize = 128_000;
const DEFAULT_INFERRED_OUTPUT_TOKENS: usize = DEFAULT_MAX_TOKENS_FALLBACK;

#[must_use]
pub fn inferred_model_token_limits(
    provider_name: &str,
    protocol: &ProviderProtocol,
    model: &str,
) -> ModelTokenLimits {
    let provider = provider_name.to_ascii_lowercase();
    let model = model.to_ascii_lowercase();
    let mut limits = model_family_token_limits(&provider, protocol, &model);
    if let Some(context_tokens) = context_tokens_from_model_name(&model) {
        limits.context_tokens = context_tokens;
    }
    limits
}

#[must_use]
pub fn resolved_model_token_limits(
    provider_name: &str,
    protocol: &ProviderProtocol,
    model: &str,
    context_tokens: usize,
    output_tokens: usize,
) -> ModelTokenLimits {
    inferred_model_token_limits(provider_name, protocol, model)
        .with_overrides(context_tokens, output_tokens)
}

fn model_family_token_limits(
    provider: &str,
    protocol: &ProviderProtocol,
    model: &str,
) -> ModelTokenLimits {
    if model.contains("claude") {
        return ModelTokenLimits::new(200_000, 8_192);
    }
    if model.contains("gpt-3.5") {
        return ModelTokenLimits::new(16_385, 4_096);
    }
    if model.contains("gpt-4o")
        || model.contains("gpt-4.1")
        || model.starts_with("o1")
        || model.starts_with("o3")
        || model.starts_with("o4")
    {
        return ModelTokenLimits::new(128_000, 16_384);
    }
    if model.contains("gpt-4") {
        return ModelTokenLimits::new(128_000, 8_192);
    }
    non_openai_family_token_limits(provider, protocol, model)
}

fn non_openai_family_token_limits(
    provider: &str,
    protocol: &ProviderProtocol,
    model: &str,
) -> ModelTokenLimits {
    if model.contains("glm") || provider.contains("zai") || provider.contains("zhipu") {
        return ModelTokenLimits::new(128_000, 8_192);
    }
    if model.contains("moonshot") || model.contains("kimi") || provider.contains("moonshot") {
        return ModelTokenLimits::new(128_000, 8_192);
    }
    if model.contains("qwen") {
        return ModelTokenLimits::new(128_000, 8_192);
    }
    if model.contains("deepseek") {
        return ModelTokenLimits::new(64_000, 8_192);
    }
    open_weight_family_token_limits(protocol, model)
}

fn open_weight_family_token_limits(protocol: &ProviderProtocol, model: &str) -> ModelTokenLimits {
    if model.contains("llama-3.1")
        || model.contains("llama3.1")
        || model.contains("llama-3.2")
        || model.contains("llama3.2")
        || model.contains("llama-3.3")
        || model.contains("llama3.3")
        || model.contains("llama-4")
        || model.contains("llama4")
    {
        return ModelTokenLimits::new(128_000, 8_192);
    }
    if model.contains("llama") {
        return ModelTokenLimits::new(8_192, 4_096);
    }
    if model.contains("mistral") || model.contains("mixtral") {
        return ModelTokenLimits::new(32_768, 8_192);
    }
    protocol_default_token_limits(protocol)
}

const fn protocol_default_token_limits(protocol: &ProviderProtocol) -> ModelTokenLimits {
    match protocol {
        ProviderProtocol::Anthropic => ModelTokenLimits::new(200_000, 8_192),
        ProviderProtocol::OpenAI => ModelTokenLimits::new(
            DEFAULT_INFERRED_CONTEXT_TOKENS,
            DEFAULT_INFERRED_OUTPUT_TOKENS,
        ),
        ProviderProtocol::Ollama => ModelTokenLimits::new(32_768, 4_096),
    }
}

fn context_tokens_from_model_name(model: &str) -> Option<usize> {
    const MARKERS: &[(&str, usize)] = &[
        ("1m", 1_000_000),
        ("1000k", 1_000_000),
        ("512k", 512_000),
        ("256k", 256_000),
        ("200k", 200_000),
        ("128k", 128_000),
        ("100k", 100_000),
        ("64k", 64_000),
        ("32k", 32_000),
        ("16k", 16_000),
        ("8k", 8_000),
    ];
    MARKERS
        .iter()
        .find_map(|(marker, tokens)| model.contains(marker).then_some(*tokens))
}
