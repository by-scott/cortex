use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json as JsonResponse};

use super::http_api::HttpState;

pub(super) async fn handle_memory_list(State(state): State<HttpState>) -> impl IntoResponse {
    let actor = state.daemon.transport_actor("http");
    let memories = state
        .daemon
        .memory_store()
        .list_for_actor(&actor)
        .unwrap_or_default();
    (
        StatusCode::OK,
        JsonResponse(serde_json::to_value(memories).unwrap_or_default()),
    )
}

pub(super) async fn handle_memory_save_http(
    State(state): State<HttpState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            JsonResponse(serde_json::json!({"error": "missing content"})),
        )
            .into_response();
    }
    let memory_type: cortex_types::MemoryType = body
        .get("memory_type")
        .or_else(|| body.get("type"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(cortex_types::MemoryType::User);
    let kind: cortex_types::MemoryKind = body
        .get("kind")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(cortex_types::MemoryKind::Episodic);
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let mut entry = cortex_types::MemoryEntry::new(content, description, memory_type, kind);
    entry.owner_actor = state.daemon.transport_actor("http");
    let id = entry.id.clone();
    match state.daemon.memory_store().save(&entry) {
        Ok(()) => {
            state
                .daemon
                .heartbeat_state()
                .pending_embeddings
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (
                StatusCode::CREATED,
                JsonResponse(serde_json::json!({"id": id, "status": "saved"})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            JsonResponse(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}
