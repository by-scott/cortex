use std::fmt::Write as _;

use super::DaemonState;
use crate::format::{fmt_tokens, format_duration};

pub(super) fn format_status_for_session(state: &DaemonState, session_id: Option<&str>) -> String {
    let snap = state.metrics.snapshot();
    let session_tokens = status_session_tokens(state, session_id);
    let cfg = state.config().clone();
    let model = cfg.api.model.clone();
    let thinking_output = if cfg.turn.strip_think_tags {
        "request off / output hidden"
    } else {
        "request on / output shown"
    };
    let trace_level = format!("{:?}", cfg.turn.trace.level).to_lowercase();
    let tool_count = state.tools.tool_names().len();
    let pending_memories = state
        .heartbeat_state
        .pending_consolidation
        .load(std::sync::atomic::Ordering::Relaxed);
    let pending_embeddings = state
        .heartbeat_state
        .pending_embeddings
        .load(std::sync::atomic::Ordering::Relaxed);
    let uptime_secs = chrono::Utc::now()
        .signed_duration_since(state.start_time)
        .num_seconds();
    let uptime = format_duration(uptime_secs);
    let session_count = state
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    let persisted_sessions = state.session_manager().list_sessions();
    let persisted_session_count = persisted_sessions.len();
    let persisted_turn_count: usize = persisted_sessions.iter().map(|s| s.turn_count).sum();
    let journal_event_count = state.journal.event_count().unwrap_or(0);
    let busy = state.turn_semaphore.available_permits() == 0;
    let queue_depth = 1usize.saturating_sub(state.turn_semaphore.available_permits());
    let active_bindings = state.active_session_bindings();
    let shared_bindings: Vec<(String, Vec<String>)> = active_bindings
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .cloned()
        .collect();
    let shared_owner_count: usize = shared_bindings.iter().map(|(_, owners)| owners.len()).sum();
    let transports = state
        .active_transports
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .join(" \u{b7} ");

    let dot = if busy { "\u{1f7e2}" } else { "\u{26aa}" };
    let tool_success = if snap.tool_calls == 0 {
        "n/a".to_string()
    } else {
        format!("{:.0}%", snap.tool_success_rate * 100.0)
    };

    let mut out = String::new();
    let _ = writeln!(
        out,
        "{dot} Cortex v{} \u{b7} {uptime}",
        env!("CARGO_PKG_VERSION")
    );
    let _ = writeln!(out);
    let _ = writeln!(out, "🔄 State      {}", if busy { "busy" } else { "idle" });
    let _ = writeln!(out, "🧠 Model      {model}");
    let _ = writeln!(out, "💭 Thinking   {thinking_output}");
    if !transports.is_empty() {
        let _ = writeln!(out, "🔌 Transports {transports}");
    }
    let _ = writeln!(
        out,
        "🗂️ Sessions   {session_count} active  Queue {queue_depth}  Trace {trace_level}"
    );
    let _ = writeln!(
        out,
        "🔗 Bindings   {} targets  {} shared sessions / {} clients",
        active_bindings.len(),
        shared_bindings.len(),
        shared_owner_count
    );
    let _ = writeln!(out, "🛠️ Tools      {tool_count} loaded");
    let _ = writeln!(out);
    let _ = writeln!(
        out,
        "💬 Turns      {} (errors: {})",
        snap.turn_count, snap.turn_errors
    );
    let _ = writeln!(
        out,
        "💾 Persisted  {persisted_turn_count} turns / {persisted_session_count} sessions / {journal_event_count} events"
    );
    write_status_counters(
        &mut out,
        &snap,
        session_tokens,
        &tool_success,
        pending_memories,
        pending_embeddings,
    );
    write_shared_bindings(&mut out, &shared_bindings);
    out
}

fn status_session_tokens(state: &DaemonState, session_id: Option<&str>) -> Option<u64> {
    state.session_token_total(session_id).or_else(|| {
        if session_id.is_some() {
            return None;
        }

        let sessions = state
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if sessions.len() != 1 {
            return None;
        }
        sessions
            .values()
            .next()
            .map(|session| session.meta.total_tokens())
    })
}

fn write_status_counters(
    out: &mut String,
    snap: &crate::metrics::LiveMetrics,
    session_tokens: Option<u64>,
    tool_success: &str,
    pending_memories: u32,
    pending_embeddings: u32,
) {
    let _ = writeln!(
        out,
        "🪟 Context    call {} in / {} out",
        fmt_tokens(snap.last_call_input_tokens),
        fmt_tokens(snap.last_call_output_tokens),
    );
    let _ = writeln!(
        out,
        "🧊 Cache      call {} read / {} write",
        fmt_tokens(snap.last_call_cache_read_input_tokens),
        fmt_tokens(snap.last_call_cache_creation_input_tokens),
    );
    let session_tokens = session_tokens.map_or_else(|| "n/a".to_string(), fmt_tokens);
    let _ = writeln!(
        out,
        "🧮 Tokens     total {} / session {session_tokens}",
        fmt_tokens(snap.total_tokens),
    );
    let _ = writeln!(
        out,
        "🛠️ Tools run  {} calls / {} errors / {} success",
        snap.tool_calls, snap.tool_errors, tool_success
    );
    let _ = writeln!(
        out,
        "🧠 Memory     {} captures / {} recalls / {} alerts",
        snap.memory_captures, snap.memory_recalls, snap.alerts_fired,
    );
    let _ = writeln!(
        out,
        "📦 Backlog    {pending_memories} consolidate / {pending_embeddings} embed",
    );
}

