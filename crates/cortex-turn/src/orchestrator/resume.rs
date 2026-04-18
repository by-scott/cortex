use cortex_kernel::StoredEvent;
use cortex_types::{Payload, ResumePacket};

/// Build a [`ResumePacket`] from recent journal events.
///
/// Scans events to extract: last assistant message, recent tool names, pending user message.
/// When session boundaries are present, only processes events after the last `SessionStarted`.
#[must_use]
pub fn build_resume_packet(events: &[StoredEvent]) -> ResumePacket {
    // Find the last SessionStarted event to scope the projection.
    let session_start_idx = events
        .iter()
        .rposition(|e| matches!(&e.payload, Payload::SessionStarted { .. }));

    let scoped_events = session_start_idx.map_or(events, |idx| &events[idx..]);

    let session_id = scoped_events.iter().find_map(|e| match &e.payload {
        Payload::SessionStarted { session_id } => Some(session_id.clone()),
        _ => None,
    });

    let mut summary = String::new();
    let mut last_actions: Vec<String> = Vec::new();
    let mut pending_context: Option<String> = None;
    let mut last_user_message: Option<String> = None;
    let mut last_assistant_message: Option<String> = None;
    let mut goals: Vec<String> = Vec::new();
    let mut meta_alerts: Vec<String> = Vec::new();
    let mut active_skills: Vec<String> = Vec::new();

    for event in scoped_events {
        match &event.payload {
            Payload::UserMessage { content } => {
                last_user_message = Some(content.clone());
            }
            Payload::AssistantMessage { content } => {
                last_assistant_message = Some(content.clone());
                last_user_message = None;
            }
            Payload::ToolInvocationIntent { tool_name, .. } => {
                if !last_actions.contains(tool_name) {
                    last_actions.push(tool_name.clone());
                }
                if last_actions.len() > 5 {
                    last_actions.remove(0);
                }
            }
            Payload::GoalSet {
                level, description, ..
            } => {
                let tag = format!("[{level}] {description}");
                if !goals.contains(&tag) {
                    goals.push(tag);
                }
            }
            Payload::MetaControlApplied { action } if !meta_alerts.contains(action) => {
                meta_alerts.push(action.clone());
            }
            Payload::SkillInvoked { name, .. } if !active_skills.contains(name) => {
                active_skills.push(name.clone());
            }
            _ => {}
        }
    }

    if let Some(msg) = last_assistant_message {
        summary = if msg.len() > 200 {
            format!("{}...", &msg[..197])
        } else {
            msg
        };
    }

    if let Some(msg) = last_user_message {
        pending_context = Some(msg);
    }

    ResumePacket {
        summary,
        last_actions,
        pending_context,
        session_id,
        goals,
        meta_alerts,
        active_skills,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn make_stored(payload: Payload) -> StoredEvent {
        StoredEvent {
            offset: 0,
            event_id: "e1".into(),
            turn_id: "t1".into(),
            correlation_id: "c1".into(),
            timestamp: Utc::now(),
            event_type: "test".into(),
            payload,
            execution_version: String::new(),
        }
    }

    #[test]
    fn empty_events_empty_packet() {
        let packet = build_resume_packet(&[]);
        assert!(packet.is_empty());
    }

    #[test]
    fn extracts_last_assistant_message() {
        let events = vec![
            make_stored(Payload::UserMessage {
                content: "hello".into(),
            }),
            make_stored(Payload::AssistantMessage {
                content: "I can help".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert_eq!(packet.summary, "I can help");
        assert!(packet.pending_context.is_none());
    }

    #[test]
    fn tracks_tool_names() {
        let events = vec![
            make_stored(Payload::ToolInvocationIntent {
                tool_name: "read".into(),
                input: "{}".into(),
            }),
            make_stored(Payload::ToolInvocationIntent {
                tool_name: "write".into(),
                input: "{}".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert_eq!(packet.last_actions, vec!["read", "write"]);
    }

    #[test]
    fn pending_user_message() {
        let events = vec![
            make_stored(Payload::AssistantMessage {
                content: "done".into(),
            }),
            make_stored(Payload::UserMessage {
                content: "now do this".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert_eq!(packet.pending_context.unwrap(), "now do this");
        assert!(packet.session_id.is_none());
    }

    #[test]
    fn session_boundary_scopes_to_latest_session() {
        let events = vec![
            make_stored(Payload::SessionStarted {
                session_id: "sess-1".into(),
            }),
            make_stored(Payload::UserMessage {
                content: "old question".into(),
            }),
            make_stored(Payload::AssistantMessage {
                content: "old answer".into(),
            }),
            make_stored(Payload::SessionEnded {
                session_id: "sess-1".into(),
            }),
            make_stored(Payload::SessionStarted {
                session_id: "sess-2".into(),
            }),
            make_stored(Payload::UserMessage {
                content: "new question".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert_eq!(packet.session_id, Some("sess-2".into()));
        assert_eq!(packet.pending_context, Some("new question".into()));
        assert!(packet.summary.is_empty());
    }

    #[test]
    fn no_session_boundary_backward_compatible() {
        let events = vec![
            make_stored(Payload::UserMessage {
                content: "hello".into(),
            }),
            make_stored(Payload::AssistantMessage {
                content: "world".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert!(packet.session_id.is_none());
        assert_eq!(packet.summary, "world");
    }

    #[test]
    fn session_id_set_from_session_started() {
        let events = vec![
            make_stored(Payload::SessionStarted {
                session_id: "my-session".into(),
            }),
            make_stored(Payload::UserMessage {
                content: "test".into(),
            }),
            make_stored(Payload::AssistantMessage {
                content: "reply".into(),
            }),
        ];
        let packet = build_resume_packet(&events);
        assert_eq!(packet.session_id, Some("my-session".into()));
        assert_eq!(packet.summary, "reply");
        assert!(packet.pending_context.is_none());
    }
}
