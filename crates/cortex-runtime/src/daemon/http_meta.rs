use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::Json;

use super::HttpState;

#[derive(serde::Deserialize)]
pub(super) struct AlertsQuery {
    session_id: Option<String>,
}

pub(super) async fn handle_meta_alerts(
    State(state): State<HttpState>,
    Query(query): Query<AlertsQuery>,
) -> impl axum::response::IntoResponse {
    if let Some(ref session_id) = query.session_id
        && !state
            .daemon
            .transport_can_access_session("http", session_id)
    {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "session not found"})),
        );
    }

    let sessions = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let alerts: Vec<serde_json::Value> = query.session_id.as_ref().map_or_else(Vec::new, |sid| {
        sessions
            .get(sid)
            .map(|session| {
                session
                    .monitor
                    .check_with_confidence(0.5)
                    .into_iter()
                    .map(|a| {
                        serde_json::json!({ "kind": format!("{:?}", a.kind), "message": a.message })
                    })
                    .collect()
            })
            .unwrap_or_default()
    });

    (StatusCode::OK, Json(serde_json::json!(alerts)))
}
