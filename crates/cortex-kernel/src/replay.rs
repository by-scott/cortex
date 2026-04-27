use std::collections::HashMap;

use chrono::{DateTime, Utc};
use cortex_types::{CausalRelation, Message, Payload, SideEffectKind};
use sha2::{Digest, Sha256};

use crate::journal::StoredEvent;

/// Trait for providing recorded side effects during deterministic replay.
pub trait SideEffectProvider {
    fn provide(&mut self, kind: &SideEffectKind, key: &str) -> Option<String>;
}

/// Replays side effects from previously recorded journal events.
pub struct JournalSideEffectProvider {
    recordings: HashMap<(SideEffectKind, String), String>,
}

impl JournalSideEffectProvider {
    #[must_use]
    pub fn from_events(events: &[StoredEvent]) -> Self {
        let mut recordings = HashMap::new();
        for e in events {
            if let Payload::SideEffectRecorded { kind, key, value } = &e.payload {
                recordings.insert((kind.clone(), key.clone()), value.clone());
            }
        }
        Self { recordings }
    }
}

impl SideEffectProvider for JournalSideEffectProvider {
    fn provide(&mut self, kind: &SideEffectKind, key: &str) -> Option<String> {
        self.recordings
            .get(&(kind.clone(), key.to_string()))
            .cloned()
    }
}

/// Summary of a single Turn extracted from journal events.
#[derive(Debug, Clone)]
pub struct TurnSummary {
    pub turn_id: String,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub tool_calls: Vec<String>,
    pub has_response: bool,
}

/// Versioned replay projection surfaces used by compatibility tests and audit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectionVersion {
    pub name: String,
    pub version: u32,
}

/// One inferred causal edge in a replay audit graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayCausalEdge {
    pub cause_event_id: String,
    pub effect_event_id: String,
    pub cause_type: String,
    pub effect_type: String,
    pub relation: CausalRelation,
    pub reason: String,
}

/// Replay-time audit graph for explaining why projected state changed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayAuditGraph {
    pub projection_versions: Vec<ProjectionVersion>,
    pub edges: Vec<ReplayCausalEdge>,
    pub root_event_ids: Vec<String>,
    pub terminal_event_ids: Vec<String>,
}

/// High-level diff between two replay projections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplayDiff {
    pub left_digest: String,
    pub right_digest: String,
    pub same_digest: bool,
    pub left_message_count: usize,
    pub right_message_count: usize,
    pub left_turn_count: usize,
    pub right_turn_count: usize,
    pub left_tool_effect_count: usize,
    pub right_tool_effect_count: usize,
    pub changed_categories: Vec<String>,
}

/// Generic fold over stored events.
#[must_use]
pub fn replay<S>(
    events: &[StoredEvent],
    init: S,
    mut projector: impl FnMut(&StoredEvent, &mut S),
) -> S {
    let mut state = init;
    for event in events {
        projector(event, &mut state);
    }
    state
}

/// Fold with side-effect substitution for deterministic replay.
#[must_use]
pub fn replay_with_sideeffects<S>(
    events: &[StoredEvent],
    init: S,
    mut projector: impl FnMut(&StoredEvent, &mut S),
    provider: &mut dyn SideEffectProvider,
) -> S {
    let mut state = init;
    for event in events {
        if let Payload::SideEffectRecorded { kind, key, .. } = &event.payload
            && let Some(value) = provider.provide(kind, key)
        {
            let mut substituted = event.clone();
            substituted.payload = Payload::SideEffectRecorded {
                kind: kind.clone(),
                key: key.clone(),
                value,
            };
            projector(&substituted, &mut state);
        } else {
            projector(event, &mut state);
        }
    }
    state
}

/// Produce a deterministic digest over the replay projection after applying
/// side-effect substitution.
///
/// Event IDs and timestamps are deliberately excluded so this can compare
/// equivalent runs across fresh journals.
#[must_use]
pub fn replay_determinism_digest(
    events: &[StoredEvent],
    provider: &mut dyn SideEffectProvider,
) -> String {
    let digest = replay_with_sideeffects(
        events,
        Sha256::new(),
        |event, hasher| {
            hasher.update(event.turn_id.as_bytes());
            hasher.update([0]);
            hasher.update(event.correlation_id.as_bytes());
            hasher.update([0]);
            let payload = serde_json::to_vec(&event.payload).unwrap_or_default();
            hasher.update(payload);
            hasher.update([0xff]);
        },
        provider,
    )
    .finalize();
    hex::encode(digest)
}

