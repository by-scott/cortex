use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

use crate::{EffectConfirmation, ToolEffect, ToolEffectKind};

/// Plugin type enum retained for manifest index metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PluginType {
    Tool,
    Llm,
    Memory,
}

/// Governance tier assigned to a plugin package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginTrustTier {
    TrustedNative,
    ReviewedProcess,
    #[default]
    UnreviewedProcess,
    Disabled,
    Quarantined,
}

/// Process/container isolation level requested by a plugin package.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginSandboxLevel {
    TrustedInProcess,
    #[default]
    ChildProcess,
    UidNoNetwork,
    SystemSandbox,
    ContainerVm,
    RemoteWorker,
}

/// Network policy attached to a sandbox profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxNetworkMode {
    None,
    Allowlist,
    #[default]
    Inherit,
}

/// Filesystem policy attached to a sandbox profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SandboxFilesystemMode {
    #[default]
    PluginOnly,
    ReadOnlyHost,
    DeclaredPaths,
}

/// Runtime sandbox profile requested by the package.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginSandboxProfile {
    #[serde(default)]
    pub level: PluginSandboxLevel,
    #[serde(default)]
    pub network: SandboxNetworkMode,
    #[serde(default)]
    pub filesystem: SandboxFilesystemMode,
    #[serde(default)]
    pub writable_paths: Vec<String>,
    #[serde(default)]
    pub seccomp: String,
    #[serde(default)]
    pub uid_drop: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_mb: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cpu_seconds: Option<u64>,
}

impl Default for PluginSandboxProfile {
    fn default() -> Self {
        Self {
            level: PluginSandboxLevel::ChildProcess,
            network: SandboxNetworkMode::Inherit,
            filesystem: SandboxFilesystemMode::PluginOnly,
            writable_paths: Vec::new(),
            seccomp: String::new(),
            uid_drop: false,
            memory_mb: None,
            cpu_seconds: None,
        }
    }
}

impl PluginSandboxProfile {
    #[must_use]
    pub fn unsupported_runtime_claim(&self) -> Option<&'static str> {
        match self.level {
            PluginSandboxLevel::TrustedInProcess | PluginSandboxLevel::ChildProcess => {}
            PluginSandboxLevel::UidNoNetwork => {
                return Some("uid_no_network is not enforced by this runtime");
            }
            PluginSandboxLevel::SystemSandbox => {
                return Some("system_sandbox is not enforced by this runtime");
            }
            PluginSandboxLevel::ContainerVm => {
                return Some("container_vm is not enforced by this runtime");
            }
            PluginSandboxLevel::RemoteWorker => {
                return Some("remote_worker isolation is not provided by this runtime");
            }
        }
        if self.uid_drop {
            return Some("uid_drop is not enforced by this runtime");
        }
        if !self.seccomp.trim().is_empty() {
            return Some("seccomp profiles are not enforced by this runtime");
        }
        if self.network == SandboxNetworkMode::None {
            return Some("sandbox.network=none is not kernel-enforced by this runtime");
        }
        None
    }
}

/// Package metadata used by install review, signing, SBOM, and conformance flows.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPackageMetadata {
    #[serde(default)]
    pub publisher_id: String,
    #[serde(default)]
    pub manifest_sha256: String,
    #[serde(default)]
    pub binary_sha256: String,
    #[serde(default)]
    pub signature_algorithm: String,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub signature: String,
    #[serde(default)]
    pub sbom: String,
    #[serde(default)]
    pub risk_profile: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conformance: Option<PluginConformanceCertificate>,
}

/// Persisted plugin conformance certificate.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConformanceCertificate {
    #[serde(default)]
    pub suite: String,
    #[serde(default)]
    pub passed: bool,
    #[serde(default)]
    pub checked_at: String,
    #[serde(default)]
    pub checks: Vec<PluginConformanceCheck>,
}

/// One conformance check result.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginConformanceCheck {
    pub name: String,
    pub passed: bool,
    #[serde(default)]
    pub message: String,
}

