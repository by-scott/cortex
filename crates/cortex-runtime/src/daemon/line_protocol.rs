use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::rpc;

use super::{
    BlockingStreamingTurnRequest, ChannelTurnTracer, DaemonState, ForegroundSlotError, RpcHandler,
    batch_payload, rpc_param_attachments, rpc_param_images,
    run_blocking_streaming_turn_with_timeout, structured_response_payload_from_output,
    validate_session_id,
};

pub(super) async fn handle_line_protocol<S>(
    stream: S,
    handler: &RpcHandler,
    state: &Arc<DaemonState>,
    source: &str,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    // 64 KB buffer handles large prompts (e.g. multi-KB Chinese text).
    let buf_reader = BufReader::with_capacity(64 * 1024, reader);
    let mut lines = buf_reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // handler.handle() uses block_in_place internally, which requires
        // running on a tokio worker thread (not spawn_blocking's thread pool).
        // Tool execution itself runs in scoped OS threads, so this won't block.

        // Try batch (JSON array) first
        if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&line) {
            let payload = batch_payload(batch.iter(), |request| {
                handler.handle_for_client(request, source)
            });
            if let Some(json) = payload.and_then(|value| serde_json::to_string(&value).ok()) {
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.flush().await;
            }
            continue;
        }

        // Intercept session/prompt for streaming event delivery.
        if let Ok(req) = rpc::parse_request(&line)
            && req.method == "session/prompt"
        {
            handle_streaming_prompt(&req, &mut writer, state, source).await;
            continue;
        }

        let response = match rpc::parse_request(&line) {
            Ok(req) => handler.handle_for_client(&req, source),
            Err(err_resp) => *err_resp,
        };

        // JSON-RPC 2.0: notifications (null id) must not receive a response.
        if response.id.as_ref().is_some_and(serde_json::Value::is_null) && response.error.is_none()
        {
            continue;
        }

        if let Ok(json) = serde_json::to_string(&response) {
            let _ = writer.write_all(json.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
            let _ = writer.flush().await;
        }
    }
}

/// Handle `session/prompt` with streaming events (shared by socket and stdio).
///
/// Emits NDJSON event lines (`text`, `tool`, `trace`) as the turn
/// executes, finishing with a `done` or `error` event.
pub(super) async fn handle_streaming_prompt<W>(
    req: &rpc::RpcRequest,
    writer: &mut W,
    state: &Arc<DaemonState>,
    source: &str,
) where
    W: tokio::io::AsyncWrite + Unpin,
{
    let prompt = req
        .params
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let attachments = rpc_param_attachments(&req.params);
    let inline_images = rpc_param_images(&req.params);

    if prompt.trim().is_empty() {
        write_error_event(writer, "missing prompt parameter").await;
        return;
    }

    // Resolve session id (use provided, or generate a new one).
    let session_id = req
        .params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|sid| !sid.trim().is_empty())
        .map_or_else(|| state.resolve_client_session(source), String::from);

    if !state.transport_can_access_session(source, &session_id) {
        write_error_event(
            writer,
            "session not found or not accessible for this identity",
        )
        .await;
        return;
    }

    if let Err(msg) = validate_session_id(&session_id) {
        write_error_event(writer, &msg).await;
        return;
    }

    // Rate limit check before queueing on the semaphore.
    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited = state.rate_limiter.check(&session_id)
    {
        write_error_event(writer, "rate limit exceeded").await;
        return;
    }

    // Serialize foreground turns (GWT: one task at a time).
    let _foreground = match state
        .acquire_foreground_execution(std::time::Duration::from_secs(30))
        .await
    {
        Ok(foreground) => foreground,
        Err(err @ (ForegroundSlotError::ShuttingDown | ForegroundSlotError::Timeout)) => {
            write_error_event(writer, err.operator_detail()).await;
            return;
        }
    };

    let final_result = execute_streaming_turn(
        state,
        &session_id,
        prompt,
        attachments,
        inline_images,
        writer,
    )
    .await;

    // Send the final done or error event.
    let done_event = match final_result {
        Ok(output) => {
            let (response, response_format, response_parts) =
                structured_response_payload_from_output(&output);
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
    let _ = writer.write_all(done_event.to_string().as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.flush().await;
}

/// Write an NDJSON error event and flush.
async fn write_error_event<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, message: &str) {
    let evt = serde_json::json!({"event":"error","data":{"message": message}});
    let _ = writer.write_all(evt.to_string().as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.flush().await;
}

/// Spawn the turn in a blocking thread and stream events to the writer.
///
/// Returns the final turn response or an error message.
async fn execute_streaming_turn<W>(
    state: &Arc<DaemonState>,
    session_id: &str,
    prompt: &str,
    attachments: Vec<cortex_types::Attachment>,
    inline_images: Vec<(String, String)>,
    writer: &mut W,
) -> Result<crate::turn_executor::TurnOutput, String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);
    let join = spawn_socket_streaming_turn(
        state,
        session_id,
        prompt,
        attachments,
        inline_images,
        tx.clone(),
    );

    // Drop the original sender so the channel closes when spawn_blocking finishes.
    drop(tx);

    // Stream events from channel to the writer concurrently with the join handle.
    tokio::pin!(join);
    let mut join_done = false;
    let mut final_result: Option<Result<crate::turn_executor::TurnOutput, String>> = None;

    loop {
        if join_done && final_result.is_some() {
            // Drain any remaining events.
            while let Ok(line) = rx.try_recv() {
                write_stream_line(writer, &line).await;
            }
            let _ = writer.flush().await;
            break;
        }

        tokio::select! {
            biased;
            Some(line) = rx.recv() => {
                write_stream_line(writer, &line).await;
                let _ = writer.flush().await;
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

fn spawn_socket_streaming_turn(
    state: &Arc<DaemonState>,
    session_id: &str,
    prompt: &str,
    attachments: Vec<cortex_types::Attachment>,
    inline_images: Vec<(String, String)>,
    tx: tokio::sync::mpsc::Sender<String>,
) -> impl std::future::Future<Output = Result<crate::turn_executor::TurnOutput, String>> {
    let sid = session_id.to_string();
    let prompt_text = prompt.to_string();
    let tx_trace = tx.clone();

    let (timeout_secs, trace_config) = {
        let cfg = state.config();
        (cfg.turn.execution_timeout_secs, cfg.turn.trace.clone())
    };

    run_blocking_streaming_turn_with_timeout(BlockingStreamingTurnRequest {
        daemon: Arc::clone(state),
        timeout: std::time::Duration::from_secs(timeout_secs),
        session_id: sid,
        source: "socket",
        input_text: prompt_text,
        attachments,
        inline_images,
        tracer: ChannelTurnTracer {
            config: trace_config,
            tx: tx_trace,
        },
        on_event: Arc::new(move |event| {
            if let Some(json) = encode_socket_stream_event(event) {
                let _ = tx.try_send(json);
            }
        }),
    })
}

fn encode_socket_stream_event(
    event: &cortex_turn::orchestrator::TurnStreamEvent,
) -> Option<String> {
    super::encode_json_stream_event(event).map(|(_, json)| json)
}

async fn write_stream_line<W>(writer: &mut W, line: &str)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let _ = writer.write_all(line.as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
}
