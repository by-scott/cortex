//! Message channel framework for external messaging platforms.
//!
//! Channels run inside the daemon process, sharing `DaemonState` directly.
//! The daemon starts/stops them alongside other transports (HTTP, socket, stdio).

pub mod pairing;
pub mod store;
pub mod telegram;
pub mod whatsapp;

use std::path::Path;
use std::sync::Arc;

use pairing::PairingAction;
use store::ChannelStore;

use crate::daemon::DaemonState;

/// Process an inbound message through pairing + Cortex execution.
///
/// Uses streaming execution so trace events (tool, meta, etc.) are captured
/// and included in the response, matching the behavior of Web and CLI.
/// Session state is tracked per-user via the channel store so that
/// `/session new`, `/session switch <id>`, and `/quit` work uniformly
/// across `Telegram`, `WhatsApp`, and any future channel.
pub fn handle_message(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    user_id: &str,
    user_name: &str,
    text: &str,
    session_prefix: &str,
) -> String {
    // Check pairing
    match pairing::check_user(store, user_id, user_name, session_prefix) {
        PairingAction::Allowed => {}
        PairingAction::SendPairingPrompt(msg) => return msg,
        PairingAction::Denied => return String::new(),
    }

    // Resolve (or create) the user's active session
    let session_id = store.active_session(user_id).unwrap_or_else(|| {
        let sid = format!("{session_prefix}-{user_id}");
        store.set_active_session(user_id, &sid);
        sid
    });

    // Slash commands — session management syncs to channel store;
    // everything else delegates to the daemon's dispatch_command.
    if text.starts_with('/') {
        let trimmed = text.trim();

        if trimmed == "/session new" {
            let (new_sid, _) = state.session_manager().create_session();
            let sid_str = new_sid.to_string();
            store.set_active_session(user_id, &sid_str);
            return format!("New session: {sid_str}");
        }
        if let Some(id) = trimmed
            .strip_prefix("/session switch ")
            .or_else(|| trimmed.strip_prefix("/session resume "))
            .map(str::trim)
        {
            store.set_active_session(user_id, id);
            return format!("Switched to session: {id}");
        }
        if trimmed == "/quit" || trimmed == "/exit" {
            store.set_active_session(user_id, "");
            return "Session ended. Send a message to start fresh.".into();
        }

        // All other commands — fully reuse daemon logic
        return state.dispatch_command(text);
    }

    // If a turn is already running, inject the message mid-turn so the LLM
    // sees it in the next TPN iteration.
    if state.inject_message(&session_id, text.to_string()) {
        return "Message injected into running turn.".into();
    }

    run_single_turn(state, &session_id, text, session_prefix)
}

fn run_single_turn(state: &Arc<DaemonState>, session_id: &str, text: &str, source: &str) -> String {
    let tracer = crate::daemon::TracingTurnTracer {
        config: state.config().turn.trace.clone(),
    };
    let turn_input = crate::turn_executor::TurnInput {
        text,
        attachments: &[],
        inline_images: &[],
    };
    state
        .execute_turn_streaming(
            session_id,
            &turn_input,
            source,
            |_chunk| { /* text assembled by execute_turn_streaming into final response */ },
            |progress| {
                let status = match progress.status {
                    cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                    cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                    cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
                };
                tracing::info!(
                    tool = progress.tool_name.as_str(),
                    status,
                    "Tool: {} ({status})",
                    progress.tool_name,
                );
            },
            &tracer,
        )
        .unwrap_or_else(|e| format!("Error: {e}"))
}

/// Split a message into chunks respecting a platform's character limit.
#[must_use]
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_len).min(text.len());
        if end < text.len() {
            if let Some(nl) = text[start..end].rfind('\n') {
                end = start + nl + 1;
            } else {
                while end > start && !text.is_char_boundary(end) {
                    end -= 1;
                }
            }
        }
        if end == start {
            end = start + 1;
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }
    chunks
}

/// Read channel auth credentials from `channels/<platform>/auth.json`.
#[must_use]
pub fn read_channel_auth(home: &Path, platform: &str) -> Option<serde_json::Value> {
    let path = home.join("channels").join(platform).join("auth.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Save channel auth credentials to `channels/<platform>/auth.json`.
pub fn save_channel_auth(home: &Path, platform: &str, auth: &serde_json::Value) {
    let dir = home.join("channels").join(platform);
    let _ = std::fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string_pretty(auth) {
        let _ = std::fs::write(dir.join("auth.json"), json);
    }
}
