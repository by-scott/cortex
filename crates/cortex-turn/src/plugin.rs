use std::collections::HashMap;

use cortex_types::{
    Payload, PluginManifest, PluginType, PluginVersionCheck, check_plugin_version,
    plugin::PluginIndex,
};

use crate::llm::LlmClient;
// ── Plugin metadata ─────────────────────────────────────────

/// Runtime metadata for a loaded plugin.
#[derive(Debug, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub plugin_type: PluginType,
}

// ── Plugin traits ───────────────────────────────────────────

/// An LLM plugin provides a custom LLM client implementation.
pub trait LlmPlugin: LlmClient {
    fn plugin_info(&self) -> PluginInfo;
}

/// A memory plugin provides a custom memory storage backend.
pub trait MemoryPlugin: Send + Sync {
    fn plugin_info(&self) -> PluginInfo;

    /// Store a key-value pair.
    ///
    /// # Errors
    /// Returns `MemoryPluginError` if the storage backend fails.
    fn store(&self, key: &str, value: &str) -> Result<(), MemoryPluginError>;

    /// Retrieve a value by key.
    ///
    /// # Errors
    /// Returns `MemoryPluginError` if the retrieval fails.
    fn retrieve(&self, key: &str) -> Result<Option<String>, MemoryPluginError>;

    /// List all stored keys.
    ///
    /// # Errors
    /// Returns `MemoryPluginError` if the listing fails.
    fn list_keys(&self) -> Result<Vec<String>, MemoryPluginError>;

    /// Delete a key-value pair.
    ///
    /// # Errors
    /// Returns `MemoryPluginError` if the deletion fails.
    fn delete(&self, key: &str) -> Result<(), MemoryPluginError>;

    /// Search for matching entries by query.
    ///
    /// # Errors
    /// Returns `MemoryPluginError` if the search fails.
    fn search(&self, query: &str, limit: usize)
    -> Result<Vec<(String, String)>, MemoryPluginError>;
}

/// Error type for memory plugin operations.
#[derive(Debug)]
pub enum MemoryPluginError {
    StorageError(String),
    NotFound(String),
}

impl std::fmt::Display for MemoryPluginError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StorageError(e) => write!(f, "storage error: {e}"),
            Self::NotFound(e) => write!(f, "not found: {e}"),
        }
    }
}

impl std::error::Error for MemoryPluginError {}

// ── Plugin registry (runtime) ───────────────────────────────

struct PluginEntry {
    info: PluginInfo,
}

/// Registry of runtime-loaded plugins.
pub struct PluginRegistry {
    plugins: HashMap<String, PluginEntry>,
}

impl PluginRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            plugins: HashMap::new(),
        }
    }

    /// Register a multi-tool plugin by its info (tools are registered separately).
    pub fn register_tool_info(&mut self, info: &PluginInfo) {
        self.plugins
            .insert(info.name.clone(), PluginEntry { info: info.clone() });
    }

    pub fn register_llm(&mut self, plugin: &dyn LlmPlugin) {
        let info = plugin.plugin_info();
        self.plugins.insert(info.name.clone(), PluginEntry { info });
    }

    pub fn register_memory(&mut self, plugin: &dyn MemoryPlugin) {
        let info = plugin.plugin_info();
        self.plugins.insert(info.name.clone(), PluginEntry { info });
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PluginInfo> {
        self.plugins.get(name).map(|e| &e.info)
    }

    #[must_use]
    pub fn list(&self) -> Vec<&PluginInfo> {
        self.plugins.values().map(|e| &e.info).collect()
    }

    #[must_use]
    pub fn list_by_type(&self, plugin_type: &PluginType) -> Vec<&PluginInfo> {
        self.plugins
            .values()
            .filter(|e| e.info.plugin_type == *plugin_type)
            .map(|e| &e.info)
            .collect()
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.plugins.len()
    }
}

impl Default for PluginRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── Plugin manifest registry ────────────────────────────────

/// Current cortex version used for manifest target checks during registration.
const CORTEX_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Registry that tracks loaded plugin manifests and validates their target version.
pub struct PluginManifestRegistry {
    manifests: HashMap<String, PluginManifest>,
}

impl PluginManifestRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self {
            manifests: HashMap::new(),
        }
    }

    /// Register a plugin manifest. Validates the minimum Cortex version before storing.
    ///
    /// # Errors
    /// Returns an error if the plugin targets a different Cortex version.
    pub fn register(&mut self, manifest: PluginManifest) -> Result<(), String> {
        let version_check = check_plugin_version(&manifest, CORTEX_VERSION);
        if !version_check.accepted {
            return Err(format!(
                "plugin '{}' targets a different cortex version: {}",
                manifest.name,
                version_check.reason.unwrap_or_default()
            ));
        }
        self.manifests.insert(manifest.name.clone(), manifest);
        Ok(())
    }

    /// Look up a manifest by plugin name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&PluginManifest> {
        self.manifests.get(name)
    }

    /// Return all registered manifests.
    #[must_use]
    pub fn list(&self) -> Vec<&PluginManifest> {
        self.manifests.values().collect()
    }

    /// Check all registered manifests against a given Cortex version.
    #[must_use]
    pub fn check_all_versions(&self, cortex_version: &str) -> Vec<(String, PluginVersionCheck)> {
        self.manifests
            .values()
            .map(|m| (m.name.clone(), check_plugin_version(m, cortex_version)))
            .collect()
    }

    /// Fetch a plugin manifest from a remote URL.
    ///
    /// # Errors
    /// Returns an error if the HTTP request fails or the response cannot be parsed.
    pub async fn fetch_manifest(url: &str) -> Result<PluginManifest, String> {
        let response = reqwest::get(url)
            .await
            .map_err(|e| format!("fetch manifest from {url}: {e}"))?;

        if !response.status().is_success() {
            return Err(format!(
                "fetch manifest from {url}: HTTP {}",
                response.status()
            ));
        }

        response
            .json()
            .await
            .map_err(|e| format!("parse manifest from {url}: {e}"))
    }

    /// Install a plugin from an index by name: find in index, fetch manifest,
    /// validate, register.
    ///
    /// # Errors
    /// Returns an error if the plugin is not found, the fetch fails, or the
    /// manifest targets a different Cortex version.
    pub async fn install_from_index(
        &mut self,
        name: &str,
        index: &PluginIndex,
    ) -> Result<Payload, String> {
        let entry = index
            .find_by_name(name)
            .ok_or_else(|| format!("plugin '{name}' not found in index"))?;

        let manifest = Self::fetch_manifest(&entry.manifest_url).await?;

        let version_check = check_plugin_version(&manifest, CORTEX_VERSION);
        if !version_check.accepted {
            return Err(format!(
                "plugin '{}' targets a different cortex version: {}",
                manifest.name,
                version_check.reason.unwrap_or_default()
            ));
        }

        let version = manifest.version.clone();
        let source_url = entry.manifest_url.clone();
        self.manifests.insert(manifest.name.clone(), manifest);

        Ok(Payload::PluginDiscovered {
            name: name.to_string(),
            version,
            source_url,
        })
    }
}

impl Default for PluginManifestRegistry {
    fn default() -> Self {
        Self::new()
    }
}
