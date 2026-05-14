use std::sync::Arc;

use axum::extract::ws::{Message, WebSocket};
use futures_util::SinkExt;

use crate::rpc;

use super::{
    BlockingStreamingTurnRequest, ChannelTurnTracer, DaemonState, ForegroundSlotError,
    run_blocking_streaming_turn_with_timeout, transport_payloads,
};

/// Handle `session/prompt` over WebSocket with streaming events.
///
/// Emits the same NDJSON event format (`text`, `tool`, `trace`, `done`,
/// `error`) as the socket/stdio transports, each as a separate WebSocket
/// text message.
pub(super) async fn handle_ws_streaming_prompt(
    daemon: &Arc<DaemonState>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    req: &rpc::RpcRequest,
) {
    let prompt = req
        .params
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let attachments = transport_payloads::rpc_param_attachments(&req.params);
    let inline_images = transport_payloads::rpc_param_images(&req.params);

    if prompt.trim().is_empty() {
        ws_send_error(ws_sender, "missing prompt").await;
        return;
    }

    let session_id = req
        .params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|sid| !sid.trim().is_empty())
        .map_or_else(|| daemon.resolve_client_session("ws"), String::from);

    if !daemon.transport_can_access_session("ws", &session_id) {
        ws_send_error(
            ws_sender,
            "session not found or not accessible for this identity",
        )
        .await;
        return;
    }

    if let Err(msg) = transport_payloads::validate_session_id(&session_id) {
        ws_send_error(ws_sender, &msg).await;
        return;
    }

    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited =
        daemon.rate_limiter.check(&session_id)
    {
        ws_send_error(ws_sender, "rate limit exceeded").await;
        return;
    }

    let _foreground = match daemon
        .acquire_foreground_execution(std::time::Duration::from_secs(30))
        .await
    {
        Ok(foreground) => foreground,
        Err(err @ (ForegroundSlotError::ShuttingDown | ForegroundSlotError::Timeout)) => {
            ws_send_error(ws_sender, err.operator_detail()).await;
            return;
        }
    };

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    let result = execute_ws_turn(
        BlockingWsTurnRequest {
            daemon,
            session_id: &session_id,
            prompt,
            attachments,
            inline_images,
            tx,
            rx: &mut rx,
        },
        ws_sender,
    )
    .await;

    let done_event = match result {
        Ok(output) => {
            let (response, response_format, response_parts) =
                transport_payloads::structured_response_payload_from_output(&output);
            serde_json::json!({
                "event": "done",
                "data": {
                    "session_id": session_id,
                    "response": response,
                    "response_format": response_format,
                    "response_parts": response_parts
                }
            })
        }
        Err(msg) => serde_json::json!({
            "event": "error",
            "data": {"message": msg}
        }),
    };
    let _ = ws_sender
        .send(Message::Text(done_event.to_string().into()))
        .await;
}

/// Execute a streaming turn and pipe events through a channel to a WebSocket.
async fn execute_ws_turn(
    request: BlockingWsTurnRequest<'_>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> Result<crate::turn_executor::TurnOutput, String> {
    let BlockingWsTurnRequest {
        daemon,
        session_id,
        prompt,
        attachments,
        inline_images,
        tx,
        rx,
    } = request;
    let sid = session_id.to_string();
    let prompt_text = prompt.to_string();
    let tx_text = tx.clone();
    let tx_trace = tx.clone();

    let (timeout_secs, trace_config) = {
        let cfg = daemon.config();
        (cfg.turn.execution_timeout_secs, cfg.turn.trace.clone())
    };

    let join = run_blocking_streaming_turn_with_timeout(BlockingStreamingTurnRequest {
        daemon: Arc::clone(daemon),
        timeout: std::time::Duration::from_secs(timeout_secs),
        session_id: sid,
        source: "ws",
        input_text: prompt_text,
        attachments,
        inline_images,
        tracer: ChannelTurnTracer {
            config: trace_config,
            tx: tx_trace,
        },
        on_event: Arc::new(move |event| {
            if let Some((_, json)) = transport_payloads::encode_json_stream_event(event) {
                let _ = tx_text.try_send(json);
            }
        }),
    });

    drop(tx);

    tokio::pin!(join);
    let mut join_done = false;
    let mut final_result: Option<Result<crate::turn_executor::TurnOutput, String>> = None;

    loop {
        if join_done && final_result.is_some() {
            while let Ok(line) = rx.try_recv() {
                let _ = ws_sender.send(Message::Text(line.into())).await;
            }
            break;
        }
        tokio::select! {
            biased;
            Some(line) = rx.recv() => {
                let _ = ws_sender.send(Message::Text(line.into())).await;
            }
            result = &mut join, if !join_done => {
                join_done = true;
                final_result = Some(result);
            }
            else => break,
        }
    }

    final_result.unwrap_or_else(|| Err("unexpected end".into()))
}

struct BlockingWsTurnRequest<'a> {
    daemon: &'a Arc<DaemonState>,
    session_id: &'a str,
    prompt: &'a str,
    attachments: Vec<cortex_types::Attachment>,
    inline_images: Vec<(String, String)>,
    tx: tokio::sync::mpsc::Sender<String>,
    rx: &'a mut tokio::sync::mpsc::Receiver<String>,
}

async fn ws_send_error(
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &str,
) {
    let err = serde_json::json!({"event":"error","data":{"message":message}});
    let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
}
