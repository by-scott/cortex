use std::collections::BTreeMap;

const DEFAULT_TIMELINE_LIMIT: usize = 80;
const MAX_TIMELINE_LIMIT: usize = 500;

pub fn timeline_limit(requested: usize) -> usize {
    if requested == 0 {
        DEFAULT_TIMELINE_LIMIT
    } else {
        requested.min(MAX_TIMELINE_LIMIT)
    }
}

pub fn model_profiles_json(profiles: &[cortex_types::ModelProfile]) -> Vec<serde_json::Value> {
    profiles
        .iter()
        .map(|profile| {
            serde_json::json!({
                "group": &profile.target.group,
                "provider": &profile.target.provider,
                "model": &profile.target.model,
                "capabilities": profile
                    .capabilities
                    .iter()
                    .map(|capability| capability.label())
                    .collect::<Vec<_>>(),
                "context_tokens": profile.context_tokens,
                "output_tokens": profile.output_tokens,
                "latency_ms": profile.latency_ms,
                "input_cost_per_million": profile.input_cost_per_million,
                "output_cost_per_million": profile.output_cost_per_million,
                "safety_score": profile.safety_score,
                "reasoning_depth": profile.reasoning_depth,
                "json_reliability": profile.json_reliability,
                "health": format!("{:?}", profile.health).to_lowercase(),
            })
        })
        .collect()
}

pub fn timeline_counts(events: &[cortex_kernel::StoredEvent]) -> BTreeMap<&'static str, u64> {
    let mut counts = BTreeMap::new();
    for event in events {
        let category = timeline_category(&event.payload);
        let count = counts.entry(category).or_insert(0);
        *count += 1;
    }
    counts
}

pub fn timeline_entry(event: &cortex_kernel::StoredEvent) -> serde_json::Value {
    let (category, label, details) = timeline_payload(&event.payload);
    serde_json::json!({
        "offset": event.offset,
        "event_id": &event.event_id,
        "turn_id": &event.turn_id,
        "correlation_id": &event.correlation_id,
        "timestamp": event.timestamp.to_rfc3339(),
        "event_type": &event.event_type,
        "category": category,
        "label": label,
        "details": details,
        "execution_version": &event.execution_version,
    })
}

const fn timeline_category(payload: &cortex_types::Payload) -> &'static str {
    match payload {
        cortex_types::Payload::TurnStarted
        | cortex_types::Payload::TurnCompleted
        | cortex_types::Payload::TurnInterrupted
        | cortex_types::Payload::SessionStarted { .. }
        | cortex_types::Payload::SessionEnded { .. } => "lifecycle",
        cortex_types::Payload::UserMessage { .. }
        | cortex_types::Payload::AssistantMessage { .. } => "message",
        cortex_types::Payload::LlmCallCompleted { .. } => "llm",
        cortex_types::Payload::ToolInvocationIntent { .. }
        | cortex_types::Payload::ToolInvocationResult { .. }
        | cortex_types::Payload::ToolEffectPreviewed { .. }
        | cortex_types::Payload::ToolEffectVerified { .. }
        | cortex_types::Payload::ToolEffectCommitted { .. } => "tool",
        cortex_types::Payload::PermissionRequested { .. }
        | cortex_types::Payload::PermissionGranted { .. }
        | cortex_types::Payload::PermissionDenied { .. } => "permission",
        cortex_types::Payload::WorkspaceFrameAssembled { .. }
        | cortex_types::Payload::WorkspaceItemPromoted { .. }
        | cortex_types::Payload::ContextPressureObserved { .. }
        | cortex_types::Payload::ContextCompacted { .. }
        | cortex_types::Payload::ContextCompactBoundary { .. } => "workspace",
        cortex_types::Payload::RetrievalDecisionRecorded { .. }
        | cortex_types::Payload::EvidenceRetrieved { .. }
        | cortex_types::Payload::EvidencePromoted { .. } => "retrieval",
        cortex_types::Payload::MemoryCaptured { .. }
        | cortex_types::Payload::MemoryMaterialized { .. }
        | cortex_types::Payload::MemoryStabilized { .. }
        | cortex_types::Payload::MemorySplit { .. }
        | cortex_types::Payload::MemoryGraphHealthAssessed { .. }
        | cortex_types::Payload::MemoryRelationReorganized { .. } => "memory",
        cortex_types::Payload::ImpasseDetected { .. }
        | cortex_types::Payload::ConflictDetected { .. }
        | cortex_types::Payload::MetaControlApplied { .. }
        | cortex_types::Payload::FrameCheckResult { .. }
        | cortex_types::Payload::ControlDecisionRecorded { .. }
        | cortex_types::Payload::ImpasseRecorded { .. }
        | cortex_types::Payload::ConfidenceAssessed { .. }
        | cortex_types::Payload::ConfidenceLow { .. }
        | cortex_types::Payload::PressureResponseApplied { .. } => "control",
        cortex_types::Payload::GuardrailTriggered { .. }
        | cortex_types::Payload::ExternalInputObserved { .. } => "guardrail",
        cortex_types::Payload::WorkingMemoryItemActivated { .. }
        | cortex_types::Payload::WorkingMemoryItemRehearsed { .. }
        | cortex_types::Payload::WorkingMemoryItemEvicted { .. }
        | cortex_types::Payload::WorkingMemoryCapacityExceeded { .. }
        | cortex_types::Payload::ChannelScheduled { .. }
        | cortex_types::Payload::MaintenanceExecuted { .. }
        | cortex_types::Payload::EmergencyTriggered { .. } => "scheduler",
        _ => "other",
    }
}