/// Plugin manifest — describes a plugin's identity and capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub cortex_version: String,
    /// Governance tier for package review and runtime loading.
    #[serde(default)]
    pub trust: PluginTrustTier,
    /// Declared capabilities this plugin provides.
    #[serde(default)]
    pub capabilities: PluginCapabilities,
    /// Requested process/container sandbox profile.
    #[serde(default)]
    pub sandbox: PluginSandboxProfile,
    /// Optional package metadata. Packed archives may also carry package.toml.
    #[serde(default)]
    pub package: PluginPackageMetadata,
    /// Native library configuration (if this plugin provides native code).
    #[serde(default)]
    pub native: Option<NativeLibConfig>,

    /// Optional manifest index type. Runtime capability checks use `capabilities`.
    #[serde(default = "default_plugin_type")]
    pub plugin_type: PluginType,
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
#[serde(deny_unknown_fields)]
pub struct PluginCapabilities {
    /// Active capability names (e.g. `["tools", "skills", "prompts"]`).
    #[serde(default)]
    pub provides: Vec<String>,
    /// File-read globs requested by plugin tools.
    #[serde(default)]
    pub file_read: Vec<String>,
    /// File-write globs requested by plugin tools.
    #[serde(default)]
    pub file_write: Vec<String>,
    /// Network hosts requested by plugin tools.
    #[serde(default)]
    pub network: Vec<String>,
    /// Whether plugin tools may spawn their own subprocesses.
    #[serde(default)]
    pub process: bool,
    /// Whether plugin tools request access to host secrets or inherited credentials.
    #[serde(default)]
    pub secrets: bool,
    /// Whether plugin tools request background or long-running execution.
    #[serde(default)]
    pub background: bool,
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

    /// Convert declared package capabilities into conservative tool effects.
    #[must_use]
    pub fn declared_effects(&self) -> Vec<ToolEffect> {
        let file_reads = self
            .file_read
            .iter()
            .map(|target| ToolEffect::new(ToolEffectKind::ReadFile).with_target(target.clone()));
        let file_writes = self.file_write.iter().map(|target| {
            ToolEffect::new(ToolEffectKind::WriteFile)
                .with_target(target.clone())
                .with_confirmation(EffectConfirmation::Always)
        });
        let networks = self.network.iter().map(|target| {
            ToolEffect::new(ToolEffectKind::NetworkRequest).with_target(target.clone())
        });
        let process = self.process.then(|| {
            ToolEffect::new(ToolEffectKind::RunProcess)
                .with_target("plugin subprocess")
                .with_confirmation(EffectConfirmation::Always)
        });
        let secrets = self.secrets.then(|| {
            ToolEffect::new(ToolEffectKind::ReadSecret)
                .with_target("host secrets")
                .with_confirmation(EffectConfirmation::Always)
        });
        let background = self.background.then(|| {
            ToolEffect::new(ToolEffectKind::ScheduleTask)
                .with_target("background execution")
                .with_confirmation(EffectConfirmation::Always)
        });

        file_reads
            .chain(file_writes)
            .chain(networks)
            .chain(process)
            .chain(secrets)
            .chain(background)
            .collect()
    }

    /// Human-readable capability request summary.
    #[must_use]
    pub fn requested_summary(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if !self.file_read.is_empty() {
            lines.push(format!("read files: {}", self.file_read.join(", ")));
        }
        if !self.file_write.is_empty() {
            lines.push(format!("write files: {}", self.file_write.join(", ")));
        }
        if !self.network.is_empty() {
            lines.push(format!("network: {}", self.network.join(", ")));
        }
        if self.process {
            lines.push("spawn subprocesses".to_string());
        }
        if self.secrets {
            lines.push("access host secrets".to_string());
        } else {
            lines.push("does not request host secrets".to_string());
        }
        if self.background {
            lines.push("run in background".to_string());
        }
        lines
    }
}

/// Native shared library configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeLibConfig {
    /// Library filename (relative to plugin directory).
    #[serde(default)]
    pub library: String,
    /// Stable native ABI version expected by this native library.
    #[serde(default)]
    pub abi_version: Option<u32>,
    /// Native execution boundary. `process` registers manifest-declared proxy
    /// tools that run outside the daemon process. `trusted_in_process` is an
    /// internal trusted-code boundary and is never the default.
    #[serde(default)]
    pub isolation: NativePluginIsolation,
    /// Tool declarations used when `isolation = "process"`.
    #[serde(default)]
    pub tools: Vec<ProcessToolConfig>,
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
#[serde(deny_unknown_fields)]
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
    /// Tool-specific effects. Plugin-level capabilities are added as a floor.
    #[serde(default)]
    pub effects: Vec<ToolEffect>,
}

const fn default_plugin_type() -> PluginType {
    PluginType::Tool
}

