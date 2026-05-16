use std::path::{Path, PathBuf};

/// Configuration for the daemon server.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// HTTP listen address (from `[daemon].addr` in config.toml).
    pub http_addr: String,
    /// Unix socket path (default: `{home}/cortex.sock`).
    pub socket_path: PathBuf,
    /// Whether to enable stdio transport.
    pub enable_stdio: bool,
}

impl DaemonConfig {
    /// Create config from `CortexConfig` and home directory.
    #[must_use]
    pub fn from_config(config: &cortex_types::config::CortexConfig, home: &Path) -> Self {
        let paths = cortex_kernel::CortexPaths::from_instance_home(home);
        Self {
            http_addr: config.daemon.addr.clone(),
            socket_path: paths.socket_path(),
            enable_stdio: false,
        }
    }

    /// Create default config for the given home directory (random port).
    #[must_use]
    pub fn for_home(home: &Path) -> Self {
        let paths = cortex_kernel::CortexPaths::from_instance_home(home);
        Self {
            http_addr: "127.0.0.1:0".into(),
            socket_path: paths.socket_path(),
            enable_stdio: false,
        }
    }
}