type TimelinePayload = (&'static str, &'static str, serde_json::Value);

fn timeline_payload(payload: &cortex_types::Payload) -> TimelinePayload {
    if let Some(entry) = lifecycle_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = llm_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = tool_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = workspace_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = retrieval_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = memory_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = control_timeline_payload(payload) {
        return entry;
    }
    if let Some(entry) = guardrail_timeline_payload(payload) {
        return entry;
    }
    (
        timeline_category(payload),
        payload_label(payload),
        serde_json::to_value(payload).unwrap_or_default(),
    )
}

fn lifecycle_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::TurnStarted => {
            Some(("lifecycle", "turn_started", serde_json::json!({})))
        }
        cortex_types::Payload::TurnCompleted => {
            Some(("lifecycle", "turn_completed", serde_json::json!({})))
        }
        cortex_types::Payload::TurnInterrupted => {
            Some(("lifecycle", "turn_interrupted", serde_json::json!({})))
        }
        cortex_types::Payload::SessionStarted { session_id } => Some((
            "lifecycle",
            "session_started",
            serde_json::json!({ "session_id": session_id }),
        )),
        cortex_types::Payload::SessionEnded { session_id } => Some((
            "lifecycle",
            "session_ended",
            serde_json::json!({ "session_id": session_id }),
        )),
        cortex_types::Payload::UserMessage { content } => Some(message_timeline("user", content)),
        cortex_types::Payload::AssistantMessage { content } => {
            Some(message_timeline("assistant", content))
        }
        _ => None,
    }
}

fn llm_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::LlmCallCompleted {
            input_tokens,
            output_tokens,
            cache_read_input_tokens,
            cache_creation_input_tokens,
            model,
            estimated_cost_usd,
        } => Some((
            "llm",
            "llm_call_completed",
            serde_json::json!({
                "model": model,
                "input_tokens": input_tokens,
                "output_tokens": output_tokens,
                "total_tokens": input_tokens + output_tokens,
                "cache_read_input_tokens": cache_read_input_tokens,
                "cache_creation_input_tokens": cache_creation_input_tokens,
                "estimated_cost_usd": estimated_cost_usd,
            }),
        )),
        _ => None,
    }
}

fn tool_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::ToolInvocationIntent { tool_name, input } => Some((
            "tool",
            "tool_intent",
            serde_json::json!({ "tool": tool_name, "input": preview_text(input, 240) }),
        )),
        cortex_types::Payload::ToolInvocationResult {
            tool_name,
            output,
            is_error,
        } => Some((
            "tool",
            "tool_result",
            serde_json::json!({
                "tool": tool_name,
                "is_error": is_error,
                "output": preview_text(output, 240),
            }),
        )),
        cortex_types::Payload::ToolEffectPreviewed {
            tool_name,
            effects,
            preview,
            rollback,
        } => Some((
            "tool",
            "tool_effect_previewed",
            serde_json::json!({
                "tool": tool_name,
                "effects": effects,
                "preview": preview_text(preview, 240),
                "rollback": rollback,
            }),
        )),
        cortex_types::Payload::ToolEffectVerified {
            tool_name,
            success,
            verification,
        } => Some((
            "tool",
            "tool_effect_verified",
            serde_json::json!({
                "tool": tool_name,
                "success": success,
                "verification": preview_text(verification, 240),
            }),
        )),
        cortex_types::Payload::ToolEffectCommitted { tool_name, receipt } => Some((
            "tool",
            "tool_effect_committed",
            serde_json::json!({ "tool": tool_name, "receipt": receipt }),
        )),
        _ => None,
    }
}

