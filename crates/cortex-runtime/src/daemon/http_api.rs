use std::sync::Arc;

use axum::Router;
use axum::extract::Request;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use tower_http::cors::CorsLayer;

use crate::rpc::{self, RpcHandler};

use super::{
    DaemonState, http_memory, http_meta, http_operator, http_rpc, http_sessions, http_turn,
    sse_stream, ws_stream,
};

#[derive(Clone)]
pub(super) struct HttpState {
    pub(super) handler: Arc<RpcHandler>,
    pub(super) daemon: Arc<DaemonState>,
}

pub(super) fn build_state(state: &Arc<DaemonState>) -> HttpState {
    let handler = Arc::new(RpcHandler::new(Arc::clone(state)));
    HttpState {
        handler,
        daemon: Arc::clone(state),
    }
}

pub(super) fn build_router(http_state: HttpState) -> Router<()> {
    use axum::middleware as mw;

    let auth_daemon = Arc::clone(&http_state.daemon);
    let auth_layer = mw::from_fn(move |req: Request, next: Next| {
        let cfg = auth_daemon.config().auth.clone();
        async move { auth_check(cfg, req, next).await }
    });

    let rate_limiter_state = Arc::clone(&http_state.daemon);
    let rate_limit_layer = mw::from_fn(move |req: Request, next: Next| {
        let rl = Arc::clone(&rate_limiter_state);
        async move {
            if req.method() == axum::http::Method::POST {
                // Use would_allow (check-only, no recording) to avoid
                // double-counting: individual handlers record via check().
                let result = rl.rate_limiter.would_allow("__http_global__");
                if result == crate::rate_limiter::RateLimitResult::GlobalLimited {
                    return (
                        StatusCode::TOO_MANY_REQUESTS,
                        [(
                            axum::http::header::HeaderName::from_static("retry-after"),
                            axum::http::header::HeaderValue::from_static("5"),
                        )],
                        Json(serde_json::json!({"error": "rate limit exceeded"})),
                    )
                        .into_response();
                }
            }
            next.run(req).await
        }
    });

    let protected = Router::new()
        .route("/api/sessions", get(http_sessions::handle_sessions_list))
        .route("/api/session", post(http_sessions::handle_session_create))
        .route(
            "/api/session/{id}",
            get(http_sessions::handle_session_get_http),
        )
        .route("/api/turn", post(http_turn::handle_turn))
        .route(
            "/api/memory",
            get(http_memory::handle_memory_list).post(http_memory::handle_memory_save_http),
        )
        .route("/api/meta/alerts", get(http_meta::handle_meta_alerts))
        .route(
            "/api/audit/summary",
            get(http_operator::handle_audit_summary),
        )
        .route("/api/audit/health", get(http_operator::handle_audit_health))
        .route(
            "/api/audit/decision-path/{id}",
            get(http_operator::handle_audit_decision_path),
        )
        .route("/api/rpc", post(http_rpc::handle_http_rpc))
        .route("/api/daemon/status", get(http_operator::handle_http_status))
        .route(
            "/api/operator/dashboard",
            get(http_operator::handle_operator_dashboard),
        )
        .route("/api/turn/stream", post(sse_stream::handle_turn_stream))
        .route("/api/ws", get(handle_ws_upgrade))
        .layer(auth_layer)
        .layer(rate_limit_layer)
        .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024));

    Router::new()
        .route("/api/health", get(http_operator::handle_health))
        .route(
            "/api/metrics/structured",
            get(http_operator::handle_metrics_structured),
        )
        .merge(protected)
        .layer(localhost_cors())
        .layer(axum::middleware::from_fn(reject_non_localhost_preflight))
        .layer(axum::middleware::from_fn(security_headers))
        .fallback(crate::static_assets::serve_embedded_static)
        .with_state(http_state)
}

/// CORS layer: allow only localhost origins with restricted methods/headers.
fn localhost_cors() -> CorsLayer {
    use axum::http::{Method, header};
    use tower_http::cors::AllowOrigin;
    CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(|origin, _| {
            origin.to_str().is_ok_and(|s| {
                s.starts_with("http://localhost")
                    || s.starts_with("http://127.0.0.1")
                    || s.starts_with("https://localhost")
                    || s.starts_with("https://127.0.0.1")
            })
        }))
        .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
}

/// Reject OPTIONS preflight requests from non-localhost origins with 403.
/// This prevents tower-http `CorsLayer` from sending CORS headers for
/// disallowed origins on preflight requests.
async fn reject_non_localhost_preflight(req: Request, next: Next) -> axum::response::Response {
    if req.method() == axum::http::Method::OPTIONS
        && let Some(origin) = req
            .headers()
            .get(axum::http::header::ORIGIN)
            .and_then(|v| v.to_str().ok())
    {
        let is_localhost = origin.starts_with("http://localhost")
            || origin.starts_with("http://127.0.0.1")
            || origin.starts_with("https://localhost")
            || origin.starts_with("https://127.0.0.1");
        if !is_localhost {
            return (StatusCode::FORBIDDEN, "CORS: origin not allowed").into_response();
        }
    }
    next.run(req).await
}

/// Security headers middleware: add standard hardening headers to all responses.
async fn security_headers(req: Request, next: Next) -> axum::response::Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::header::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::header::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("referrer-policy"),
        axum::http::header::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    resp
}

/// Auth check: when `auth.enabled`, require a valid Bearer JWT.
async fn auth_check(
    auth_config: cortex_types::config::AuthConfig,
    req: Request,
    next: Next,
) -> axum::response::Response {
    if !auth_config.enabled || auth_config.secret.is_empty() {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) => {
            let mut validation = jsonwebtoken::Validation::default();
            validation.set_required_spec_claims(&["sub", "exp", "iat"]);
            match jsonwebtoken::decode::<serde_json::Value>(
                t,
                &jsonwebtoken::DecodingKey::from_secret(auth_config.secret.as_bytes()),
                &validation,
            ) {
                Ok(_) => next.run(req).await,
                Err(_) => (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "invalid or expired token"})),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing Authorization header"})),
        )
            .into_response(),
    }
}

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(socket: WebSocket, state: HttpState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let daemon = Arc::clone(&state.daemon);
    let handler = RpcHandler::new(Arc::clone(&daemon));

    while let Some(Ok(msg)) = ws_receiver.next().await {
        let Message::Text(text) = msg else { continue };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let Ok(req) = rpc::parse_request(trimmed) else {
            let err = serde_json::json!({
                "event": "error",
                "data": {"message": "invalid JSON-RPC request"}
            });
            let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
            continue;
        };

        if req.method == "session/prompt" {
            ws_stream::handle_ws_streaming_prompt(&daemon, &mut ws_sender, &req).await;
        } else {
            // Synchronous RPC methods
            let resp = handler.handle_for_client(&req, "ws");
            if let Ok(json) = serde_json::to_string(&resp) {
                let _ = ws_sender.send(Message::Text(json.into())).await;
            }
        }
    }
}
