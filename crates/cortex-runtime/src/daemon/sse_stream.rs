use std::sync::Arc;

use axum::extract::{Json, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use tokio_stream::wrappers::ReceiverStream;

use super::{
    BlockingStreamingTurnRequest, DaemonState, HttpState, SlashCommandAction,
    run_blocking_streaming_turn_with_timeout, transport_payloads,
};

/// Turn tracer that emits to both tracing (stderr) and an SSE channel.
struct SseTurnTracer {
    config: cortex_types::config::TurnTraceConfig,
    tx: tokio::sync::mpsc::Sender<Result<SseEvent, std::convert::Infallible>>,
}

impl cortex_turn::orchestrator::TurnTracer for SseTurnTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat_str = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat_str) < level {
            return;
        }

        tracing::info!(category = cat_str.as_str(), "{message}");

        let payload = serde_json::json!({
            "category": cat_str,
            "level": format!("{level:?}").to_lowercase(),
            "message": message,
        });
        if let Ok(json) = serde_json::to_string(&payload) {
            let event = SseEvent::default().event("trace").data(json);
            let _ = self.tx.try_send(Ok(event));
        }
    }
}

#[derive(serde::Deserialize)]
pub(super) struct TurnStreamRequest {
    session_id: Option<String>,
    input: String,
    #[serde(default)]
    images: Vec<cortex_types::web::ImageData>,
    #[serde(default)]
    attachments: Vec<cortex_types::Attachment>,
}

/// SSE event wrapper for serialization into `data:` fields.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum SsePayload {
    Text {
        content: String,
    },
    Observer {
        source: String,
        content: String,
    },
    Done {
        session_id: String,
        response: String,
        response_format: cortex_types::TextFormat,
        response_parts: Vec<cortex_types::ResponsePart>,
    },
    Error {
        message: String,
    },
}

/// Create an SSE stream that emits a single error event then closes.
async fn sse_error_stream(
    message: String,
) -> Sse<
    axum::response::sse::KeepAliveStream<
        ReceiverStream<Result<SseEvent, std::convert::Infallible>>,
    >,
> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(1);
    let payload = SsePayload::Error { message };
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = tx
            .send(Ok(SseEvent::default().event("error").data(json)))
            .await;
    }
    drop(tx);
    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

