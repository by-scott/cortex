use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use cortex_types::config::AutonomousLimits;

use super::DaemonState;

const MIN_SLEEP: Duration = Duration::from_secs(1);
const MAX_SLEEP: Duration = Duration::from_mins(1);
const BUSY_RETRY: Duration = Duration::from_secs(2);

pub(super) fn spawn(
    state: Arc<DaemonState>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        tracing::info!("Cron scheduler started");

        loop {
            let wait = tick(&state);
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                changed = shutdown_rx.changed() => {
                    if changed.is_ok() {
                        tracing::debug!("Cron scheduler received shutdown signal");
                    }
                    break;
                }
            }
        }
    })
}

fn tick(state: &DaemonState) -> Duration {
    let promoted = state.cron_queue().promote_due();
    if promoted > 0 {
        tracing::info!(count = promoted, "Cron scheduler promoted due tasks");
    }

    let Some(task) = state.cron_queue().pending().into_iter().next() else {
        return sleep_until(state.cron_queue().next_wake_at());
    };

    let heartbeat = state.heartbeat_state();
    if heartbeat.foreground_busy.load(Ordering::Relaxed) {
        return BUSY_RETRY;
    }

    let limits = state.config().autonomous.limits;
    if !can_call_autonomous_llm(heartbeat, limits) {
        return llm_retry_after(heartbeat, limits);
    }

    execute_task(state, heartbeat, &task);
    MIN_SLEEP
}

fn execute_task(
    state: &DaemonState,
    heartbeat: &crate::heartbeat::HeartbeatState,
    task: &cortex_turn::tools::cron::CronInvocation,
) {
    tracing::info!(task_id = %task.task_id, due_at = %task.due_at, "Cron scheduler executing task");
    let session_id = format!("cron-{}-{}", task.task_id, chrono::Utc::now().timestamp());

    match state.execute_background_turn(&session_id, &task.prompt, "cron", &[]) {
        Ok(_) => {
            heartbeat.record_llm_call();
            if state.cron_queue().complete(&task.task_id, &task.due_at) {
                tracing::info!(task_id = %task.task_id, "Cron scheduler completed task");
            } else {
                tracing::warn!(task_id = %task.task_id, due_at = %task.due_at, "Cron scheduler completion missing");
            }
        }
        Err(error) => {
            tracing::warn!(task_id = %task.task_id, error = %error, "Cron scheduler task failed");
        }
    }
}

fn can_call_autonomous_llm(
    heartbeat: &crate::heartbeat::HeartbeatState,
    limits: AutonomousLimits,
) -> bool {
    let calls = heartbeat.llm_calls_this_hour.load(Ordering::Relaxed);
    if calls >= limits.max_llm_calls_per_hour {
        return false;
    }

    let last = heartbeat.last_llm_call_secs.load(Ordering::Relaxed);
    if calls == 0 && last == 0 {
        return true;
    }

    heartbeat.elapsed_secs().saturating_sub(last) >= limits.cooldown_after_llm_secs
}

fn llm_retry_after(
    heartbeat: &crate::heartbeat::HeartbeatState,
    limits: AutonomousLimits,
) -> Duration {
    let calls = heartbeat.llm_calls_this_hour.load(Ordering::Relaxed);
    if calls >= limits.max_llm_calls_per_hour {
        return MAX_SLEEP;
    }

    let last = heartbeat.last_llm_call_secs.load(Ordering::Relaxed);
    if calls == 0 && last == 0 {
        return MIN_SLEEP;
    }

    let elapsed = heartbeat.elapsed_secs().saturating_sub(last);
    let remaining = limits.cooldown_after_llm_secs.saturating_sub(elapsed);
    bounded_sleep(remaining)
}

fn sleep_until(next_wake: Option<chrono::DateTime<chrono::Utc>>) -> Duration {
    let Some(next_wake) = next_wake else {
        return MAX_SLEEP;
    };
    let now = chrono::Utc::now();
    if next_wake <= now {
        return MIN_SLEEP;
    }
    bounded_sleep((next_wake - now).num_seconds().try_into().unwrap_or(1))
}

fn bounded_sleep(seconds: u64) -> Duration {
    Duration::from_secs(seconds).clamp(MIN_SLEEP, MAX_SLEEP)
}
