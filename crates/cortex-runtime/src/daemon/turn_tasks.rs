use std::sync::Arc;

use super::DaemonState;

pub type StreamEventCallback =
    Arc<dyn for<'a> Fn(&'a cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync>;

pub struct BlockingStreamingTurnRequest<Trace> {
    pub daemon: Arc<DaemonState>,
    pub timeout: std::time::Duration,
    pub session_id: String,
    pub source: &'static str,
    pub input_text: String,
    pub attachments: Vec<cortex_types::Attachment>,
    pub inline_images: Vec<(String, String)>,
    pub tracer: Trace,
    pub on_event: StreamEventCallback,
}

pub async fn run_blocking_turn_with_timeout<T: Send + 'static>(
    timeout: std::time::Duration,
    turn: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let task = tokio::task::spawn_blocking(turn);
    if timeout.is_zero() {
        return task.await.unwrap_or_else(|err| Err(err.to_string()));
    }

    tokio::time::timeout(timeout, task).await.map_or_else(
        |_| Err("turn execution timed out".into()),
        |join_result| join_result.unwrap_or_else(|err| Err(err.to_string())),
    )
}

pub async fn run_blocking_streaming_turn_with_timeout<Trace>(
    request: BlockingStreamingTurnRequest<Trace>,
) -> Result<crate::turn_executor::TurnOutput, String>
where
    Trace: cortex_turn::orchestrator::TurnTracer + Send + Sync + 'static,
{
    let BlockingStreamingTurnRequest {
        daemon,
        timeout,
        session_id,
        source,
        input_text,
        attachments,
        inline_images,
        tracer,
        on_event,
    } = request;
    run_blocking_turn_with_timeout(timeout, move || {
        let turn_input = crate::turn_executor::TurnInput {
            text: &input_text,
            attachments: &attachments,
            inline_images: &inline_images,
        };
        daemon.execute_turn_streaming(
            &session_id,
            &turn_input,
            source,
            move |event| on_event(event),
            &tracer,
        )
    })
    .await
}
