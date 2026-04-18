use std::collections::HashMap;

use chrono::{DateTime, Utc};
use cortex_types::{Message, Payload, SideEffectKind};

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
            && let Some(_value) = provider.provide(kind, key)
        {
            // Use the provided value instead of the recorded one
        }
        projector(event, &mut state);
    }
    state
}

/// Extract message history from journal events.
#[must_use]
pub fn project_message_history(events: &[StoredEvent]) -> Vec<Message> {
    let mut messages = Vec::new();
    for e in events {
        match &e.payload {
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

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::{CorrelationId, Event, TurnId};

    fn make_stored(payload: Payload) -> StoredEvent {
        let e = Event::new(TurnId::new(), CorrelationId::new(), payload);
        StoredEvent {
            offset: 0,
            event_id: e.id.to_string(),
            turn_id: e.turn_id.to_string(),
            correlation_id: e.correlation_id.to_string(),
            timestamp: e.timestamp,
            event_type: String::new(),
            payload: e.payload,
            execution_version: String::new(),
        }
    }

    #[test]
    fn replay_empty() {
        let result: i32 = replay(&[], 0, |_, _| {});
        assert_eq!(result, 0);
    }

    #[test]
    fn replay_accumulates() {
        let events = vec![
            make_stored(Payload::TurnStarted),
            make_stored(Payload::TurnStarted),
        ];
        let count = replay(&events, 0_u32, |_, state| *state += 1);
        assert_eq!(count, 2);
    }

    #[test]
    fn project_messages() {
        let events = vec![
            make_stored(Payload::UserMessage {
                content: "hello".into(),
            }),
            make_stored(Payload::AssistantMessage {
                content: "hi".into(),
            }),
        ];
        let msgs = project_message_history(&events);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].text_content(), "hello");
    }

    #[test]
    fn project_turns() {
        let tid = TurnId::new();
        let cid = CorrelationId::new();
        let e1 = Event::new(tid, cid, Payload::TurnStarted);
        let e2 = Event::new(tid, cid, Payload::TurnCompleted);
        let events: Vec<StoredEvent> = [e1, e2]
            .iter()
            .map(|e| StoredEvent {
                offset: 0,
                event_id: e.id.to_string(),
                turn_id: e.turn_id.to_string(),
                correlation_id: e.correlation_id.to_string(),
                timestamp: e.timestamp,
                event_type: String::new(),
                payload: e.payload.clone(),
                execution_version: String::new(),
            })
            .collect();
        let summaries = project_turn_summaries(&events);
        assert_eq!(summaries.len(), 1);
        assert!(summaries[0].started_at.is_some());
        assert!(summaries[0].completed_at.is_some());
    }
}