fn workspace_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::WorkspaceFrameAssembled { frame } => Some((
            "workspace",
            "workspace_frame_assembled",
            serde_json::json!({
                "actor": &frame.actor,
                "session_id": &frame.session_id,
                "items": frame.items.len(),
                "max_items": frame.budget.max_items,
                "max_input_tokens": frame.budget.max_input_tokens,
            }),
        )),
        cortex_types::Payload::WorkspaceItemPromoted { item } => Some((
            "workspace",
            "workspace_item_promoted",
            serde_json::json!({
                "id": &item.id,
                "kind": format!("{:?}", item.kind),
                "lane": item.lane.map(|lane| format!("{lane:?}")),
                "taint": format!("{:?}", item.taint),
                "utility": item.utility,
                "risk": item.risk,
                "reason": &item.promotion_reason,
            }),
        )),
        _ => None,
    }
}

fn retrieval_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::RetrievalDecisionRecorded { decision } => Some((
            "retrieval",
            "retrieval_decision",
            serde_json::json!({
                "kind": format!("{:?}", decision.kind),
                "query": preview_text(&decision.query_plan.query, 180),
                "support": decision.support,
                "rationale": preview_text(&decision.rationale, 240),
            }),
        )),
        cortex_types::Payload::EvidenceRetrieved { evidence } => Some((
            "retrieval",
            "evidence_retrieved",
            serde_json::json!({
                "evidence_id": &evidence.id,
                "corpus_id": &evidence.corpus_id,
                "role": format!("{:?}", evidence.role),
                "taint": format!("{:?}", evidence.taint),
                "score": evidence.scores.hybrid(),
                "source_uri": &evidence.source_uri,
            }),
        )),
        cortex_types::Payload::EvidencePromoted {
            evidence_id,
            frame_item_id,
        } => Some((
            "retrieval",
            "evidence_promoted",
            serde_json::json!({
                "evidence_id": evidence_id,
                "frame_item_id": frame_item_id,
            }),
        )),
        _ => None,
    }
}

fn memory_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::MemoryCaptured {
            memory_id,
            memory_type,
        } => Some((
            "memory",
            "memory_captured",
            serde_json::json!({ "memory_id": memory_id, "memory_type": memory_type }),
        )),
        cortex_types::Payload::MemoryMaterialized { memory_id } => Some((
            "memory",
            "memory_materialized",
            serde_json::json!({ "memory_id": memory_id }),
        )),
        cortex_types::Payload::MemoryStabilized { memory_id } => Some((
            "memory",
            "memory_stabilized",
            serde_json::json!({ "memory_id": memory_id }),
        )),
        _ => None,
    }
}

fn control_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::ControlDecisionRecorded { decision } => Some((
            "control",
            "control_decision_recorded",
            serde_json::json!({
                "signal": format!("{:?}", decision.signal),
                "confidence": decision.confidence,
                "expected_benefit": decision.expected_benefit,
                "expected_cost": decision.expected_cost,
                "risk": decision.risk,
                "expected_value": decision.expected_value(),
                "reversibility": decision.reversibility.map(|item| format!("{item:?}")),
                "candidate_actions": decision.candidate_actions.len(),
                "rejected_alternatives": decision.rejected_alternatives.len(),
                "required_evidence": &decision.required_evidence,
                "blocking_uncertainty": preview_text(&decision.blocking_uncertainty, 240),
                "risk_boundary": preview_text(&decision.risk_boundary, 240),
                "fallback_plan": preview_text(&decision.fallback_plan, 240),
                "rationale": preview_text(&decision.rationale, 240),
            }),
        )),
        cortex_types::Payload::ImpasseRecorded { impasse } => Some((
            "control",
            "impasse_recorded",
            serde_json::json!({
                "id": &impasse.id,
                "kind": format!("{:?}", impasse.kind),
                "owner_actor": &impasse.owner_actor,
                "session_id": &impasse.session_id,
                "conflicts": impasse.conflicts.len(),
                "resolved": impasse.is_resolved(),
                "summary": preview_text(&impasse.summary, 240),
            }),
        )),
        _ => None,
    }
}

fn guardrail_timeline_payload(payload: &cortex_types::Payload) -> Option<TimelinePayload> {
    match payload {
        cortex_types::Payload::GuardrailTriggered {
            category,
            reason,
            source,
        } => Some((
            "guardrail",
            "guardrail_triggered",
            serde_json::json!({
                "category": category,
                "reason": reason,
                "source": source,
            }),
        )),
        cortex_types::Payload::ExternalInputObserved {
            source,
            trust,
            summary,
        } => Some((
            "guardrail",
            "external_input_observed",
            serde_json::json!({
                "source": source,
                "trust": trust,
                "summary": preview_text(summary, 240),
            }),
        )),
        _ => None,
    }
}