/// Current projection versions used by replay audit surfaces.
#[must_use]
pub fn replay_projection_versions() -> Vec<ProjectionVersion> {
    vec![
        ProjectionVersion {
            name: "message_history".to_string(),
            version: 3,
        },
        ProjectionVersion {
            name: "turn_summary".to_string(),
            version: 1,
        },
        ProjectionVersion {
            name: "replay_audit_graph".to_string(),
            version: 1,
        },
    ]
}

/// Build a replay audit graph from stored events.
///
/// The graph is deterministic and does not mutate the journal. It prefers
/// explicit event identity over event type so an operator can trace a projected
/// state change back to concrete journal rows.
#[must_use]
pub fn project_replay_audit_graph(events: &[StoredEvent]) -> ReplayAuditGraph {
    let mut edges = Vec::new();
    for (index, event) in events.iter().enumerate() {
        add_replay_causal_edges(events, index, event, &mut edges);
    }

    let mut has_incoming = std::collections::HashSet::new();
    let mut has_outgoing = std::collections::HashSet::new();
    for edge in &edges {
        has_outgoing.insert(edge.cause_event_id.clone());
        has_incoming.insert(edge.effect_event_id.clone());
    }
    let root_event_ids = events
        .iter()
        .filter(|event| !has_incoming.contains(&event.event_id))
        .map(|event| event.event_id.clone())
        .collect();
    let terminal_event_ids = events
        .iter()
        .filter(|event| !has_outgoing.contains(&event.event_id))
        .map(|event| event.event_id.clone())
        .collect();

    ReplayAuditGraph {
        projection_versions: replay_projection_versions(),
        edges,
        root_event_ids,
        terminal_event_ids,
    }
}

/// Compare two event streams through the stable replay projections.
#[must_use]
pub fn diff_replay_projection(
    left: &[StoredEvent],
    right: &[StoredEvent],
    left_provider: &mut dyn SideEffectProvider,
    right_provider: &mut dyn SideEffectProvider,
) -> ReplayDiff {
    let left_digest = replay_determinism_digest(left, left_provider);
    let right_digest = replay_determinism_digest(right, right_provider);
    let left_message_count = project_message_history(left).len();
    let right_message_count = project_message_history(right).len();
    let left_turn_count = project_turn_summaries(left).len();
    let right_turn_count = project_turn_summaries(right).len();
    let left_tool_effect_count = count_tool_effect_events(left);
    let right_tool_effect_count = count_tool_effect_events(right);
    let mut changed_categories = Vec::new();
    if left_digest != right_digest {
        changed_categories.push("digest".to_string());
    }
    if left_message_count != right_message_count {
        changed_categories.push("message_history".to_string());
    }
    if left_turn_count != right_turn_count {
        changed_categories.push("turn_summary".to_string());
    }
    if left_tool_effect_count != right_tool_effect_count {
        changed_categories.push("tool_effects".to_string());
    }

    ReplayDiff {
        same_digest: left_digest == right_digest,
        left_digest,
        right_digest,
        left_message_count,
        right_message_count,
        left_turn_count,
        right_turn_count,
        left_tool_effect_count,
        right_tool_effect_count,
        changed_categories,
    }
}

fn count_tool_effect_events(events: &[StoredEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event.payload,
                Payload::ToolEffectPreviewed { .. }
                    | Payload::ToolEffectVerified { .. }
                    | Payload::ToolEffectCommitted { .. }
            )
        })
        .count()
}

fn add_replay_causal_edges(
    events: &[StoredEvent],
    index: usize,
    event: &StoredEvent,
    edges: &mut Vec<ReplayCausalEdge>,
) {
    for rule in causal_rules_for(&event.event_type) {
        if let Some(cause) = find_prior_event(events, index, rule.cause_type, rule.same_turn) {
            edges.push(ReplayCausalEdge {
                cause_event_id: cause.event_id.clone(),
                effect_event_id: event.event_id.clone(),
                cause_type: cause.event_type.clone(),
                effect_type: event.event_type.clone(),
                relation: rule.relation,
                reason: rule.reason.to_string(),
            });
        }
    }
}

