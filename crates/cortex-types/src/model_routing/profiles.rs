use crate::config::{
    CortexConfig, LlmGroupConfig, ProviderProtocol, ProviderRegistry, resolved_model_token_limits,
};

use super::{DEFAULT_CONTEXT_TOKENS, ModelCapability, ModelProfile, ModelRouteTarget, push_unique};

pub(super) fn from_config(
    config: &CortexConfig,
    providers: &ProviderRegistry,
) -> Vec<ModelProfile> {
    let mut profiles = Vec::new();
    let mut group_names: Vec<&String> = config.llm_groups.keys().collect();
    group_names.sort();

    for group_name in group_names {
        if let Some(group) = config.llm_groups.get(group_name)
            && let Some(profile) = profile_from_group(config, providers, group_name, group)
        {
            profiles.push(profile);
        }
    }

    if profiles.is_empty()
        && let Some(profile) = profile_from_primary(config, providers)
    {
        profiles.push(profile);
    }

    profiles
}

fn profile_from_primary(
    config: &CortexConfig,
    providers: &ProviderRegistry,
) -> Option<ModelProfile> {
    let provider = providers.get(&config.api.provider)?;
    let model = first_non_empty_model(&config.api.model, provider.models.first());
    let mut profile = ModelProfile::new(ModelRouteTarget::new(
        "primary",
        config.api.provider.clone(),
        model.clone(),
    ));
    let limits = resolved_model_token_limits(
        &config.api.provider,
        &provider.protocol,
        &model,
        config.context.max_tokens,
        config.api.max_tokens,
    );
    profile.context_tokens = limits.context_tokens;
    profile.output_tokens = limits.output_tokens;
    fill_inferred_profile(&mut profile, &provider.protocol, &[]);
    Some(profile)
}

fn profile_from_group(
    config: &CortexConfig,
    providers: &ProviderRegistry,
    group_name: &str,
    group: &LlmGroupConfig,
) -> Option<ModelProfile> {
    let provider_name = if group.provider.is_empty() {
        &config.api.provider
    } else {
        &group.provider
    };
    let provider = providers.get(provider_name)?;
    let model = first_non_empty_model(
        if group.model.is_empty() {
            &config.api.model
        } else {
            &group.model
        },
        provider.models.first(),
    );
    let mut profile = ModelProfile::new(ModelRouteTarget::new(group_name, provider_name, model));
    profile.capabilities.clone_from(&group.capabilities);
    let output_tokens = if group.output_tokens > 0 {
        group.output_tokens
    } else if group.max_tokens > 0 {
        group.max_tokens
    } else {
        config.api.max_tokens
    };
    let limits = resolved_model_token_limits(
        provider_name,
        &provider.protocol,
        &profile.target.model,
        positive_or(group.context_tokens, config.context.max_tokens),
        output_tokens,
    );
    profile.context_tokens = limits.context_tokens;
    profile.output_tokens = limits.output_tokens;
    profile.latency_ms = positive_or_u32(group.latency_ms, inferred_latency_ms(group_name));
    profile.input_cost_per_million = positive_or_f32(
        group.input_cost_per_million,
        inferred_input_cost_per_million(group_name),
    );
    profile.output_cost_per_million = positive_or_f32(
        group.output_cost_per_million,
        inferred_output_cost_per_million(group_name),
    );
    profile.safety_score = positive_or_f32(
        group.safety_score,
        inferred_safety_score(group_name, &profile.target),
    );
    profile.reasoning_depth = positive_or_f32(
        group.reasoning_depth,
        inferred_reasoning_depth(group_name, &profile.target.model),
    );
    profile.json_reliability = positive_or_f32(
        group.json_reliability,
        inferred_json_reliability(&provider.protocol),
    );
    fill_inferred_profile(&mut profile, &provider.protocol, &group.capabilities);
    Some(profile)
}

fn fill_inferred_profile(
    profile: &mut ModelProfile,
    protocol: &ProviderProtocol,
    explicit: &[ModelCapability],
) {
    for capability in inferred_capabilities(profile, protocol) {
        profile.add_capability(capability);
    }
    for capability in explicit {
        profile.add_capability(*capability);
    }
}

