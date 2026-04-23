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