fn find_prior_event<'a>(
    events: &'a [StoredEvent],
    index: usize,
    event_type: &str,
    same_turn: bool,
) -> Option<&'a StoredEvent> {
    let current = events.get(index)?;
    events[..index].iter().rev().find(|candidate| {
        candidate.event_type == event_type
            && candidate.correlation_id == current.correlation_id
            && (!same_turn || candidate.turn_id == current.turn_id)
    })
}

#[derive(Debug, Clone, Copy)]
struct CausalRule {
    cause_type: &'static str,
    relation: CausalRelation,
    reason: &'static str,
    same_turn: bool,
}

const USER_MESSAGE_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "TurnStarted",
    relation: CausalRelation::Triggers,
    reason: "turn start admits the first user message",
    same_turn: true,
}];

const EVIDENCE_PROMOTED_RULES: &[CausalRule] = &[
    CausalRule {
        cause_type: "EvidenceRetrieved",
        relation: CausalRelation::Enables,
        reason: "retrieved evidence can be promoted into workspace",
        same_turn: true,
    },
    CausalRule {
        cause_type: "GuardrailTriggered",
        relation: CausalRelation::Invalidates,
        reason: "guardrail findings can invalidate unsafe evidence promotion",
        same_turn: true,
    },
];

const TOOL_INVOCATION_INTENT_RULES: &[CausalRule] = &[
    CausalRule {
        cause_type: "UserMessage",
        relation: CausalRelation::Enables,
        reason: "user request enables tool planning",
        same_turn: true,
    },
    CausalRule {
        cause_type: "EvidencePromoted",
        relation: CausalRelation::Contributes,
        reason: "workspace evidence contributes to tool selection",
        same_turn: true,
    },
];

const TOOL_EFFECT_PREVIEWED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ToolInvocationIntent",
    relation: CausalRelation::Triggers,
    reason: "tool intent must be previewed before mutating effects proceed",
    same_turn: true,
}];

const PERMISSION_REQUESTED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ToolEffectPreviewed",
    relation: CausalRelation::DependsOn,
    reason: "permission request depends on the effect preview",
    same_turn: true,
}];

const PERMISSION_RESOLVED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "PermissionRequested",
    relation: CausalRelation::Triggers,
    reason: "operator decision resolves the pending permission request",
    same_turn: true,
}];

const TOOL_INVOCATION_RESULT_RULES: &[CausalRule] = &[
    CausalRule {
        cause_type: "PermissionGranted",
        relation: CausalRelation::Enables,
        reason: "permission grant enables gated tool execution",
        same_turn: true,
    },
    CausalRule {
        cause_type: "ToolInvocationIntent",
        relation: CausalRelation::Triggers,
        reason: "tool intent triggers tool execution result",
        same_turn: true,
    },
];

const TOOL_EFFECT_VERIFIED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ToolInvocationResult",
    relation: CausalRelation::Triggers,
    reason: "tool result is verified before commit",
    same_turn: true,
}];

const TOOL_EFFECT_COMMITTED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ToolEffectVerified",
    relation: CausalRelation::DependsOn,
    reason: "effect commit depends on verification",
    same_turn: true,
}];

const ASSISTANT_MESSAGE_RULES: &[CausalRule] = &[
    CausalRule {
        cause_type: "ToolInvocationResult",
        relation: CausalRelation::Contributes,
        reason: "tool result contributes to assistant response",
        same_turn: true,
    },
    CausalRule {
        cause_type: "UserMessage",
        relation: CausalRelation::Triggers,
        reason: "user message triggers assistant response",
        same_turn: true,
    },
];

const MEMORY_CAPTURED_RULES: &[CausalRule] = &[
    CausalRule {
        cause_type: "AssistantMessage",
        relation: CausalRelation::Contributes,
        reason: "assistant response can generate memory candidates",
        same_turn: true,
    },
    CausalRule {
        cause_type: "ToolInvocationResult",
        relation: CausalRelation::Contributes,
        reason: "tool result can generate memory candidates",
        same_turn: true,
    },
    CausalRule {
        cause_type: "GuardrailTriggered",
        relation: CausalRelation::Contributes,
        reason: "guardrail finding can create hostile-source memory candidates",
        same_turn: true,
    },
];

