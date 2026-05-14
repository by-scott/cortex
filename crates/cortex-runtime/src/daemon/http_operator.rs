use axum::extract::{Path as PathParam, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};

use super::DaemonState;
use super::http_api::HttpState;

pub(super) async fn handle_http_status(State(state): State<HttpState>) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "operator routes require the local operator identity"}),
            ),
        )
            .into_response();
    }
    let status = state.daemon.status();
    (StatusCode::OK, Json(status)).into_response()
}

#[derive(serde::Deserialize)]
pub(super) struct OperatorDashboardQuery {
    limit: Option<usize>,
}

pub(super) async fn handle_operator_dashboard(
    State(state): State<HttpState>,
    Query(query): Query<OperatorDashboardQuery>,
) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "operator routes require the local operator identity"}),
            ),
        )
            .into_response();
    }
    let dashboard = state.daemon.operator_dashboard(query.limit.unwrap_or(0));
    (StatusCode::OK, Json(dashboard)).into_response()
}

pub(super) async fn handle_health(State(state): State<HttpState>) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "operator routes require the local operator identity"}),
            ),
        )
            .into_response();
    }
    let uptime = chrono::Utc::now()
        .signed_duration_since(state.daemon.start_time())
        .num_seconds();
    let session_count = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "uptime_secs": uptime,
            "session_count": session_count,
        })),
    )
        .into_response()
}

pub(super) async fn handle_metrics_structured(State(state): State<HttpState>) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(
                serde_json::json!({"error": "operator routes require the local operator identity"}),
            ),
        )
            .into_response();
    }
    let live = state.daemon.metrics().snapshot();
    (
        StatusCode::OK,
        Json(serde_json::to_value(&live).unwrap_or_default()),
    )
        .into_response()
}

pub(super) async fn handle_audit_summary(State(state): State<HttpState>) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "audit routes require the local operator identity"})),
        )
            .into_response();
    }
    let events = state
        .daemon
        .journal()
        .recent_events(500)
        .unwrap_or_default();
    let summary = cortex_turn::observability::AuditAggregator::summarize(&events);
    (
        StatusCode::OK,
        Json(serde_json::to_value(summary).unwrap_or_default()),
    )
        .into_response()
}

pub(super) async fn handle_audit_health(State(state): State<HttpState>) -> impl IntoResponse {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "audit routes require the local operator identity"})),
        )
            .into_response();
    }
    let events = state
        .daemon
        .journal()
        .recent_events(500)
        .unwrap_or_default();
    let summary = cortex_turn::observability::AuditAggregator::summarize(&events);

    let health_score = if summary.turn_count == 0 {
        1.0
    } else {
        let alert_ratio = f64::from(u32::try_from(summary.meta_alert_count).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(summary.turn_count).unwrap_or(u32::MAX));
        (1.0 - alert_ratio)
            .clamp(0.0, 1.0)
            .mul_add(0.5, summary.avg_confidence * 0.5)
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "health_score": health_score,
            "total_events": summary.total_events,
            "turn_count": summary.turn_count,
            "tool_call_count": summary.tool_call_count,
            "avg_confidence": summary.avg_confidence,
            "meta_alert_count": summary.meta_alert_count,
        })),
    )
        .into_response()
}

pub(super) async fn handle_audit_decision_path(
    State(state): State<HttpState>,
    PathParam(id): PathParam<String>,
) -> axum::response::Response {
    if state.daemon.transport_actor("http") != DaemonState::local_actor() {
        return (
            StatusCode::FORBIDDEN,
            Json(serde_json::json!({"error": "audit routes require the local operator identity"})),
        )
            .into_response();
    }
    let events = state
        .daemon
        .journal()
        .recent_events(1000)
        .unwrap_or_default();
    let path = cortex_turn::observability::AuditAggregator::extract_decision_path(&events, &id);
    if path.steps.is_empty() && path.outcome.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "decision path not found"})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(path).unwrap_or_default()),
    )
        .into_response()
}