pub(super) async fn handle_turn_stream(
    State(state): State<HttpState>,
    Json(req): Json<TurnStreamRequest>,
) -> impl IntoResponse {
    let Ok(session_id) = super::http_turn::resolve_http_session_id(
        &state.daemon,
        req.session_id.unwrap_or_default(),
    ) else {
        return sse_error_stream("session not found or not accessible for this identity".into())
            .await;
    };
    let mut input = req.input;
    let inline_images = transport_payloads::images_to_inline(&req.images);
    let attachments = req.attachments;

    if input.trim().is_empty() {
        return sse_error_stream("input must not be empty".into()).await;
    }
    if let Err(msg) = transport_payloads::validate_session_id(&session_id) {
        return sse_error_stream(msg).await;
    }
    if let Some(response) = resolve_sse_slash_response(&state.daemon, &session_id, &mut input).await
    {
        return response;
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(64);
    spawn_sse_turn_task(
        Arc::clone(&state.daemon),
        session_id,
        input,
        attachments,
        inline_images,
        tx,
    );

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

async fn resolve_sse_slash_response(
    daemon: &Arc<DaemonState>,
    session_id: &str,
    input: &mut String,
) -> Option<
    Sse<
        axum::response::sse::KeepAliveStream<
            ReceiverStream<Result<SseEvent, std::convert::Infallible>>,
        >,
    >,
> {
    if !input.starts_with('/') {
        return None;
    }

    let daemon = Arc::clone(daemon);
    let cmd_input = input.clone();
    let session_id = session_id.to_string();
    let session_id_for_command = session_id.clone();
    let action = tokio::task::spawn_blocking(move || {
        daemon.resolve_slash_command_for_session(Some(&session_id_for_command), &cmd_input)
    })
    .await
    .unwrap_or_else(|e| SlashCommandAction::Output(e.to_string()));
    match action {
        SlashCommandAction::Output(response) => {
            let (tx, rx) =
                tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(1);
            let (response, response_format, response_parts) =
                transport_payloads::structured_response_payload(&response);
            let payload = SsePayload::Done {
                session_id: session_id.clone(),
                response,
                response_format,
                response_parts,
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let _ = tx
                    .send(Ok(SseEvent::default().event("done").data(json)))
                    .await;
            }
            drop(tx);
            Some(Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default()))
        }
        SlashCommandAction::Prompt(prompt) => {
            *input = prompt;
            None
        }
        SlashCommandAction::NotFound(_) => None,
    }
}

fn spawn_sse_turn_task(
    daemon: Arc<DaemonState>,
    session_id: String,
    input: String,
    attachments: Vec<cortex_types::Attachment>,
    inline_images: Vec<(String, String)>,
    tx: tokio::sync::mpsc::Sender<Result<SseEvent, std::convert::Infallible>>,
) {
    tokio::spawn(async move {
        let Ok(_foreground) = daemon
            .acquire_foreground_execution(std::time::Duration::from_secs(30))
            .await
        else {
            return;
        };

        let tx_text = tx.clone();
        let tx_trace = tx.clone();
        let tx_final = tx;
        let sid_for_done = session_id.clone();
        let (timeout_secs, trace_config) = {
            let cfg = daemon.config();
            (cfg.turn.execution_timeout_secs, cfg.turn.trace.clone())
        };

        let result = run_blocking_streaming_turn_with_timeout(BlockingStreamingTurnRequest {
            daemon: Arc::clone(&daemon),
            timeout: std::time::Duration::from_secs(timeout_secs),
            session_id,
            source: "sse",
            input_text: input,
            attachments,
            inline_images,
            tracer: SseTurnTracer {
                config: trace_config,
                tx: tx_trace,
            },
            on_event: Arc::new(move |event| emit_sse_turn_event(event, &tx_text)),
        })
        .await;
        let final_event = sse_final_event(&sid_for_done, result);
        let _ = tx_final.send(Ok(final_event)).await;
    });
}

fn emit_sse_turn_event(
    event: &cortex_turn::orchestrator::TurnStreamEvent,
    tx_text: &tokio::sync::mpsc::Sender<Result<SseEvent, std::convert::Infallible>>,
) {
    match event {
        cortex_turn::orchestrator::TurnStreamEvent::Text {
            lane: cortex_turn::orchestrator::StreamLane::UserVisible,
            content,
            ..
        } => {
            let payload = SsePayload::Text {
                content: content.clone(),
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let event = SseEvent::default().event("text").data(json);
                let _ = tx_text.try_send(Ok(event));
            }
        }
        cortex_turn::orchestrator::TurnStreamEvent::Text {
            lane: cortex_turn::orchestrator::StreamLane::Observer,
            source,
            content,
        } => {
            let payload = SsePayload::Observer {
                source: source.clone().unwrap_or_else(|| "observer".to_string()),
                content: content.clone(),
            };
            if let Ok(json) = serde_json::to_string(&payload) {
                let event = SseEvent::default().event("observer").data(json);
                let _ = tx_text.try_send(Ok(event));
            }
        }
        cortex_turn::orchestrator::TurnStreamEvent::Boundary(_)
        | cortex_turn::orchestrator::TurnStreamEvent::ToolProgress(_) => {}
    }
}

fn sse_final_event(
    session_id: &str,
    result: Result<crate::turn_executor::TurnOutput, String>,
) -> SseEvent {
    match result {
        Ok(output) => {
            let (response, response_format, response_parts) =
                transport_payloads::structured_response_payload_from_output(&output);
            let payload = SsePayload::Done {
                session_id: session_id.to_string(),
                response,
                response_format,
                response_parts,
            };
            let json = serde_json::to_string(&payload).unwrap_or_default();
            SseEvent::default().event("done").data(json)
        }
        Err(message) => {
            let payload = SsePayload::Error { message };
            let json = serde_json::to_string(&payload).unwrap_or_default();
            SseEvent::default().event("error").data(json)
        }
    }
}
