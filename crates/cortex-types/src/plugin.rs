use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// Plugin type enum retained for manifest index metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    Tool,
    Llm,
    Memory,
}

/// Plugin manifest — describes a plugin's identity and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub cortex_version: String,
    /// Declared capabilities this plugin provides.
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    /// Native library configuration (if this plugin provides native code).
    #[serde(default)]
    pub native: Option<NativeLibConfig>,

    /// Removed manifest field. If present, compatibility checks reject it.
    #[serde(default)]
    pub cortex_version_requirement: String,
    /// Optional manifest index type. Runtime capability checks use `capabilities`.
    #[serde(default = "default_plugin_type")]
    pub plugin_type: PluginType,
    /// Removed manifest field. Native plugins use `native.entry`.
    #[serde(default = "default_entry_symbol")]
    pub entry_symbol: String,
    /// Optional manifest index dependency list.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Capabilities a plugin can declare.
///
/// List capability names: `"tools"`, `"skills"`, `"prompts"`, `"llm"`, `"memory"`.
///
/// ```toml
/// [capabilities]
/// provides = ["tools", "skills"]
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PluginCapabilities {
    /// Active capability names (e.g. `["tools", "skills", "prompts"]`).
    #[serde(default)]
    pub provides: Vec<String>,
}

impl PluginCapabilities {
    /// Check if a capability is declared.
    #[must_use]
    pub fn has(&self, cap: &str) -> bool {
        self.provides.iter().any(|c| c == cap)
    }

    /// Shorthand: plugin provides native tools.
    #[must_use]
    pub fn tools(&self) -> bool {
        self.has("tools")
    }
    /// Shorthand: plugin provides skill files.
    #[must_use]
    pub fn skills(&self) -> bool {
        self.has("skills")
    }
    /// Shorthand: plugin provides prompt fragments.
    #[must_use]
    pub fn prompts(&self) -> bool {
        self.has("prompts")
    }
    /// Shorthand: plugin provides LLM backend.
    #[must_use]
    pub fn llm(&self) -> bool {
        self.has("llm")
    }
    /// Shorthand: plugin provides memory backend.
    #[must_use]
    pub fn memory(&self) -> bool {
        self.has("memory")
    }
}

/// Native shared library configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NativeLibConfig {
    /// Library filename (relative to plugin directory).
    #[serde(default)]
    pub library: String,
    /// Entry point symbol name.
    #[serde(default = "default_entry_symbol")]
    pub entry: String,
    /// Optional `cortex-sdk` version used to build this native library.
    #[serde(default)]
    pub sdk_version: String,
    /// ABI revision expected by the native in-process boundary.
    #[serde(default = "default_native_abi_revision")]
    pub abi_revision: u32,
    /// Native execution boundary. `process` registers manifest-declared proxy
    /// tools that run outside the daemon process. `trusted_in_process` is an
    /// internal trusted-code boundary and is never the default.
    #[serde(default)]
    pub isolation: NativePluginIsolation,
    /// Tool declarations used when `isolation = "process"`.
    #[serde(default)]
    pub tools: Vec<ProcessToolConfig>,
}

fn default_entry_symbol() -> String {
    String::from("cortex_plugin_create")
}

const fn default_native_abi_revision() -> u32 {
    1
}

/// Execution boundary for native plugin code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativePluginIsolation {
    TrustedInProcess,
    /// Run each declared tool as a child process behind a JSON protocol.
    #[default]
    Process,
}

/// Manifest declaration for one process-isolated tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessToolConfig {
    /// Tool name registered into the Cortex tool registry.
    pub name: String,
    /// Tool description shown to the model.
    pub description: String,
    /// JSON Schema describing accepted input.
    pub input_schema: serde_json::Value,
    /// Executable path, relative to the plugin directory unless absolute.
    pub command: String,
    /// Optional command arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Optional working directory, relative to the plugin directory unless absolute.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Allow command and working directory paths outside the plugin directory.
    ///
    /// Disabled by default so process-isolated plugin manifests cannot point
    /// directly at arbitrary host executables or working directories unless the
    /// operator opts into that trust boundary.
    #[serde(default)]
    pub allow_host_paths: bool,
    /// Host environment variable names allowed through to the process.
    ///
    /// If empty, the runtime supplies a minimal default (`PATH`) for practical
    /// script execution without inheriting the full daemon environment.
    #[serde(default)]
    pub inherit_env: Vec<String>,
    /// Explicit environment variables set for this process tool.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    /// Optional timeout hint in seconds.
    #[serde(default)]
    pub timeout_secs: Option<u64>,
    /// Maximum accepted stdout/stderr bytes. Defaults at runtime when omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_output_bytes: Option<usize>,
    /// Maximum virtual memory bytes for the child process on Unix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_memory_bytes: Option<u64>,
    /// Maximum CPU seconds for the child process on Unix.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_cpu_secs: Option<u64>,
}

const fn default_plugin_type() -> PluginType {
    PluginType::Tool
}

#[derive(Debug, Clone)]
pub struct PluginCompatibility {
    pub compatible: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginIndexEntry {
    pub name: String,
    pub version: String,
    pub description: String,
    pub manifest_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginIndex {
    pub plugins: Vec<PluginIndexEntry>,
}

fn parse_semver(version: &str) -> Option<(u64, u64, u64)> {
    let parts: Vec<&str> = version.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    Some((
        parts[0].parse().ok()?,
        parts[1].parse().ok()?,
        parts[2].parse().ok()?,
    ))
}

/// Check compatibility against the latest manifest `cortex_version` field.
#[must_use]
pub fn check_compatibility(manifest: &PluginManifest, cortex_version: &str) -> PluginCompatibility {
    if !manifest.cortex_version_requirement.trim().is_empty() {
        return PluginCompatibility {
            compatible: false,
            reason: Some("cortex_version_requirement is not supported; use cortex_version".into()),
        };
    }

    let req_str = &manifest.cortex_version;

    if req_str.is_empty() {
        return PluginCompatibility {
            compatible: false,
            reason: Some("cortex_version is required".into()),
        };
    }

    let req = req_str.strip_prefix(">=").unwrap_or(req_str);

    let Some((req_major, req_minor, _)) = parse_semver(req) else {
        return PluginCompatibility {
            compatible: false,
            reason: Some(format!("invalid requirement: {req}")),
        };
    };

    let Some((cur_major, cur_minor, _)) = parse_semver(cortex_version) else {
        return PluginCompatibility {
            compatible: false,
            reason: Some(format!("invalid cortex version: {cortex_version}")),
        };
    };

    if cur_major != req_major {
        return PluginCompatibility {
            compatible: false,
            reason: Some(format!(
                "major version mismatch: {cur_major} vs {req_major}"
            )),
        };
    }

    if cur_minor < req_minor {
        return PluginCompatibility {
            compatible: false,
            reason: Some(format!("minor version too low: {cur_minor} < {req_minor}")),
        };
    }

    PluginCompatibility {
        compatible: true,
        reason: None,
    }
}

impl fmt::Display for PluginType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl PluginManifest {
    /// Create a manifest using the current process-plugin defaults.
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        author: impl Into<String>,
        cortex_version: impl Into<String>,
        plugin_type: PluginType,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
            author: author.into(),
            cortex_version: cortex_version.into(),
            capabilities: PluginCapabilities::default(),
            native: None,
            cortex_version_requirement: String::new(),
            plugin_type,
            entry_symbol: default_entry_symbol(),
            dependencies: Vec::new(),
        }
    }
}

impl PluginIndex {
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<&PluginIndexEntry> {
        self.plugins.iter().find(|p| p.name == name)
    }
}
