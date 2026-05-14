use std::sync::Arc;

use axum::extract::{Json, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;

use super::{
    DaemonState, ForegroundSlotError, HttpState, SlashCommandAction, TracingTurnTracer,
    run_blocking_turn_with_timeout, transport_payloads,
};

#[derive(serde::Deserialize)]
pub(super) struct TurnRequest {
    session_id: String,
    input: String,
    #[serde(default)]
    images: Vec<cortex_types::web::ImageData>,
    #[serde(default)]
    attachments: Vec<cortex_types::Attachment>,
}

pub(super) async fn handle_turn(
    State(state): State<HttpState>,
    Json(req): Json<TurnRequest>,
) -> axum::response::Response {
    let daemon = Arc::clone(&state.daemon);
    let session_id = match resolve_http_session_id(&daemon, req.session_id) {
        Ok(session_id) => session_id,
        Err(response) => return *response,
    };
    let mut input = req.input;
    let inline_images = transport_payloads::images_to_inline(&req.images);
    let attachments = req.attachments;

    if let Err(msg) = transport_payloads::validate_turn_input(&session_id, &input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }

    if let Some(response) = handle_http_slash_command(&daemon, &session_id, &mut input).await {
        return response;
    }

    // Rate limit check BEFORE semaphore -- reject fast without queueing.
    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited =
        daemon.rate_limiter.check(&session_id)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::HeaderName::from_static("retry-after"),
                axum::http::header::HeaderValue::from_static("5"),
            )],
            Json(serde_json::json!({ "error": "rate limit exceeded" })),
        )
            .into_response();
    }

    let result = match run_http_turn(
        daemon,
        session_id.clone(),
        input,
        attachments,
        inline_images,
    )
    .await
    {
        Ok(result) => result,
        Err(response) => return *response,
    };

    match result {
        Ok(output) => {
            let (response, response_format, response_parts) =
                transport_payloads::structured_response_payload_from_output(&output);
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "session_id": session_id,
                    "response": response,
                    "response_format": response_format,
                    "response_parts": response_parts
                })),
            )
                .into_response()
        }
        Err(msg) if msg.contains("rate limit") => (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::HeaderName::from_static("retry-after"),
                axum::http::header::HeaderValue::from_static("5"),
            )],
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
    }
}

pub(super) fn resolve_http_session_id(
    daemon: &Arc<DaemonState>,
    requested_session_id: String,
) -> Result<String, Box<axum::response::Response>> {
    if requested_session_id.trim().is_empty() {
        return Ok(daemon.resolve_client_session("http"));
    }
    if daemon.transport_can_access_session("http", &requested_session_id) {
        Ok(requested_session_id)
    } else {
        Err(Box::new(
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({
                    "error": "session not found or not accessible for this identity"
                })),
            )
                .into_response(),
        ))
    }
}

async fn handle_http_slash_command(
    daemon: &Arc<DaemonState>,
    session_id: &str,
    input: &mut String,
) -> Option<axum::response::Response> {
    if !input.starts_with('/') {
        return None;
    }

    let d = Arc::clone(daemon);
    let cmd_input = input.clone();
    let session_id = session_id.to_string();
    let session_id_for_command = session_id.clone();
    let action = tokio::task::spawn_blocking(move || {
        d.resolve_slash_command_for_session(Some(&session_id_for_command), &cmd_input)
    })
    .await
    .unwrap_or_else(|e| SlashCommandAction::Output(e.to_string()));

    match action {
        SlashCommandAction::Output(cmd_result) => Some(
            (
                StatusCode::OK,
                Json(serde_json::json!({ "session_id": session_id, "response": cmd_result })),
            )
                .into_response(),
        ),
        SlashCommandAction::Prompt(prompt) => {
            *input = prompt;
            None
        }
        SlashCommandAction::NotFound(_) => None,
    }
}

async fn run_http_turn(
    daemon: Arc<DaemonState>,
    session_id: String,
    input: String,
    attachments: Vec<cortex_types::Attachment>,
    inline_images: Vec<(String, String)>,
) -> Result<Result<crate::turn_executor::TurnOutput, String>, Box<axum::response::Response>> {
    let _foreground = match daemon
        .acquire_foreground_execution(std::time::Duration::from_secs(30))
        .await
    {
        Ok(foreground) => foreground,
        Err(err @ ForegroundSlotError::ShuttingDown) => {
            return Err(Box::new(
                (
                    StatusCode::SERVICE_UNAVAILABLE,
                    Json(serde_json::json!({ "error": err.operator_detail() })),
                )
                    .into_response(),
            ));
        }
        Err(err @ ForegroundSlotError::Timeout) => {
            return Err(Box::new(
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    Json(serde_json::json!({ "error": err.operator_detail() })),
                )
                    .into_response(),
            ));
        }
    };

    let sid = session_id.clone();
    let daemon_for_turn = Arc::clone(&daemon);
    let timeout_secs = {
        let cfg = daemon.config();
        cfg.turn.execution_timeout_secs
    };
    let trace_config = daemon.config().turn.trace.clone();
    Ok(
        run_blocking_turn_with_timeout(std::time::Duration::from_secs(timeout_secs), move || {
            let turn_input = crate::turn_executor::TurnInput {
                text: &input,
                attachments: &attachments,
                inline_images: &inline_images,
            };
            daemon_for_turn.execute_turn_streaming(
                &sid,
                &turn_input,
                "http",
                |_| {},
                &TracingTurnTracer {
                    config: trace_config,
                },
            )
        })
        .await,
    )
}
