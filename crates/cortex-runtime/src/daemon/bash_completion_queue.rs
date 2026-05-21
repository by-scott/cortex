use std::sync::Arc;
use std::time::Duration;

use super::{DaemonState, InjectMessageResult};

type BashCompletionJob = cortex_turn::tools::bash::BashCompletion;

const IDLE_POLL: Duration = Duration::from_millis(250);

pub(super) fn channel() -> (
    tokio::sync::mpsc::UnboundedSender<BashCompletionJob>,
    tokio::sync::mpsc::UnboundedReceiver<BashCompletionJob>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

pub(super) fn spawn(
    state: Arc<DaemonState>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    let tx = state.bash_completion_sender();
    cortex_turn::tools::bash::set_completion_handler(Some(Arc::new(move |completion| {
        if tx.send(completion).is_err() {
            tracing::warn!("bash completion queue is closed");
        }
    })));

    tokio::spawn(async move {
        tracing::info!("Bash completion queue started");
        let Some(mut rx) = state.take_bash_completion_receiver() else {
            tracing::warn!("Bash completion queue receiver missing");
            cortex_turn::tools::bash::set_completion_handler(None);
            return;
        };

        loop {
            tokio::select! {
                changed = shutdown_rx.changed() => {
                    if changed.is_ok() {
                        tracing::debug!("Bash completion queue received shutdown signal");
                    }
                    break;
                }
                Some(job) = rx.recv() => {
                    let state_for_job = Arc::clone(&state);
                    let shutdown_for_job = shutdown_rx.clone();
                    tokio::spawn(async move {
                        process_job(state_for_job, job, shutdown_for_job).await;
                    });
                }
                else => break,
            }
        }

        cortex_turn::tools::bash::set_completion_handler(None);
    })
}

async fn process_job(
    state: Arc<DaemonState>,
    job: BashCompletionJob,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) {
    let session_id = job
        .invocation
        .session_id
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map_or_else(
            || format!("bash-{}", chrono::Utc::now().timestamp()),
            str::to_string,
        );
    let prompt = completion_prompt(&job);

    if state.has_active_turn(&session_id) {
        match state.inject_message(&session_id, prompt.clone()) {
            InjectMessageResult::Accepted => {
                tracing::info!(
                    process_id = %job.process_id,
                    session_id,
                    "Injected bash completion into active turn"
                );
                return;
            }
            InjectMessageResult::InputClosed | InjectMessageResult::NoActiveTurn => {
                if !wait_for_session_idle(&state, &session_id, &mut shutdown_rx).await {
                    return;
                }
            }
        }
    }

    let source = completion_source(job.invocation.source.as_deref());
    let result = run_background_turn(&state, &job, &session_id, &prompt, &source).await;
    match result {
        Ok(Ok(_)) => tracing::info!(
            process_id = %job.process_id,
            session_id,
            "Processed bash completion in background turn"
        ),
        Ok(Err(error)) => tracing::warn!(
            process_id = %job.process_id,
            session_id,
            error,
            "Bash completion background turn failed"
        ),
        Err(error) => tracing::warn!(
            process_id = %job.process_id,
            session_id,
            error = %error,
            "Bash completion worker failed"
        ),
    }
}

async fn run_background_turn(
    state: &Arc<DaemonState>,
    job: &BashCompletionJob,
    session_id: &str,
    prompt: &str,
    source: &str,
) -> Result<Result<String, String>, tokio::task::JoinError> {
    let state_for_turn = Arc::clone(state);
    let session_for_turn = session_id.to_string();
    let prompt_for_turn = prompt.to_string();
    let source_for_turn = source.to_string();
    let actor_for_turn = job.invocation.actor.clone();
    tokio::task::spawn_blocking(move || {
        actor_for_turn.as_deref().map_or_else(
            || {
                state_for_turn.execute_background_turn(
                    &session_for_turn,
                    &prompt_for_turn,
                    &source_for_turn,
                    &[],
                )
            },
            |actor| {
                state_for_turn.execute_background_turn_for_actor(
                    &session_for_turn,
                    &prompt_for_turn,
                    &source_for_turn,
                    actor,
                    &[],
                )
            },
        )
    })
    .await
}

async fn wait_for_session_idle(
    state: &DaemonState,
    session_id: &str,
    shutdown_rx: &mut tokio::sync::watch::Receiver<bool>,
) -> bool {
    while state.has_active_turn(session_id) {
        tokio::select! {
            changed = shutdown_rx.changed() => {
                return changed.is_ok() && !*shutdown_rx.borrow();
            }
            () = tokio::time::sleep(IDLE_POLL) => {}
        }
    }
    true
}

fn completion_source(source: Option<&str>) -> String {
    source.filter(|value| !value.trim().is_empty()).map_or_else(
        || "bash-background".to_string(),
        |value| format!("bash-background:{value}"),
    )
}

fn completion_prompt(job: &BashCompletionJob) -> String {
    format!(
        "Background bash process completed.\n\n\
         This is asynchronous context from a bash process spawned earlier in this session. \
         Use it to continue the task, run follow-up tools, or notify the user if action is needed.\n\n\
         Process:\n\
         - process_id: {process_id}\n\
         - command: {command}\n\
         - cwd: {cwd}\n\
         - started_at_unix: {started_at}\n\
         - completed_at_unix: {completed_at}\n\
         - success: {success}\n\
         - exit_code: {exit_code}\n\
         - stdout_cursor: {stdout_cursor}\n\
         - stderr_cursor: {stderr_cursor}\n\
         - stdout_truncated: {stdout_truncated}\n\
         - stderr_truncated: {stderr_truncated}\n\n\
         stdout:\n```text\n{stdout}\n```\n\n\
         stderr:\n```text\n{stderr}\n```",
        process_id = job.process_id,
        command = job.command,
        cwd = job.cwd,
        started_at = job.started_at,
        completed_at = job.completed_at,
        success = job.exit.success,
        exit_code = job
            .exit
            .code
            .map_or_else(|| "signal".to_string(), |code| code.to_string()),
        stdout_cursor = job.stdout_cursor,
        stderr_cursor = job.stderr_cursor,
        stdout_truncated = job.stdout_truncated,
        stderr_truncated = job.stderr_truncated,
        stdout = job.stdout.trim_end(),
        stderr = job.stderr.trim_end(),
    )
}