#[derive(Debug, Clone)]
pub struct PluginVersionCheck {
    pub accepted: bool,
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

/// Check the manifest's target Cortex version.
///
/// Cortex accepts plugins that target the exact running Cortex version. Version
/// ranges are intentionally rejected.
#[must_use]
pub fn check_plugin_version(manifest: &PluginManifest, cortex_version: &str) -> PluginVersionCheck {
    let req_str = &manifest.cortex_version;

    if req_str.is_empty() {
        return PluginVersionCheck {
            accepted: false,
            reason: Some("cortex_version is required".into()),
        };
    }

    if req_str
        .chars()
        .any(|ch| matches!(ch, '<' | '>' | '^' | '~' | '*'))
    {
        return PluginVersionCheck {
            accepted: false,
            reason: Some(format!("version ranges are not accepted: {req_str}")),
        };
    }

    let Some(req_version) = parse_semver(req_str) else {
        return PluginVersionCheck {
            accepted: false,
            reason: Some(format!("invalid cortex version target: {req_str}")),
        };
    };

    let Some(current_version) = parse_semver(cortex_version) else {
        return PluginVersionCheck {
            accepted: false,
            reason: Some(format!("invalid cortex version: {cortex_version}")),
        };
    };

    if req_version != current_version {
        return PluginVersionCheck {
            accepted: false,
            reason: Some(format!(
                "cortex_version must match running Cortex exactly: {req_str} != {cortex_version}"
            )),
        };
    }

    PluginVersionCheck {
        accepted: true,
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
            trust: PluginTrustTier::default(),
            capabilities: PluginCapabilities::default(),
            sandbox: PluginSandboxProfile::default(),
            package: PluginPackageMetadata::default(),
            native: None,
            plugin_type,
            dependencies: Vec::new(),
        }
    }

    /// Validate governance fields that are independent of the filesystem.
    ///
    /// # Errors
    /// Returns a deterministic error string when the manifest asks for an
    /// impossible or unsafe trust/sandbox combination.
    pub fn validate_governance(&self) -> Result<(), String> {
        if matches!(
            self.trust,
            PluginTrustTier::Disabled | PluginTrustTier::Quarantined
        ) {
            return Err(format!("plugin '{}' is {:?}", self.name, self.trust));
        }

        let native_isolation = self.native.as_ref().map(|native| native.isolation);
        if native_isolation == Some(NativePluginIsolation::TrustedInProcess)
            && self.trust != PluginTrustTier::TrustedNative
        {
            return Err(format!(
                "plugin '{}' uses trusted_in_process native code but trust is {:?}",
                self.name, self.trust
            ));
        }
        if self.trust == PluginTrustTier::TrustedNative
            && native_isolation != Some(NativePluginIsolation::TrustedInProcess)
        {
            return Err(format!(
                "plugin '{}' declares trusted_native without trusted_in_process native isolation",
                self.name
            ));
        }
        if self.capabilities.secrets && self.trust == PluginTrustTier::UnreviewedProcess {
            return Err(format!(
                "plugin '{}' is unreviewed but requests secrets capability",
                self.name
            ));
        }
        if self.sandbox.network == SandboxNetworkMode::None && !self.capabilities.network.is_empty()
        {
            return Err(format!(
                "plugin '{}' requests network hosts but sandbox.network is none",
                self.name
            ));
        }
        if self.sandbox.level == PluginSandboxLevel::TrustedInProcess
            && native_isolation != Some(NativePluginIsolation::TrustedInProcess)
        {
            return Err(format!(
                "plugin '{}' requests trusted_in_process sandbox without native trusted isolation",
                self.name
            ));
        }
        if let Some(claim) = self.sandbox.unsupported_runtime_claim() {
            return Err(format!(
                "plugin '{}' declares unsupported sandbox enforcement: {claim}",
                self.name
            ));
        }
        Ok(())
    }

    /// Non-fatal governance gaps that should be visible during install review.
    #[must_use]
    pub fn governance_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        if self.package.publisher_id.is_empty() {
            warnings.push("missing publisher_id".to_string());
        }
        if self.package.signature.is_empty() {
            warnings.push("missing package signature".to_string());
        }
        if self.package.sbom.is_empty() {
            warnings.push("missing SBOM reference".to_string());
        }
        if self.package.risk_profile.is_empty() {
            warnings.push("missing recommended risk profile".to_string());
        }
        if !self
            .package
            .conformance
            .as_ref()
            .is_some_and(|certificate| certificate.passed)
        {
            warnings.push("missing passing conformance certificate".to_string());
        }
        warnings
    }
}

impl PluginIndex {
    #[must_use]
    pub fn find_by_name(&self, name: &str) -> Option<&PluginIndexEntry> {
        self.plugins.iter().find(|p| p.name == name)
    }
}
