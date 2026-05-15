use crate::orchestrator::stream::ThinkStreamFilter;
use crate::orchestrator::{StreamLane, TurnStreamBoundary, TurnStreamEvent, strip_think_tags};

use super::ToolProgress;

pub(super) fn emit_text_event(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    lane: StreamLane,
    source: Option<&str>,
    content: &str,
) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::Text {
            lane,
            source: source.map(str::to_string),
            content: content.to_string(),
        });
    }
}

pub(super) fn emit_filtered_stream_text(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    stream_filter: &std::sync::Mutex<ThinkStreamFilter>,
    text: &str,
) {
    let visible = stream_filter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(text);
    if !visible.is_empty() {
        emit_text_event(on_event, StreamLane::UserVisible, None, &visible);
    }
}

pub(super) fn emit_pending_stream_text(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    stream_filter: &std::sync::Mutex<ThinkStreamFilter>,
) {
    let pending_visible = stream_filter
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish();
    if !pending_visible.is_empty() {
        emit_text_event(on_event, StreamLane::UserVisible, None, &pending_visible);
    }
}

pub(super) fn emit_tool_progress(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    progress: ToolProgress,
) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::ToolProgress(progress));
    }
}

pub(super) fn emit_restart_boundary_event(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::Boundary(TurnStreamBoundary::Restart));
    }
}

pub(super) fn visible_assistant_text(strip_think: bool, text: &str) -> String {
    if strip_think {
        strip_think_tags(text)
    } else {
        text.to_string()
    }
}
