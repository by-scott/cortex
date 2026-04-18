use serde::{Deserialize, Serialize};
use std::fmt;

/// Legacy plugin type enum.
/// Deprecated: prefer `PluginCapabilities` in the new manifest format.
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

    // ── Legacy fields (kept for backward compatibility) ──
    /// Legacy: version requirement string (e.g. ">=1.0.0").
    /// Prefer `cortex_version` for new manifests.
    #[serde(default)]
    pub cortex_version_requirement: String,
    /// Legacy: single plugin type. Prefer `capabilities` for new manifests.
    #[serde(default = "default_plugin_type")]
    pub plugin_type: PluginType,
    /// Legacy: entry symbol name. Prefer `native.entry` for new manifests.
    #[serde(default = "default_entry_symbol")]
    pub entry_symbol: String,
    /// Legacy: dependency list.
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
    pub library: String,
    /// Entry point symbol name.
    #[serde(default = "default_entry_symbol")]
    pub entry: String,
}

fn default_entry_symbol() -> String {
    String::from("cortex_plugin_create")
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

/// Check compatibility using the legacy `cortex_version_requirement` field.
#[must_use]
pub fn check_compatibility(manifest: &PluginManifest, cortex_version: &str) -> PluginCompatibility {
    let req_str = if manifest.cortex_version_requirement.is_empty() {
        &manifest.cortex_version
    } else {
        &manifest.cortex_version_requirement
    };

    if req_str.is_empty() {
        // No version constraint specified — assume compatible.
        return PluginCompatibility {
            compatible: true,
            reason: None,
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
    /// Create a manifest using legacy fields (backward-compatible constructor).
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        version: impl Into<String>,
        description: impl Into<String>,
        author: impl Into<String>,
        cortex_version_requirement: impl Into<String>,
        plugin_type: PluginType,
    ) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            description: description.into(),
            author: author.into(),
            cortex_version: String::new(),
            capabilities: PluginCapabilities::default(),
            native: None,
            cortex_version_requirement: cortex_version_requirement.into(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compatible_version() {
        let m = PluginManifest::new(
            "test",
            "1.0.0",
            "desc",
            "author",
            ">=2.7.0",
            PluginType::Tool,
        );
        let c = check_compatibility(&m, "2.7.0");
        assert!(c.compatible);
    }

    #[test]
    fn incompatible_major() {
        let m = PluginManifest::new(
            "test",
            "1.0.0",
            "desc",
            "author",
            ">=3.0.0",
            PluginType::Tool,
        );
        let c = check_compatibility(&m, "2.7.0");
        assert!(!c.compatible);
    }

    #[test]
    fn incompatible_minor() {
        let m = PluginManifest::new(
            "test",
            "1.0.0",
            "desc",
            "author",
            ">=2.8.0",
            PluginType::Tool,
        );
        let c = check_compatibility(&m, "2.7.0");
        assert!(!c.compatible);
    }

    #[test]
    fn serde_roundtrip() {
        let m = PluginManifest::new(
            "test",
            "1.0.0",
            "desc",
            "author",
            ">=2.7.0",
            PluginType::Llm,
        );
        let json = serde_json::to_string(&m).unwrap();
        let back: PluginManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "test");
        assert_eq!(back.entry_symbol, "cortex_plugin_create");
    }

    #[test]
    fn new_manifest_capabilities() {
        let toml_str = r#"
name = "my-plugin"
version = "1.0.0"
description = "A test plugin"
cortex_version = "1.4.0"

[capabilities]
provides = ["tools", "skills"]

[native]
library = "libmy_plugin.so"
"#;
        let m: PluginManifest = toml::from_str(toml_str).unwrap();
        assert_eq!(m.name, "my-plugin");
        assert!(m.capabilities.tools());
        assert!(m.capabilities.skills());
        assert!(!m.capabilities.prompts());
        assert!(!m.capabilities.llm());
        assert!(m.native.is_some());
        let native = m.native.unwrap();
        assert_eq!(native.library, "libmy_plugin.so");
        assert_eq!(native.entry, "cortex_plugin_create");
    }

    #[test]
    fn empty_version_requirement_is_compatible() {
        let mut m = PluginManifest::new("test", "1.0.0", "desc", "author", "", PluginType::Tool);
        m.cortex_version = String::new();
        let c = check_compatibility(&m, "2.7.0");
        assert!(c.compatible);
    }
}
