//! Unified hot-reload for all externalized files.
//!
//! Uses `notify` file watcher to detect changes in real-time.
//! Monitored: config.toml, providers.toml, mcp.toml, prompts/, skills/, plugins/.
//!
//! Recovery policy:
//! - Structural files (config.toml, providers.toml, directories): restored on deletion
//! - Content files (prompts/*.md, skills/*/SKILL.md): warn on deletion, do NOT restore

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};

/// Callback invoked when externalized files change or are deleted.
pub trait ReloadTarget: Send + Sync + 'static {
    /// Called when config.toml, providers.toml, or mcp.toml is modified.
    /// Implementation should: parse new content, keep old config on parse failure.
    fn reload_config(&self);
    /// Called when config.toml, providers.toml, or mcp.toml is deleted.
    /// Implementation should: restore default file, then reload.
    fn restore_config(&self);
    /// Called when prompt files are modified.
    fn reload_prompts(&self);
    /// Called when prompt files are deleted (warn only, do NOT restore).
    fn on_prompt_deleted(&self, path: &Path);
    /// Called when skill files are modified.
    fn reload_skills(&self);
    /// Called when skill files are deleted (warn only, do NOT restore).
    fn on_skill_deleted(&self, path: &Path);
    /// Called when plugin manifests or package content changes.
    fn on_plugins_changed(&self, path: &Path);
}

/// Watches all externalized files and triggers reload on changes.
pub struct HotReloader {
    _watcher: RecommendedWatcher,
}

/// Error from hot-reload watcher setup.
#[derive(Debug)]
pub struct HotReloadError(pub String);

impl std::fmt::Display for HotReloadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for HotReloadError {}

impl HotReloader {
    /// Start watching all externalized file directories.
    ///
    /// # Errors
    ///
    /// Returns an error if the file watcher cannot be initialized.
    pub fn start<T: ReloadTarget>(home: &Path, target: Arc<T>) -> Result<Self, HotReloadError> {
        let paths = cortex_kernel::CortexPaths::from_instance_home(home);
        let files = paths.config_files();
        let prompts_dir = paths.prompts_dir();
        let skills_dir = paths.skills_dir();
        let plugins_dir = paths.instance_home().join("plugins");
        let mcp_match = files.mcp;

        let config_match = files.config;
        let providers_match = files.providers;
        let prompts_match = prompts_dir.clone();
        let skills_match = skills_dir.clone();
        let plugins_match = plugins_dir.clone();

        // Debounce: track last reload time as millis since UNIX epoch.
        let last_reload_ms = Arc::new(AtomicU64::new(0));

        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, notify::Error>| {
                let Ok(event) = res else { return };

                let is_modify = matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_));
                let is_remove = matches!(event.kind, EventKind::Remove(_));

                if !is_modify && !is_remove {
                    return;
                }

                // Debounce: ignore events within 500ms of last reload.
                let now_ms = SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
                let prev = last_reload_ms.load(Ordering::Relaxed);
                if now_ms.saturating_sub(prev) < 500 {
                    return;
                }
                last_reload_ms.store(now_ms, Ordering::Relaxed);

                for path in &event.paths {
                    if *path == config_match || *path == providers_match || *path == mcp_match {
                        if is_remove {
                            target.restore_config();
                            tracing::warn!("Hot-reload: config deleted, restored default");
                        } else {
                            target.reload_config();
                            tracing::debug!("Hot-reload: config reloaded");
                        }
                    } else if path.starts_with(&prompts_match) {
                        if is_remove {
                            target.on_prompt_deleted(path);
                        } else {
                            target.reload_prompts();
                            tracing::debug!("Hot-reload: prompts reloaded");
                        }
                    } else if path.starts_with(&skills_match) {
                        if is_remove {
                            target.on_skill_deleted(path);
                        } else {
                            target.reload_skills();
                            tracing::debug!("Hot-reload: skills reloaded");
                        }
                    } else if path.starts_with(&plugins_match) {
                        target.on_plugins_changed(path);
                    }
                }
            })
            .map_err(|e| HotReloadError(format!("watcher init: {e}")))?;

        // Watch home directory (non-recursive) for config.toml / providers.toml
        watcher
            .watch(home, RecursiveMode::NonRecursive)
            .map_err(|e| HotReloadError(format!("watch home: {e}")))?;

        if prompts_dir.exists() {
            watcher
                .watch(&prompts_dir, RecursiveMode::Recursive)
                .map_err(|e| HotReloadError(format!("watch prompts: {e}")))?;
        }

        if skills_dir.exists() {
            watcher
                .watch(&skills_dir, RecursiveMode::Recursive)
                .map_err(|e| HotReloadError(format!("watch skills: {e}")))?;
        }

        if plugins_dir.exists() {
            watcher
                .watch(&plugins_dir, RecursiveMode::Recursive)
                .map_err(|e| HotReloadError(format!("watch plugins: {e}")))?;
        }

        let _ = watcher.watch(paths.base_dir(), RecursiveMode::NonRecursive);

        tracing::info!("Hot-reload watcher started (config + prompts + skills + plugins)");

        Ok(Self { _watcher: watcher })
    }
}
