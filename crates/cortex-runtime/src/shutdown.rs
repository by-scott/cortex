use std::time::Duration;

pub async fn join_with_grace(
    task: &'static str,
    mut handle: tokio::task::JoinHandle<()>,
    grace: Duration,
) {
    tokio::select! {
        result = &mut handle => log_join_result(task, result),
        () = tokio::time::sleep(grace) => {
            tracing::debug!(task, "daemon task did not stop before grace period; aborting");
            handle.abort();
            abort_joined(task, handle).await;
        }
    }
}

pub async fn abort_and_join(task: &'static str, handle: tokio::task::JoinHandle<()>) {
    handle.abort();
    abort_joined(task, handle).await;
}

async fn abort_joined(task: &'static str, handle: tokio::task::JoinHandle<()>) {
    if let Ok(result) = tokio::time::timeout(Duration::from_millis(500), handle).await {
        log_join_result(task, result);
    } else {
        tracing::warn!(task, "daemon task did not join after abort");
    }
}

fn log_join_result(task: &'static str, result: Result<(), tokio::task::JoinError>) {
    match result {
        Ok(()) => tracing::debug!(task, "daemon task stopped"),
        Err(err) if err.is_cancelled() => tracing::debug!(task, "daemon task aborted"),
        Err(err) => tracing::warn!(task, error = %err, "daemon task failed during shutdown"),
    }
}

pub async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut sigterm =
            match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
                Ok(signal) => signal,
                Err(err) => {
                    tracing::error!("failed to install SIGTERM handler: {err}");
                    return;
                }
            };
        let mut sighup = match tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
        {
            Ok(signal) => signal,
            Err(err) => {
                tracing::error!("failed to install SIGHUP handler: {err}");
                return;
            }
        };
        loop {
            tokio::select! {
                _ = &mut ctrl_c => { tracing::info!("Received SIGINT"); break; }
                _ = sigterm.recv() => { tracing::info!("Received SIGTERM"); break; }
                _ = sighup.recv() => {
                    tracing::info!("Received SIGHUP -- ignored (config reload via file watcher)");
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        ctrl_c.await.ok();
        tracing::info!("Received Ctrl+C");
    }
}