fn inferred_capabilities(
    profile: &ModelProfile,
    protocol: &ProviderProtocol,
) -> Vec<ModelCapability> {
    let mut capabilities = Vec::new();
    let model = profile.target.model.to_lowercase();
    let group = profile.target.group.to_lowercase();
    if profile.context_tokens >= DEFAULT_CONTEXT_TOKENS || model.contains("long") {
        push_unique(&mut capabilities, ModelCapability::LongContext);
    }
    if is_vision_model_name(&model) {
        push_unique(&mut capabilities, ModelCapability::Vision);
    }
    if matches!(
        protocol,
        &ProviderProtocol::Anthropic | &ProviderProtocol::OpenAI
    ) {
        push_unique(&mut capabilities, ModelCapability::ToolCalling);
        push_unique(&mut capabilities, ModelCapability::JsonReliability);
    }
    if model.contains("code") || model.contains("coder") || group.contains("code") {
        push_unique(&mut capabilities, ModelCapability::Coding);
    }
    if profile.latency_ms <= 1_200 || group.contains("light") {
        push_unique(&mut capabilities, ModelCapability::LowLatency);
    }
    if profile.input_cost_per_million <= 0.50 || group.contains("light") {
        push_unique(&mut capabilities, ModelCapability::LowCost);
    }
    if profile.safety_score >= 0.75 {
        push_unique(&mut capabilities, ModelCapability::HighSafety);
    }
    if profile.reasoning_depth >= 0.70 || group.contains("heavy") {
        push_unique(&mut capabilities, ModelCapability::DeepReasoning);
    }
    if profile.json_reliability >= 0.75 {
        push_unique(&mut capabilities, ModelCapability::JsonReliability);
    }
    capabilities
}

fn first_non_empty_model(configured: &str, provider_first: Option<&String>) -> String {
    if configured.is_empty() {
        provider_first.cloned().unwrap_or_default()
    } else {
        configured.to_string()
    }
}

fn is_vision_model_name(model: &str) -> bool {
    ["vision", "vl", "4v", "image", "multimodal", "omni"]
        .iter()
        .any(|marker| model.contains(marker))
}

fn inferred_latency_ms(group_name: &str) -> u32 {
    match group_name {
        "light" => 800,
        "medium" => 1_800,
        "heavy" => 3_200,
        _ => 2_000,
    }
}

fn inferred_input_cost_per_million(group_name: &str) -> f32 {
    match group_name {
        "light" => 0.20,
        "medium" => 0.80,
        "heavy" => 2.50,
        _ => 1.00,
    }
}

fn inferred_output_cost_per_million(group_name: &str) -> f32 {
    match group_name {
        "light" => 0.80,
        "heavy" => 10.00,
        _ => 3.00,
    }
}

fn inferred_safety_score(group_name: &str, target: &ModelRouteTarget) -> f32 {
    let model = target.model.to_lowercase();
    if group_name == "heavy"
        || model.contains("claude")
        || model.contains("gpt-5")
        || model.contains("safety")
    {
        0.85
    } else if group_name == "medium" {
        0.75
    } else {
        0.62
    }
}

fn inferred_reasoning_depth(group_name: &str, model: &str) -> f32 {
    let model = model.to_lowercase();
    if group_name == "heavy"
        || model.contains("reason")
        || model.contains("thinking")
        || model.contains("opus")
    {
        0.90
    } else if group_name == "medium" {
        0.65
    } else {
        0.35
    }
}

const fn inferred_json_reliability(protocol: &ProviderProtocol) -> f32 {
    match protocol {
        &ProviderProtocol::Anthropic | &ProviderProtocol::OpenAI => 0.80,
        &ProviderProtocol::Ollama => 0.55,
    }
}

const fn positive_or(value: usize, fallback: usize) -> usize {
    if value == 0 { fallback } else { value }
}

const fn positive_or_u32(value: u32, fallback: u32) -> u32 {
    if value == 0 { fallback } else { value }
}

fn positive_or_f32(value: f32, fallback: f32) -> f32 {
    if value <= 0.0 {
        fallback
    } else {
        value.clamp(0.0, 1_000_000.0)
    }
}