const CONTEXT_COMPACTED_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ContextPressureObserved",
    relation: CausalRelation::Triggers,
    reason: "context pressure triggers compaction",
    same_turn: true,
}];

const CONTEXT_COMPACT_BOUNDARY_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "ContextCompacted",
    relation: CausalRelation::DependsOn,
    reason: "compact boundary depends on the compaction decision",
    same_turn: true,
}];

const TURN_TERMINAL_RULES: &[CausalRule] = &[CausalRule {
    cause_type: "TurnStarted",
    relation: CausalRelation::DependsOn,
    reason: "terminal turn state depends on turn start",
    same_turn: true,
}];

fn causal_rules_for(effect_type: &str) -> &'static [CausalRule] {
    match effect_type {
        "UserMessage" => USER_MESSAGE_RULES,
        "EvidencePromoted" => EVIDENCE_PROMOTED_RULES,
        "ToolInvocationIntent" => TOOL_INVOCATION_INTENT_RULES,
        "ToolEffectPreviewed" => TOOL_EFFECT_PREVIEWED_RULES,
        "PermissionRequested" => PERMISSION_REQUESTED_RULES,
        "PermissionGranted" | "PermissionDenied" => PERMISSION_RESOLVED_RULES,
        "ToolInvocationResult" => TOOL_INVOCATION_RESULT_RULES,
        "ToolEffectVerified" => TOOL_EFFECT_VERIFIED_RULES,
        "ToolEffectCommitted" => TOOL_EFFECT_COMMITTED_RULES,
        "AssistantMessage" => ASSISTANT_MESSAGE_RULES,
        "MemoryCaptured" => MEMORY_CAPTURED_RULES,
        "ContextCompacted" => CONTEXT_COMPACTED_RULES,
        "ContextCompactBoundary" => CONTEXT_COMPACT_BOUNDARY_RULES,
        "TurnCompleted" | "TurnInterrupted" => TURN_TERMINAL_RULES,
        _ => &[],
    }
}

/// Extract message history from journal events.
#[must_use]
pub fn project_message_history(events: &[StoredEvent]) -> Vec<Message> {
    let mut messages = Vec::new();
    for e in events {
        match &e.payload {
            Payload::ContextCompactBoundary {
                summary,
                replacement_messages,
                ..
            } => {
                messages.clear();
                if replacement_messages.is_empty() {
                    messages.push(Message::user(format!("[Conversation Summary]\n{summary}")));
                } else {
                    messages.extend(replacement_messages.iter().cloned());
                }
            }
            Payload::UserMessage { content, .. } => {
                messages.push(Message::user(content.as_str()));
            }
            Payload::AssistantMessage { content, .. } => {
                messages.push(Message::assistant(content.as_str()));
            }
            _ => {}
        }
    }
    messages
}

/// Group events into per-turn summaries.
#[must_use]
pub fn project_turn_summaries(events: &[StoredEvent]) -> Vec<TurnSummary> {
    let mut turns: HashMap<String, TurnSummary> = HashMap::new();

    for e in events {
        let tid = e.turn_id.clone();
        let summary = turns.entry(tid.clone()).or_insert_with(|| TurnSummary {
            turn_id: tid,
            started_at: None,
            completed_at: None,
            tool_calls: Vec::new(),
            has_response: false,
        });

        match &e.payload {
            Payload::TurnStarted => {
                summary.started_at = Some(e.timestamp);
            }
            Payload::TurnCompleted => {
                summary.completed_at = Some(e.timestamp);
            }
            Payload::ToolInvocationIntent { tool_name, .. } => {
                summary.tool_calls.push(tool_name.clone());
            }
            Payload::AssistantMessage { .. } => {
                summary.has_response = true;
            }
            _ => {}
        }
    }

    let mut result: Vec<TurnSummary> = turns.into_values().collect();
    result.sort_by_key(|t| t.started_at);
    result
}
