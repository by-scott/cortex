use std::path::{Path, PathBuf};

use axum::Router;

/// Serve HTTP with optional TLS.
pub(super) async fn serve(
    listener: tokio::net::TcpListener,
    router: Router<()>,
    tls_config: &cortex_types::config::TlsConfig,
    home_for_tls: Option<PathBuf>,
) {
    if !tls_config.enabled {
        let _ = axum::serve(listener, router).await;
        return;
    }
    let (Some(cert_rel), Some(key_rel)) = (&tls_config.cert_path, &tls_config.key_path) else {
        tracing::error!("TLS enabled but cert_path/key_path not set");
        return;
    };
    let base = home_for_tls.unwrap_or_default();
    let (cert, key) = (base.join(cert_rel), base.join(key_rel));
    match crate::tls::build_server_config(&cert, &key) {
        Ok(tls_cfg) => {
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_cfg);
            tracing::info!("TLS enabled for HTTP transport");
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let service = hyper_util::service::TowerToHyperService::new(router);
                        let _ = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, service)
                        .await;
                    }
                });
            }
        }
        Err(error) => {
            tracing::error!("TLS config failed: {error}, falling back to plain HTTP");
            let _ = axum::serve(listener, router).await;
        }
    }
}

pub(super) fn bind(addr: std::net::SocketAddr) -> tokio::net::TcpListener {
    // SO_REUSEADDR: allow immediate rebind after daemon restart.
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap_or_else(|error| {
        tracing::error!("Failed to create socket: {error}");
        std::process::exit(1);
    });
    socket.set_reuse_address(true).ok();
    socket.set_nonblocking(true).ok();
    socket.bind(&addr.into()).unwrap_or_else(|error| {
        tracing::error!("Failed to bind {addr}: {error}");
        std::process::exit(1);
    });
    socket.listen(128).unwrap_or_else(|error| {
        tracing::error!("Failed to listen: {error}");
        std::process::exit(1);
    });
    tokio::net::TcpListener::from_std(socket.into()).unwrap_or_else(|error| {
        tracing::error!("Failed to convert listener: {error}");
        std::process::exit(1);
    })
}

/// Persist port to config.toml using line-level replacement to preserve
/// comments and field ordering.
pub(super) fn persist_port_to_config(config_path: &Path, actual_addr: &str) {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return;
    };
    let addr_line = format!("addr = \"{actual_addr}\"");

    // Try to replace existing addr line under [daemon].
    let mut in_daemon = false;
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.trim().starts_with("[daemon]") {
            in_daemon = true;
        } else if line.trim().starts_with('[') && !line.trim().starts_with("[daemon") {
            in_daemon = false;
        }
        if in_daemon && line.trim().starts_with("addr") {
            lines.push(addr_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        // Append [daemon] section if missing.
        lines.push(String::new());
        lines.push("[daemon]".to_string());
        lines.push(addr_line);
    }

    let _ = cortex_kernel::atomic_write_text(config_path, lines.join("\n"));
    tracing::info!(addr = actual_addr, "Port persisted to config.toml");
}
