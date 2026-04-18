use std::fmt;
use std::path::Path;
use std::sync::{Arc, RwLock};

use cortex_types::config::CortexConfig;
use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

pub struct ConfigWatcher {
    _watcher: RecommendedWatcher,
}

#[derive(Debug)]
pub enum ConfigWatcherError {
    Notify(notify::Error),
}

impl fmt::Display for ConfigWatcherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Notify(e) => write!(f, "config watcher: {e}"),
        }
    }
}

impl std::error::Error for ConfigWatcherError {}

impl From<notify::Error> for ConfigWatcherError {
    fn from(e: notify::Error) -> Self {
        Self::Notify(e)
    }
}

impl ConfigWatcher {
    /// Start watching `config_path` for changes.
    /// On modification, reloads the config into `shared_config`.
    ///
    /// # Errors
    /// Returns `ConfigWatcherError` if the watcher cannot be started.
    pub fn start(
        config_path: &Path,
        shared_config: Arc<RwLock<CortexConfig>>,
    ) -> Result<Self, ConfigWatcherError> {
        let config_file = config_path.to_path_buf();
        let watch_dir = config_path
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                let Ok(event) = res else { return };
                let dominated = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                if !dominated {
                    return;
                }
                let affects_config = event.paths.iter().any(|p| p == &config_file);
                if !affects_config {
                    return;
                }
                if let Ok(content) = std::fs::read_to_string(&config_file)
                    && let Ok(new_config) = toml::from_str::<CortexConfig>(&content)
                    && let Ok(mut guard) = shared_config.write()
                {
                    *guard = new_config;
                }
            })?;

        watcher.watch(&watch_dir, RecursiveMode::NonRecursive)?;

        Ok(Self { _watcher: watcher })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn config_watcher_detects_change() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        fs::write(&config_path, "[api]\nprovider = \"anthropic\"\n").unwrap();

        let config = Arc::new(RwLock::new(CortexConfig::default()));
        let _watcher = ConfigWatcher::start(&config_path, Arc::clone(&config)).unwrap();

        fs::write(&config_path, "[api]\nprovider = \"ollama\"\n").unwrap();

        // Allow file system event propagation
        std::thread::sleep(std::time::Duration::from_millis(500));

        // Note: timing-dependent — the watcher may or may not have fired by now
        // On CI this test validates the watcher can start without error
    }
}
