use super::{
    ModelCapability, ModelFallbackReason, ModelHealth, ModelProfile, ModelRouteCandidate,
    ModelRouteIntent, ModelRouteRequest, ModelRouteTarget,
};

pub(super) fn score_candidate(
    profile: &ModelProfile,
    request: &ModelRouteRequest,
    required: &[ModelCapability],
    preferred: &[ModelCapability],
) -> ModelRouteCandidate {
    let missing_required = profile.missing_capabilities(required);
    let matched_preferred = profile.matched_capabilities(preferred);
    let mut score = 100.0;
    score += capability_score(required, profile, 32.0);
    score += capability_score(preferred, profile, 12.0);
    score -= usize_to_score(missing_required.len()) * 90.0;
    score += numeric_profile_score(profile, request);
    score += preference_score(profile, request);
    score -= health_penalty(profile.health);
    score -= failed_target_penalty(profile, request);

    let rejected = profile.health == ModelHealth::Failed
        || request
            .failed_targets
            .iter()
            .any(|target| same_route_target(target, &profile.target));
    let rationale = candidate_rationale(profile, &missing_required, &matched_preferred, score);
    ModelRouteCandidate {
        target: profile.target.clone(),
        score,
        missing_required,
        matched_preferred,
        rejected,
        rationale,
    }
}

fn numeric_profile_score(profile: &ModelProfile, request: &ModelRouteRequest) -> f32 {
    let mut score = 0.0;
    if request.estimated_input_tokens > 0 {
        if profile.context_tokens >= request.estimated_input_tokens {
            score += 12.0;
        } else {
            score -= 60.0;
        }
    }
    score += profile.safety_score.clamp(0.0, 1.0) * request.risk.clamp(0.0, 1.0) * 32.0;
    score += profile.reasoning_depth.clamp(0.0, 1.0) * (1.0 - request.confidence) * 28.0;
    score += profile.json_reliability.clamp(0.0, 1.0) * json_weight(request);
    score -= latency_penalty(profile, request);
    score -= cost_penalty(profile, request);
    score
}

fn preference_score(profile: &ModelProfile, request: &ModelRouteRequest) -> f32 {
    let mut score = 0.0;
    if request
        .preferred_group
        .as_ref()
        .is_some_and(|group| group == &profile.target.group)
    {
        score += 18.0;
    }
    if request.needs_escalation() {
        score += profile.safety_score.clamp(0.0, 1.0) * 30.0;
        score += profile.reasoning_depth.clamp(0.0, 1.0) * 30.0;
    }
    if request
        .fallback_reasons
        .contains(&ModelFallbackReason::SchemaInvalid)
    {
        score += profile.json_reliability.clamp(0.0, 1.0) * 35.0;
    }
    score
}

fn candidate_rationale(
    profile: &ModelProfile,
    missing_required: &[ModelCapability],
    matched_preferred: &[ModelCapability],
    score: f32,
) -> String {
    let matched = capability_labels(matched_preferred);
    let missing = capability_labels(missing_required);
    if missing_required.is_empty() {
        format!(
            "score={score:.1}; matched preferred [{}]; latency={}ms; cost={:.2}/{:.2}",
            matched,
            profile.latency_ms,
            profile.input_cost_per_million,
            profile.output_cost_per_million
        )
    } else {
        format!("score={score:.1}; missing required [{missing}]; matched preferred [{matched}]")
    }
}

pub(super) fn build_explanation(
    request: &ModelRouteRequest,
    selected: Option<&ModelRouteTarget>,
    candidates: &[ModelRouteCandidate],
) -> Vec<String> {
    let mut explanation = Vec::new();
    if let Some(target) = selected {
        explanation.push(format!("selected {}", target.display_name()));
    } else {
        explanation.push("no model route selected".to_string());
    }
    if request.needs_escalation() {
        explanation
            .push("low confidence and high risk required high_safety + deep_reasoning".to_string());
    }
    if !request.fallback_reasons.is_empty() {
        explanation.push(format!(
            "fallback reasons: {}",
            fallback_reason_labels(&request.fallback_reasons)
        ));
    }
    if let Some(best) = candidates.first() {
        explanation.push(format!("top candidate: {}", best.rationale));
    }
    explanation
}

pub(super) fn compare_candidates(
    left: &ModelRouteCandidate,
    right: &ModelRouteCandidate,
) -> std::cmp::Ordering {
    right
        .score
        .total_cmp(&left.score)
        .then_with(|| left.target.group.cmp(&right.target.group))
}

fn capability_score(capabilities: &[ModelCapability], profile: &ModelProfile, weight: f32) -> f32 {
    usize_to_score(profile.matched_capabilities(capabilities).len()) * weight
}

fn latency_penalty(profile: &ModelProfile, request: &ModelRouteRequest) -> f32 {
    let latency = f32::from(u16::try_from(profile.latency_ms).unwrap_or(u16::MAX));
    let divisor = if matches!(
        request.intent,
        ModelRouteIntent::Extraction | ModelRouteIntent::Summarization
    ) {
        500.0
    } else {
        1_500.0
    };
    latency / divisor
}

fn cost_penalty(profile: &ModelProfile, request: &ModelRouteRequest) -> f32 {
    let combined = profile.input_cost_per_million + profile.output_cost_per_million;
    let divisor = if matches!(
        request.intent,
        ModelRouteIntent::Extraction | ModelRouteIntent::Summarization
    ) {
        2.0
    } else {
        6.0
    };
    combined / divisor
}

const fn json_weight(request: &ModelRouteRequest) -> f32 {
    match request.intent {
        ModelRouteIntent::Extraction
        | ModelRouteIntent::Retrieval
        | ModelRouteIntent::SafetyReview
        | ModelRouteIntent::Evaluation => 20.0,
        _ => 8.0,
    }
}

const fn health_penalty(health: ModelHealth) -> f32 {
    match health {
        ModelHealth::Healthy => 0.0,
        ModelHealth::Degraded => 35.0,
        ModelHealth::Failed => 1_000.0,
    }
}

fn failed_target_penalty(profile: &ModelProfile, request: &ModelRouteRequest) -> f32 {
    if request
        .failed_targets
        .iter()
        .any(|target| same_route_target(target, &profile.target))
    {
        1_000.0
    } else {
        0.0
    }
}

fn same_route_target(left: &ModelRouteTarget, right: &ModelRouteTarget) -> bool {
    left.group == right.group && left.provider == right.provider && left.model == right.model
}

fn capability_labels(capabilities: &[ModelCapability]) -> String {
    capabilities
        .iter()
        .map(|capability| capability.label())
        .collect::<Vec<_>>()
        .join(", ")
}

fn fallback_reason_labels(reasons: &[ModelFallbackReason]) -> String {
    reasons
        .iter()
        .map(|reason| match reason {
            ModelFallbackReason::ProviderFailure => "provider_failure",
            ModelFallbackReason::SchemaInvalid => "schema_invalid",
            ModelFallbackReason::LowConfidence => "low_confidence",
            ModelFallbackReason::MissingCapability => "missing_capability",
            ModelFallbackReason::HealthDegraded => "health_degraded",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn usize_to_score(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}
