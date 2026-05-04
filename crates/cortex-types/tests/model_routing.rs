use cortex_types::config::{
    CortexConfig, LlmGroupConfig, ProviderConfig, ProviderProtocol, ProviderRegistry,
    ResolvedEndpoint,
};
use cortex_types::{
    ModelCapability, ModelCapabilityRegistry, ModelFallbackReason, ModelRouteRequest,
    ModelRouteTarget,
};

fn provider_registry() -> ProviderRegistry {
    let mut providers = ProviderRegistry::new();
    providers.insert(
        "test".to_string(),
        ProviderConfig {
            name: "test".to_string(),
            base_url: "https://example.invalid".to_string(),
            models: vec![
                "fast-json".to_string(),
                "balanced-json".to_string(),
                "heavy-reasoner".to_string(),
            ],
            ..ProviderConfig::default()
        },
    );
    providers
}

fn base_config() -> CortexConfig {
    let mut config = CortexConfig::default();
    config.api.provider = "test".to_string();
    config.api.model = "heavy-reasoner".to_string();
    config.llm_groups.insert(
        "light".to_string(),
        LlmGroupConfig {
            provider: "test".to_string(),
            model: "fast-json".to_string(),
            capabilities: vec![
                ModelCapability::JsonReliability,
                ModelCapability::LowCost,
                ModelCapability::LowLatency,
            ],
            latency_ms: 350,
            input_cost_per_million: 0.10,
            output_cost_per_million: 0.25,
            json_reliability: 0.95,
            ..LlmGroupConfig::default()
        },
    );
    config.llm_groups.insert(
        "medium".to_string(),
        LlmGroupConfig {
            provider: "test".to_string(),
            model: "balanced-json".to_string(),
            capabilities: vec![
                ModelCapability::JsonReliability,
                ModelCapability::ToolCalling,
                ModelCapability::HighSafety,
            ],
            safety_score: 0.80,
            reasoning_depth: 0.65,
            json_reliability: 0.90,
            ..LlmGroupConfig::default()
        },
    );
    config.llm_groups.insert(
        "heavy".to_string(),
        LlmGroupConfig {
            provider: "test".to_string(),
            model: "heavy-reasoner".to_string(),
            capabilities: vec![
                ModelCapability::Coding,
                ModelCapability::ToolCalling,
                ModelCapability::HighSafety,
                ModelCapability::DeepReasoning,
            ],
            safety_score: 0.95,
            reasoning_depth: 0.95,
            input_cost_per_million: 4.0,
            output_cost_per_million: 12.0,
            ..LlmGroupConfig::default()
        },
    );
    config
}

#[test]
fn extraction_routes_to_low_cost_json_capable_group() {
    let config = base_config();
    let registry = ModelCapabilityRegistry::from_config(&config, &provider_registry());
    let decision = registry.route(&ModelRouteRequest::for_endpoint("memory_extract"));

    assert_eq!(decision.selected_group(), Some("light"));
    assert!(!decision.escalated);
    assert!(
        decision
            .explanation
            .iter()
            .any(|line| line.contains("selected light:test/fast-json"))
    );
}

#[test]
fn high_risk_low_confidence_escalates_to_safer_reasoning_model() {
    let config = base_config();
    let registry = ModelCapabilityRegistry::from_config(&config, &provider_registry());
    let request = ModelRouteRequest::for_endpoint("memory_extract")
        .with_risk(0.90)
        .with_confidence(0.20);
    let decision = registry.route(&request);

    assert_eq!(decision.selected_group(), Some("heavy"));
    assert!(decision.escalated);
    assert!(
        decision
            .explanation
            .iter()
            .any(|line| line.contains("high_safety + deep_reasoning"))
    );
}

#[test]
fn provider_failure_and_schema_invalid_fall_back_with_explanation() {
    let config = base_config();
    let registry = ModelCapabilityRegistry::from_config(&config, &provider_registry());
    let failed = ModelRouteTarget::new("light", "test", "fast-json");
    let request = ModelRouteRequest::for_endpoint("memory_extract")
        .after_failure(failed, ModelFallbackReason::ProviderFailure)
        .with_fallback_reason(ModelFallbackReason::SchemaInvalid);
    let decision = registry.route(&request);

    assert_eq!(decision.selected_group(), Some("medium"));
    assert!(decision.fallback);
    assert!(
        decision
            .explanation
            .iter()
            .any(|line| line.contains("provider_failure"))
    );
    assert!(
        decision
            .candidates
            .iter()
            .any(|candidate| candidate.target.group == "light" && candidate.rejected)
    );
}

#[test]
fn profiles_infer_model_specific_token_limits() {
    let mut providers = ProviderRegistry::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: "openai".to_string(),
            protocol: ProviderProtocol::OpenAI,
            models: vec!["gpt-4o".to_string()],
            ..ProviderConfig::default()
        },
    );
    providers.insert(
        "anthropic".to_string(),
        ProviderConfig {
            name: "anthropic".to_string(),
            protocol: ProviderProtocol::Anthropic,
            models: vec!["claude-sonnet-4-20250514".to_string()],
            ..ProviderConfig::default()
        },
    );

    let mut config = CortexConfig::default();
    config.api.provider = "openai".to_string();
    config.api.model = "gpt-4o".to_string();
    config.llm_groups.insert(
        "openai".to_string(),
        LlmGroupConfig {
            provider: "openai".to_string(),
            model: "gpt-4o".to_string(),
            ..LlmGroupConfig::default()
        },
    );
    config.llm_groups.insert(
        "claude".to_string(),
        LlmGroupConfig {
            provider: "anthropic".to_string(),
            model: "claude-sonnet-4-20250514".to_string(),
            ..LlmGroupConfig::default()
        },
    );

    let registry = ModelCapabilityRegistry::from_config(&config, &providers);
    let openai = registry
        .profiles
        .iter()
        .find(|profile| profile.target.group == "openai")
        .expect("openai profile should exist");
    let claude = registry
        .profiles
        .iter()
        .find(|profile| profile.target.group == "claude")
        .expect("claude profile should exist");

    assert_eq!(openai.context_tokens, 128_000);
    assert_eq!(openai.output_tokens, 16_384);
    assert_eq!(claude.context_tokens, 200_000);
    assert_eq!(claude.output_tokens, 8_192);
}

#[test]
fn resolved_endpoints_use_model_specific_output_caps() {
    let mut providers = ProviderRegistry::new();
    providers.insert(
        "openai".to_string(),
        ProviderConfig {
            name: "openai".to_string(),
            protocol: ProviderProtocol::OpenAI,
            models: vec!["gpt-4o".to_string()],
            ..ProviderConfig::default()
        },
    );
    providers.insert(
        "anthropic".to_string(),
        ProviderConfig {
            name: "anthropic".to_string(),
            protocol: ProviderProtocol::Anthropic,
            models: vec!["claude-sonnet-4-20250514".to_string()],
            ..ProviderConfig::default()
        },
    );

    let mut openai = CortexConfig::default();
    openai.api.provider = "openai".to_string();
    openai.api.model = "gpt-4o".to_string();
    let openai_endpoint =
        ResolvedEndpoint::resolve_primary(&openai.api, &providers).expect("openai should resolve");

    let mut claude = CortexConfig::default();
    claude.api.provider = "anthropic".to_string();
    claude.api.model = "claude-sonnet-4-20250514".to_string();
    let claude_endpoint =
        ResolvedEndpoint::resolve_primary(&claude.api, &providers).expect("claude should resolve");

    assert_eq!(openai_endpoint.max_tokens, 16_384);
    assert_eq!(claude_endpoint.max_tokens, 8_192);
}
