use serde::{Deserialize, Serialize};

use crate::config::{CortexConfig, ProviderRegistry};

mod profiles;
mod scoring;

const HIGH_RISK_THRESHOLD: f32 = 0.70;
const LOW_CONFIDENCE_THRESHOLD: f32 = 0.45;
pub(super) const DEFAULT_CONTEXT_TOKENS: usize = 128_000;
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
        Self {
            profiles: profiles::from_config(config, providers),
        }
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

pub(super) fn push_unique<T>(items: &mut Vec<T>, item: T)
where
    T: Copy + Eq,
{
    if !items.contains(&item) {
        items.push(item);
    }
}
