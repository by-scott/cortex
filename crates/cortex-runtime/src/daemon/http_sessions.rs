use axum::extract::{Path as PathParam, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};

use super::HttpState;

#[derive(serde::Deserialize)]
pub(super) struct SessionsListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

pub(super) async fn handle_sessions_list(
    State(state): State<HttpState>,
    Query(query): Query<SessionsListQuery>,
) -> impl IntoResponse {
    let all = state.daemon.visible_sessions_for_transport("http");
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);
    let page: Vec<_> = all.into_iter().skip(offset).take(limit).collect();
    (
        StatusCode::OK,
        Json(serde_json::to_value(page).unwrap_or_default()),
    )
}

pub(super) async fn handle_session_get_http(
    State(state): State<HttpState>,
    PathParam(id): PathParam<String>,
) -> axum::response::Response {
    let sessions = state.daemon.visible_sessions_for_transport("http");
    let found = sessions
        .iter()
        .find(|s| s.id.to_string() == id || s.name.as_deref() == Some(&id));
    found.map_or_else(
        || {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        },
        |s| {
            (
                StatusCode::OK,
                Json(serde_json::to_value(s).unwrap_or_default()),
            )
                .into_response()
        },
    )
}

pub(super) async fn handle_session_create(
    State(state): State<HttpState>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    // Accept optional user-supplied session_id
    let user_sid = body.and_then(|Json(v)| v.get("session_id")?.as_str().map(String::from));

    let session_count = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    if session_count >= 10_000 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "maximum session count reached"})),
        );
    }

    if let Some(ref sid) = user_sid {
        if sid.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "session_id must not be empty" })),
            );
        }
        if sid.len() > 256
            || !sid
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid session_id" })),
            );
        }
        // Duplicate IDs/names are checked globally. Visibility filtering still
        // applies to reads, but hidden tenant sessions must not be overwritten.
        if state.daemon.session_id_or_name_exists(sid) {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "session already exists", "session_id": sid })),
            );
        }
        let owner_actor = state.daemon.transport_actor("http");
        let (created_id, meta) = state
            .daemon
            .session_manager()
            .create_session_with_id_for_actor(sid, &owner_actor);
        // Return the user's original ID if it was stored as name (non-UUID input),
        // otherwise return the UUID that was used directly.
        let returned_id = meta
            .name
            .as_deref()
            .unwrap_or(&created_id.to_string())
            .to_string();
        return (
            StatusCode::CREATED,
            Json(serde_json::json!({ "session_id": returned_id })),
        );
    }

    let owner_actor = state.daemon.transport_actor("http");
    let (session_id, _meta) = state
        .daemon
        .session_manager()
        .create_session_for_actor(&owner_actor);
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "session_id": session_id.to_string() })),
    )
}
