use serde::{Deserialize, Serialize};

use crate::config::{
    CortexConfig, ProviderProtocol, ProviderRegistry, resolved_model_token_limits,
};

mod scoring;

const HIGH_RISK_THRESHOLD: f32 = 0.70;
const LOW_CONFIDENCE_THRESHOLD: f32 = 0.45;
const DEFAULT_CONTEXT_TOKENS: usize = 128_000;
const DEFAULT_OUTPUT_TOKENS: usize = 8_192;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelCapability {
    Coding,
    LongContext,
    Vision,
    ToolCalling,
    JsonReliability,
    LowLatency,
    LowCost,
    HighSafety,
    DeepReasoning,
}

impl ModelCapability {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Coding => "coding",
            Self::LongContext => "long_context",
            Self::Vision => "vision",
            Self::ToolCalling => "tool_calling",
            Self::JsonReliability => "json_reliability",
            Self::LowLatency => "low_latency",
            Self::LowCost => "low_cost",
            Self::HighSafety => "high_safety",
            Self::DeepReasoning => "deep_reasoning",
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteIntent {
    #[default]
    Conversation,
    Extraction,
    Summarization,
    Retrieval,
    Coding,
    Vision,
    ToolUse,
    SafetyReview,
    Evaluation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelHealth {
    #[default]
    Healthy,
    Degraded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFallbackReason {
    ProviderFailure,
    SchemaInvalid,
    LowConfidence,
    MissingCapability,
    HealthDegraded,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelRouteTarget {
    pub group: String,
    pub provider: String,
    pub model: String,
}

impl Default for ModelRouteTarget {
    fn default() -> Self {
        Self {
            group: "primary".to_string(),
            provider: String::new(),
            model: String::new(),
        }
    }
}

impl ModelRouteTarget {
    #[must_use]
    pub fn new(
        group: impl Into<String>,
        provider: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            group: group.into(),
            provider: provider.into(),
            model: model.into(),
        }
    }

    #[must_use]
    pub fn display_name(&self) -> String {
        format!("{}:{}/{}", self.group, self.provider, self.model)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelProfile {
    pub target: ModelRouteTarget,
    pub capabilities: Vec<ModelCapability>,
    pub context_tokens: usize,
    pub output_tokens: usize,
    pub latency_ms: u32,
    pub input_cost_per_million: f32,
    pub output_cost_per_million: f32,
    pub safety_score: f32,
    pub reasoning_depth: f32,
    pub json_reliability: f32,
    pub health: ModelHealth,
}

impl Default for ModelProfile {
    fn default() -> Self {
        Self {
            target: ModelRouteTarget::default(),
            capabilities: Vec::new(),
            context_tokens: DEFAULT_CONTEXT_TOKENS,
            output_tokens: DEFAULT_OUTPUT_TOKENS,
            latency_ms: 2_000,
            input_cost_per_million: 1.0,
            output_cost_per_million: 3.0,
            safety_score: 0.65,
            reasoning_depth: 0.50,
            json_reliability: 0.70,
            health: ModelHealth::Healthy,
        }
    }
}

impl ModelProfile {
    #[must_use]
    pub fn new(target: ModelRouteTarget) -> Self {
        Self {
            target,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn supports(&self, capability: ModelCapability) -> bool {
        self.capabilities.contains(&capability)
    }

    pub fn add_capability(&mut self, capability: ModelCapability) {
        if !self.supports(capability) {
            self.capabilities.push(capability);
        }
    }

    #[must_use]
    pub fn matched_capabilities(&self, capabilities: &[ModelCapability]) -> Vec<ModelCapability> {
        capabilities
            .iter()
            .copied()
            .filter(|capability| self.supports(*capability))
            .collect()
    }

    #[must_use]
    pub fn missing_capabilities(&self, capabilities: &[ModelCapability]) -> Vec<ModelCapability> {
        capabilities
            .iter()
            .copied()
            .filter(|capability| !self.supports(*capability))
            .collect()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ModelRouteRequest {
    pub intent: ModelRouteIntent,
    pub required_capabilities: Vec<ModelCapability>,
    pub preferred_capabilities: Vec<ModelCapability>,
    pub preferred_group: Option<String>,
    pub failed_targets: Vec<ModelRouteTarget>,
    pub estimated_input_tokens: usize,
    pub risk: f32,
    pub confidence: f32,
    pub fallback_reasons: Vec<ModelFallbackReason>,
}

impl Default for ModelRouteRequest {
    fn default() -> Self {
        Self {
            intent: ModelRouteIntent::Conversation,
            required_capabilities: Vec::new(),
            preferred_capabilities: Vec::new(),
            preferred_group: None,
            failed_targets: Vec::new(),
            estimated_input_tokens: 0,
            risk: 0.0,
            confidence: 0.80,
            fallback_reasons: Vec::new(),
        }
    }
}

impl ModelRouteRequest {
    #[must_use]
    pub fn new(intent: ModelRouteIntent) -> Self {
        Self {
            intent,
            ..Self::default()
        }
        .with_intent_defaults()
    }

    #[must_use]
    pub fn for_endpoint(endpoint_name: &str) -> Self {
        match endpoint_name {
            "memory_extract" | "entity_extract" => {
                Self::new(ModelRouteIntent::Extraction).prefer_group("light")
            }
            "compress" | "summary" => {
                Self::new(ModelRouteIntent::Summarization).prefer_group("light")
            }
            "self_update" | "causal_analyze" => {
                Self::new(ModelRouteIntent::SafetyReview).prefer_group("medium")
            }
            "autonomous" => Self::new(ModelRouteIntent::ToolUse).prefer_group("medium"),
            _ => Self::new(ModelRouteIntent::Conversation),
        }
    }

    #[must_use]
    pub fn require(mut self, capability: ModelCapability) -> Self {
        push_unique(&mut self.required_capabilities, capability);
        self
    }

    #[must_use]
    pub fn prefer(mut self, capability: ModelCapability) -> Self {
        push_unique(&mut self.preferred_capabilities, capability);
        self
    }

    #[must_use]
    pub fn prefer_group(mut self, group: impl Into<String>) -> Self {
        self.preferred_group = Some(group.into());
        self
    }

    #[must_use]
    pub const fn with_risk(mut self, risk: f32) -> Self {
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_confidence(mut self, confidence: f32) -> Self {
        self.confidence = confidence.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_estimated_input_tokens(mut self, tokens: usize) -> Self {
        self.estimated_input_tokens = tokens;
        self
    }

    #[must_use]
    pub fn after_failure(mut self, target: ModelRouteTarget, reason: ModelFallbackReason) -> Self {
        self.failed_targets.push(target);
        push_unique(&mut self.fallback_reasons, reason);
        self
    }

    #[must_use]
    pub fn with_fallback_reason(mut self, reason: ModelFallbackReason) -> Self {
        push_unique(&mut self.fallback_reasons, reason);
        self
    }

    #[must_use]
    pub fn needs_escalation(&self) -> bool {
        self.risk >= HIGH_RISK_THRESHOLD
            && (self.confidence <= LOW_CONFIDENCE_THRESHOLD
                || self
                    .fallback_reasons
                    .contains(&ModelFallbackReason::LowConfidence))
    }

    #[must_use]
    pub fn effective_required_capabilities(&self) -> Vec<ModelCapability> {
        let mut capabilities = self.required_capabilities.clone();
        if self.intent == ModelRouteIntent::Vision {
            push_unique(&mut capabilities, ModelCapability::Vision);
        }
        if self.intent == ModelRouteIntent::ToolUse {
            push_unique(&mut capabilities, ModelCapability::ToolCalling);
        }
        if self
            .fallback_reasons
            .contains(&ModelFallbackReason::SchemaInvalid)
        {
            push_unique(&mut capabilities, ModelCapability::JsonReliability);
        }
        if self.needs_escalation() {
            push_unique(&mut capabilities, ModelCapability::HighSafety);
            push_unique(&mut capabilities, ModelCapability::DeepReasoning);
        }
        capabilities
    }

    #[must_use]
    pub fn effective_preferred_capabilities(&self) -> Vec<ModelCapability> {
        let mut capabilities = self.preferred_capabilities.clone();
        match self.intent {
            ModelRouteIntent::Conversation => {
                push_unique(&mut capabilities, ModelCapability::ToolCalling);
                push_unique(&mut capabilities, ModelCapability::DeepReasoning);
            }
            ModelRouteIntent::Extraction => {
                push_unique(&mut capabilities, ModelCapability::JsonReliability);
                push_unique(&mut capabilities, ModelCapability::LowCost);
                push_unique(&mut capabilities, ModelCapability::LowLatency);
            }
            ModelRouteIntent::Summarization | ModelRouteIntent::Retrieval => {
                push_unique(&mut capabilities, ModelCapability::LongContext);
                push_unique(&mut capabilities, ModelCapability::JsonReliability);
                push_unique(&mut capabilities, ModelCapability::LowCost);
            }
            ModelRouteIntent::Coding => {
                push_unique(&mut capabilities, ModelCapability::Coding);
                push_unique(&mut capabilities, ModelCapability::DeepReasoning);
                push_unique(&mut capabilities, ModelCapability::ToolCalling);
            }
            ModelRouteIntent::Vision => {
                push_unique(&mut capabilities, ModelCapability::Vision);
            }
            ModelRouteIntent::ToolUse => {
                push_unique(&mut capabilities, ModelCapability::ToolCalling);
                push_unique(&mut capabilities, ModelCapability::HighSafety);
            }
            ModelRouteIntent::SafetyReview | ModelRouteIntent::Evaluation => {
                push_unique(&mut capabilities, ModelCapability::HighSafety);
                push_unique(&mut capabilities, ModelCapability::DeepReasoning);
                push_unique(&mut capabilities, ModelCapability::JsonReliability);
            }
        }
        capabilities
    }

    fn with_intent_defaults(mut self) -> Self {
        if self.intent == ModelRouteIntent::Extraction {
            push_unique(
                &mut self.required_capabilities,
                ModelCapability::JsonReliability,
            );
        }
        self
    }
}

#[derive(Debug, Clone)]
pub struct ModelRouteCandidate {
    pub target: ModelRouteTarget,
    pub score: f32,
    pub missing_required: Vec<ModelCapability>,
    pub matched_preferred: Vec<ModelCapability>,
    pub rejected: bool,
    pub rationale: String,
}

#[derive(Debug, Clone)]
pub struct ModelRouteDecision {
    pub selected: Option<ModelRouteTarget>,
    pub candidates: Vec<ModelRouteCandidate>,
    pub explanation: Vec<String>,
    pub escalated: bool,
    pub fallback: bool,
}

impl ModelRouteDecision {
    #[must_use]
    pub fn selected_group(&self) -> Option<&str> {
        self.selected.as_ref().map(|target| target.group.as_str())
    }
}

#[derive(Debug, Clone, Default)]
pub struct ModelCapabilityRegistry {
    pub profiles: Vec<ModelProfile>,
}

impl ModelCapabilityRegistry {
    #[must_use]
    pub const fn new(profiles: Vec<ModelProfile>) -> Self {
        Self { profiles }
    }

    #[must_use]
    pub fn from_config(config: &CortexConfig, providers: &ProviderRegistry) -> Self {
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

        Self { profiles }
    }

    #[must_use]
    pub fn route(&self, request: &ModelRouteRequest) -> ModelRouteDecision {
        let required = request.effective_required_capabilities();
        let preferred = request.effective_preferred_capabilities();
        let mut candidates: Vec<ModelRouteCandidate> = self
            .profiles
            .iter()
            .map(|profile| scoring::score_candidate(profile, request, &required, &preferred))
            .collect();
        candidates.sort_by(scoring::compare_candidates);
        let selected = candidates
            .iter()
            .find(|candidate| !candidate.rejected)
            .or_else(|| candidates.first())
            .map(|candidate| candidate.target.clone());
        let explanation = scoring::build_explanation(request, selected.as_ref(), &candidates);
        let fallback = request
            .preferred_group
            .as_ref()
            .is_some_and(|preferred_group| {
                !matches!(
                    selected.as_ref(),
                    Some(target) if target.group == *preferred_group
                )
            })
            || !request.fallback_reasons.is_empty();
        ModelRouteDecision {
            selected,
            candidates,
            explanation,
            escalated: request.needs_escalation(),
            fallback,
        }
    }
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
    group: &crate::config::LlmGroupConfig,
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

fn push_unique<T>(items: &mut Vec<T>, item: T)
where
    T: Copy + Eq,
{
    if !items.contains(&item) {
        items.push(item);
    }
}