fn message_timeline(
    role: &'static str,
    content: &str,
) -> (&'static str, &'static str, serde_json::Value) {
    (
        "message",
        role,
        serde_json::json!({
            "role": role,
            "chars": content.chars().count(),
            "preview": preview_text(content, 240),
        }),
    )
}

const fn payload_label(payload: &cortex_types::Payload) -> &'static str {
    match payload {
        cortex_types::Payload::PermissionRequested { .. } => "permission_requested",
        cortex_types::Payload::PermissionGranted { .. } => "permission_granted",
        cortex_types::Payload::PermissionDenied { .. } => "permission_denied",
        cortex_types::Payload::ContextPressureObserved { .. } => "context_pressure_observed",
        cortex_types::Payload::ContextCompacted { .. } => "context_compacted",
        cortex_types::Payload::ContextCompactBoundary { .. } => "context_compact_boundary",
        cortex_types::Payload::ImpasseDetected { .. } => "impasse_detected",
        cortex_types::Payload::ConflictDetected { .. } => "conflict_detected",
        cortex_types::Payload::MetaControlApplied { .. } => "meta_control_applied",
        cortex_types::Payload::FrameCheckResult { .. } => "frame_check_result",
        cortex_types::Payload::ControlDecisionRecorded { .. } => "control_decision_recorded",
        cortex_types::Payload::ImpasseRecorded { .. } => "impasse_recorded",
        cortex_types::Payload::GoalSet { .. } => "goal_set",
        cortex_types::Payload::GoalShifted { .. } => "goal_shifted",
        cortex_types::Payload::GoalCompleted { .. } => "goal_completed",
        cortex_types::Payload::WorkingMemoryItemActivated { .. } => "working_memory_activated",
        cortex_types::Payload::WorkingMemoryItemRehearsed { .. } => "working_memory_rehearsed",
        cortex_types::Payload::WorkingMemoryItemEvicted { .. } => "working_memory_evicted",
        cortex_types::Payload::WorkingMemoryCapacityExceeded { .. } => {
            "working_memory_capacity_exceeded"
        }
        cortex_types::Payload::ChannelScheduled { .. } => "channel_scheduled",
        cortex_types::Payload::MaintenanceExecuted { .. } => "maintenance_executed",
        cortex_types::Payload::EmergencyTriggered { .. } => "emergency_triggered",
        cortex_types::Payload::ConfidenceAssessed { .. } => "confidence_assessed",
        cortex_types::Payload::ConfidenceLow { .. } => "confidence_low",
        cortex_types::Payload::PressureResponseApplied { .. } => "pressure_response_applied",
        cortex_types::Payload::MemorySplit { .. } => "memory_split",
        cortex_types::Payload::MemoryGraphHealthAssessed { .. } => "memory_graph_health_assessed",
        cortex_types::Payload::MemoryRelationReorganized { .. } => "memory_relation_reorganized",
        _ => "event",
    }
}

fn preview_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let preview: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{preview}...")
    } else {
        preview
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stored_event(payload: cortex_types::Payload) -> cortex_kernel::StoredEvent {
        cortex_kernel::StoredEvent {
            offset: 7,
            event_id: "event-1".to_string(),
            turn_id: "turn-1".to_string(),
            correlation_id: "corr-1".to_string(),
            timestamp: chrono::Utc::now(),
            event_type: "test".to_string(),
            payload,
            execution_version: "test".to_string(),
        }
    }

    #[test]
    fn timeline_limit_defaults_and_caps() {
        assert_eq!(timeline_limit(0), DEFAULT_TIMELINE_LIMIT);
        assert_eq!(timeline_limit(12), 12);
        assert_eq!(timeline_limit(MAX_TIMELINE_LIMIT + 1), MAX_TIMELINE_LIMIT);
    }

    #[test]
    fn timeline_counts_groups_known_payloads() {
        let events = vec![
            stored_event(cortex_types::Payload::TurnStarted),
            stored_event(cortex_types::Payload::ToolInvocationIntent {
                tool_name: "read".to_string(),
                input: "{}".to_string(),
            }),
        ];
        let counts = timeline_counts(&events);

        assert_eq!(counts.get("lifecycle"), Some(&1));
        assert_eq!(counts.get("tool"), Some(&1));
    }

    #[test]
    fn timeline_entry_truncates_message_preview() {
        let event = stored_event(cortex_types::Payload::UserMessage {
            content: "x".repeat(300),
        });
        let entry = timeline_entry(&event);

        assert_eq!(entry["category"], "message");
        assert_eq!(entry["label"], "user");
        assert_eq!(
            entry["details"]["preview"].as_str().map(str::len),
            Some(243)
        );
    }
}
