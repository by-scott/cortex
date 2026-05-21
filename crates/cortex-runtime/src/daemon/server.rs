use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::rpc;
use crate::runtime::CortexRuntime;
use crate::shutdown::{abort_and_join, join_with_grace, shutdown_signal};

use super::{
    DaemonConfig, DaemonState, RpcHandler, bash_completion_queue, cron_scheduler,
    heartbeat_actions, http_api, http_server, line_protocol, post_turn_queue, rpc_batch,
};

/// The daemon server that runs all transports concurrently.
pub struct DaemonServer {
    pub(super) state: Arc<DaemonState>,
    config: DaemonConfig,
}

impl DaemonServer {
    /// Create a new daemon server from a runtime and config.
    ///
    /// # Errors
    ///
    /// Returns an error string if daemon subsystems fail to initialize.
    pub fn new(runtime: &mut CortexRuntime, config: DaemonConfig) -> Result<Self, String> {
        Ok(Self {
            state: Arc::new(DaemonState::from_runtime(runtime)?),
            config,
        })
    }

    /// Run the daemon, starts all configured transports, and blocks until shutdown.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP listener fails to bind.
    pub async fn run(&self) {
        tracing::info!("Starting Cortex daemon...");

        let _hot_reloader =
            crate::hot_reload::HotReloader::start(self.state.home(), Arc::clone(&self.state))
                .map_err(|e| tracing::warn!("Hot-reload watcher failed to start: {e}"))
                .ok();

        let http_handle = self.spawn_http();
        let socket_handle = self.spawn_socket();
        let stdio_handle = if self.config.enable_stdio {
            Some(self.spawn_stdio())
        } else {
            None
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let maintenance_handle =
            self.spawn_heartbeat(Arc::clone(&self.state.heartbeat_state), shutdown_rx.clone());
        let cron_handle = cron_scheduler::spawn(Arc::clone(&self.state), shutdown_rx.clone());
        let bash_completion_handle =
            bash_completion_queue::spawn(Arc::clone(&self.state), shutdown_rx.clone());
        let post_turn_handle = post_turn_queue::spawn(Arc::clone(&self.state), shutdown_rx.clone());

        let channel_handles = self.spawn_channels(&shutdown_rx);

        shutdown_signal().await;

        tracing::info!("Shutting down daemon -- saving sessions...");
        let _ = shutdown_tx.send(true);
        self.state.save_all_sessions();

        let _ = std::fs::remove_file(&self.config.socket_path);

        join_with_grace(
            "heartbeat",
            maintenance_handle,
            std::time::Duration::from_secs(2),
        )
        .await;
        join_with_grace("cron", cron_handle, std::time::Duration::from_secs(2)).await;
        join_with_grace(
            "bash-completion",
            bash_completion_handle,
            std::time::Duration::from_secs(2),
        )
        .await;
        join_with_grace(
            "post-turn",
            post_turn_handle,
            std::time::Duration::from_secs(2),
        )
        .await;
        for (idx, handle) in channel_handles.into_iter().enumerate() {
            join_with_grace("channel", handle, std::time::Duration::from_secs(2)).await;
            tracing::debug!(index = idx, "channel task shutdown completed");
        }

        abort_and_join("http", http_handle).await;
        abort_and_join("socket", socket_handle).await;
        if let Some(handle) = stdio_handle {
            abort_and_join("stdio", handle).await;
        }

        tracing::info!("Daemon stopped.");
    }

    fn spawn_http(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let addr = self.config.http_addr.clone();
        let tls_config = state.config().tls.clone();
        let home_for_tls = self
            .config
            .socket_path
            .parent()
            .map(std::path::Path::to_path_buf);
        let config_path = self
            .config
            .socket_path
            .parent()
            .and_then(std::path::Path::parent)
            .map(|instance_home| {
                cortex_kernel::CortexPaths::from_instance_home(instance_home).config_path()
            });
        state.add_transport("http");

        tokio::spawn(async move {
            let http_state = http_api::build_state(&state);
            let router = http_api::build_router(http_state);

            let addr: std::net::SocketAddr = addr.parse().unwrap_or_else(|e| {
                tracing::error!("Invalid daemon HTTP address: {e}");
                std::net::SocketAddr::from(([127, 0, 0, 1], 0))
            });

            let listener = http_server::bind(addr);
            let actual_addr = listener.local_addr().unwrap_or(addr);
            tracing::info!(addr = %actual_addr, "Daemon HTTP transport listening");

            if addr.port() == 0
                && let Some(ref path) = config_path
            {
                http_server::persist_port_to_config(path, &actual_addr.to_string());
            }

            http_server::serve(listener, router, &tls_config, home_for_tls).await;
        })
    }

    fn spawn_socket(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let socket_path = self.config.socket_path.clone();
        state.add_transport("socket");

        tokio::spawn(async move {
            if socket_path.exists() {
                let _ = std::fs::remove_file(&socket_path);
            }

            let listener = match tokio::net::UnixListener::bind(&socket_path) {
                Ok(listener) => listener,
                Err(e) => {
                    tracing::error!("Failed to bind Unix socket {}: {e}", socket_path.display());
                    return;
                }
            };
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;

                let _ =
                    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o700));
            }
            tracing::info!(path = %socket_path.display(), "Daemon Socket transport listening");

            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    break;
                };
                let handler = RpcHandler::new(Arc::clone(&state));
                let conn_state = Arc::clone(&state);
                tokio::spawn(async move {
                    line_protocol::handle_line_protocol(stream, &handler, &conn_state, "socket")
                        .await;
                });
            }
        })
    }

    fn spawn_stdio(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        state.add_transport("stdio");

        tokio::spawn(async move {
            let handler = RpcHandler::new(Arc::clone(&state));
            let stdin = tokio::io::stdin();
            let mut stdout = tokio::io::stdout();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&line) {
                    let payload = rpc_batch::batch_payload(batch.iter(), |request| {
                        handler.handle_for_client(request, "stdio")
                    });
                    if let Some(json) = payload.and_then(|value| serde_json::to_string(&value).ok())
                    {
                        let _ = stdout.write_all(json.as_bytes()).await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                    continue;
                }

                if let Ok(req) = rpc::parse_request(&line)
                    && req.method == "session/prompt"
                {
                    line_protocol::handle_streaming_prompt(&req, &mut stdout, &state, "stdio")
                        .await;
                    continue;
                }

                let response = match rpc::parse_request(&line) {
                    Ok(req) => handler.handle_for_client(&req, "stdio"),
                    Err(err_resp) => *err_resp,
                };

                if response.id.as_ref().is_some_and(serde_json::Value::is_null)
                    && response.error.is_none()
                {
                    continue;
                }

                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = stdout.write_all(json.as_bytes()).await;
                    let _ = stdout.write_all(b"\n").await;
                    let _ = stdout.flush().await;
                }
            }
        })
    }

    fn spawn_heartbeat(
        &self,
        heartbeat_state: Arc<crate::heartbeat::HeartbeatState>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let hb_state = heartbeat_state;
        tokio::spawn(async move {
            let cfg = state.config().autonomous.clone();
            if !cfg.enabled {
                tracing::info!("Autonomous cognition disabled");
                let _ = shutdown_rx.changed().await;
                return;
            }

            let mut engine = crate::heartbeat::HeartbeatEngine::new(&cfg);
            let mut stability = crate::stability::StabilityMonitor::new();
            let tick_duration = std::time::Duration::from_secs(cfg.heartbeat_interval_secs);
            let mut interval = tokio::time::interval(tick_duration);
            interval.tick().await;

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let actions = engine.tick(&hb_state);
                        for action in &actions {
                            heartbeat_actions::execute(action, &state, &hb_state, &mut stability);
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        tracing::debug!("Heartbeat received shutdown signal");
                        break;
                    }
                }
            }
        })
    }
}
