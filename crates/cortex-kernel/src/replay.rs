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

    struct OverrideProvider;

    impl SideEffectProvider for OverrideProvider {
        fn provide(&mut self, kind: &SideEffectKind, key: &str) -> Option<String> {
            if *kind == SideEffectKind::LlmResponse && key == "llm:1" {
                Some("substituted".into())
            } else {
                None
            }
        }
    }

    struct JournalOverrideProvider;

    impl SideEffectProvider for JournalOverrideProvider {
        fn provide(&mut self, kind: &SideEffectKind, key: &str) -> Option<String> {
            if *kind == SideEffectKind::ExternalIo && key == "bash" {
                Some("provided output".into())
            } else {
                None
            }
        }
    }

    #[test]
    fn replay_with_sideeffects_substitutes_recorded_value() {
        let events = vec![make_stored(Payload::SideEffectRecorded {
            kind: SideEffectKind::LlmResponse,
            key: "llm:1".into(),
            value: "recorded".into(),
        })];
        let mut provider = OverrideProvider;

        let projected = replay_with_sideeffects(
            &events,
            String::new(),
            |event, state| {
                if let Payload::SideEffectRecorded { value, .. } = &event.payload {
                    *state = value.clone();
                }
            },
            &mut provider,
        );

        assert_eq!(projected, "substituted");
    }

    #[test]
    fn replay_with_sideeffects_substitutes_events_loaded_from_journal() {
        let tmp = tempfile::tempdir().unwrap();
        let journal = crate::journal::Journal::open(tmp.path().join("cortex.db")).unwrap();
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();
        journal
            .append(&Event::new(
                turn_id,
                corr_id,
                Payload::SideEffectRecorded {
                    kind: SideEffectKind::ExternalIo,
                    key: "bash".into(),
                    value: "recorded output".into(),
                },
            ))
            .unwrap();
        let events = journal.recent_events(10).unwrap();

        let mut provider = JournalOverrideProvider;
        let projected = replay_with_sideeffects(
            &events,
            String::new(),
            |event, state| {
                if let Payload::SideEffectRecorded { value, .. } = &event.payload {
                    *state = value.clone();
                }
            },
            &mut provider,
        );

        assert_eq!(projected, "provided output");
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
    fn project_messages_replaces_at_compact_boundary() {
        let events = vec![
            make_stored(Payload::UserMessage {
                content: "old".into(),
            }),
            make_stored(Payload::ContextCompactBoundary {
                original_tokens: 100,
                compressed_tokens: 20,
                preserved_user_messages: 1,
                suffix_messages: 1,
                summary: "summary".into(),
                replacement_messages: vec![
                    Message::user("[Conversation Summary]\nsummary"),
                    Message::user("preserved"),
                ],
            }),
            make_stored(Payload::AssistantMessage {
                content: "new".into(),
            }),
        ];

        let msgs = project_message_history(&events);

        assert_eq!(msgs.len(), 3);
        assert_eq!(msgs[0].text_content(), "[Conversation Summary]\nsummary");
        assert_eq!(msgs[1].text_content(), "preserved");
        assert_eq!(msgs[2].text_content(), "new");
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
