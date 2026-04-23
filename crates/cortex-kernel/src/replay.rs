use std::collections::HashMap;

use chrono::{DateTime, Utc};
use cortex_types::{Message, Payload, SideEffectKind};
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