fn write_shared_bindings(out: &mut String, shared_bindings: &[(String, Vec<String>)]) {
    if shared_bindings.is_empty() {
        return;
    }

    let _ = writeln!(out);
    for (idx, (session_id, owners)) in shared_bindings.iter().take(5).enumerate() {
        let short_id = &session_id[..session_id.len().min(12)];
        let label = if idx == 0 { "Shared" } else { "          " };
        let _ = writeln!(out, "{label}    {short_id} <= {}", owners.join(", "));
    }
    if shared_bindings.len() > 5 {
        let _ = writeln!(
            out,
            "          ... {} more shared sessions",
            shared_bindings.len() - 5
        );
    }
}

pub(super) fn status(state: &DaemonState) -> serde_json::Value {
    let uptime = chrono::Utc::now()
        .signed_duration_since(state.start_time)
        .num_seconds();
    let session_count = state
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    let transports = state
        .active_transports
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let metrics = state.metrics.snapshot();
    let auto_approve_up_to = {
        let config = state.config();
        config.risk.auto_approve_up_to
    };
    let auto_approve_up_to = format!("{auto_approve_up_to:?}");

    serde_json::json!({
        "uptime_secs": uptime,
        "session_count": session_count,
        "transports": transports,
        "metrics": {
            "total_input_tokens": metrics.total_input_tokens,
            "total_output_tokens": metrics.total_output_tokens,
            "total_tokens": metrics.total_tokens,
            "last_turn_input_tokens": metrics.last_turn_input_tokens,
            "last_turn_output_tokens": metrics.last_turn_output_tokens,
            "last_turn_tokens": metrics.last_turn_tokens,
            "last_call_input_tokens": metrics.last_call_input_tokens,
            "last_call_output_tokens": metrics.last_call_output_tokens,
            "last_call_tokens": metrics.last_call_tokens,
            "total_cache_read_input_tokens": metrics.total_cache_read_input_tokens,
            "total_cache_creation_input_tokens": metrics.total_cache_creation_input_tokens,
            "last_turn_cache_read_input_tokens": metrics.last_turn_cache_read_input_tokens,
            "last_turn_cache_creation_input_tokens": metrics.last_turn_cache_creation_input_tokens,
            "last_call_cache_read_input_tokens": metrics.last_call_cache_read_input_tokens,
            "last_call_cache_creation_input_tokens": metrics.last_call_cache_creation_input_tokens,
            "turn_count": metrics.turn_count,
            "turn_errors": metrics.turn_errors,
        },
        "risk": {
            "auto_approve_up_to": auto_approve_up_to,
        },
        "version": env!("CARGO_PKG_VERSION"),
    })
}

pub(super) fn operator_dashboard(state: &DaemonState, requested_limit: usize) -> serde_json::Value {
    let limit = crate::dashboard::timeline_limit(requested_limit);
    let events = state.journal.recent_events(limit).unwrap_or_default();
    let timeline: Vec<serde_json::Value> = events
        .iter()
        .map(crate::dashboard::timeline_entry)
        .collect();
    let metrics = state.metrics.snapshot();
    let config = state.config().clone();
    let providers = state
        .providers
        .read()
        .map(|guard| guard.clone())
        .unwrap_or_default();
    let persisted_sessions = state.session_manager().list_sessions();
    let active_session_count = state
        .sessions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    let active_bindings = state.active_session_bindings();
    let shared_bindings: Vec<serde_json::Value> = active_bindings
        .iter()
        .filter(|(_, owners)| owners.len() > 1)
        .map(|(session_id, owners)| {
            serde_json::json!({
                "session_id": session_id,
                "owners": owners,
            })
        })
        .collect();
    let active_transports = state
        .active_transports
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let pending_permissions = state
        .pending_permissions
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    let busy = state.turn_semaphore.available_permits() == 0;
    let queue_depth = 1usize.saturating_sub(state.turn_semaphore.available_permits());
    let registry = cortex_types::ModelCapabilityRegistry::from_config(&config, &providers);
    serde_json::json!({
        "generated_at": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        "state": {
            "busy": busy,
            "queue_depth": queue_depth,
            "trace": format!("{:?}", config.turn.trace.level).to_lowercase(),
            "uptime_secs": chrono::Utc::now()
                .signed_duration_since(state.start_time)
                .num_seconds(),
        },
        "provider": {
            "primary": {
                "provider": &config.api.provider,
                "model": &config.api.model,
                "preset": format!("{:?}", config.api.preset).to_lowercase(),
            },
            "profiles": crate::dashboard::model_profiles_json(&registry.profiles),
        },
        "transports": active_transports,
        "sessions": {
            "active": active_session_count,
            "persisted": persisted_sessions.len(),
            "persisted_turns": persisted_sessions.iter().map(|session| session.turn_count).sum::<usize>(),
            "active_bindings": active_bindings.len(),
            "shared_bindings": shared_bindings,
        },
        "tools": {
            "loaded": state.tools.tool_names().len(),
            "pending_permissions": pending_permissions,
        },
        "backlog": {
            "consolidate": state.heartbeat_state.pending_consolidation.load(std::sync::atomic::Ordering::Relaxed),
            "embed": state.heartbeat_state.pending_embeddings.load(std::sync::atomic::Ordering::Relaxed),
        },
        "risk": {
            "auto_approve_up_to": format!("{:?}", config.risk.auto_approve_up_to),
        },
        "metrics": serde_json::to_value(&metrics).unwrap_or_default(),
        "timeline": {
            "limit": limit,
            "counts": crate::dashboard::timeline_counts(&events),
            "events": timeline,
        },
    })
}
