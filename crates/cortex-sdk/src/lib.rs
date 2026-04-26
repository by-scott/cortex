#![forbid(unsafe_code)]

use std::path::{Component, Path};

use serde::{Deserialize, Serialize};

pub const ABI_VERSION: u32 = 2;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginContext {
    pub tenant_id: String,
    pub actor_id: String,
    pub session_id: String,
    pub capabilities: Vec<String>,
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub timeout_ms: u64,
    pub max_output_bytes: usize,
    pub max_memory_bytes: usize,
    pub allow_host_paths: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolRequest {
    pub name: String,
    pub input: serde_json::Value,
    pub required_capabilities: Vec<String>,
    pub host_paths: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResponse {
    pub output: serde_json::Value,
    pub audit_label: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginBoundary {
    Process,
    Native,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PluginManifest {
    pub name: String,
    pub version: String,
    pub abi_version: u32,
    pub boundary: PluginBoundary,
    pub capabilities: Vec<String>,
    pub limits: ResourceLimits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginAuthorizationError {
    AbiMismatch { expected: u32, actual: u32 },
    CapabilityNotDeclared { capability: String },
    EmptyManifestField { field: &'static str },
    HostPathDenied { path: String },
    MissingCapability { capability: String },
    OutputTooLarge { actual: usize, limit: usize },
}

impl ResourceLimits {
    #[must_use]
    pub const fn strict() -> Self {
        Self {
            timeout_ms: 5_000,
            max_output_bytes: 64 * 1024,
            max_memory_bytes: 64 * 1024 * 1024,
            allow_host_paths: false,
        }
    }
}

impl PluginContext {
    #[must_use]
    pub fn has_capability(&self, capability: &str) -> bool {
        self.capabilities
            .iter()
            .any(|candidate| candidate == capability)
    }

    /// # Errors
    /// Returns an error when a request asks for capabilities or host paths not
    /// granted by this context.
    pub fn authorize(&self, request: &ToolRequest) -> Result<(), PluginAuthorizationError> {
        for capability in &request.required_capabilities {
            if !self.has_capability(capability) {
                return Err(PluginAuthorizationError::MissingCapability {
                    capability: capability.clone(),
                });
            }
        }
        if !self.limits.allow_host_paths {
            for path in &request.host_paths {
                if is_host_path(path) {
                    return Err(PluginAuthorizationError::HostPathDenied { path: path.clone() });
                }
            }
        }
        Ok(())
    }
}

impl ToolRequest {
    #[must_use]
    pub fn new(name: impl Into<String>, input: serde_json::Value) -> Self {
        Self {
            name: name.into(),
            input,
            required_capabilities: Vec::new(),
            host_paths: Vec::new(),
        }
    }

    #[must_use]
    pub fn require_capability(mut self, capability: impl Into<String>) -> Self {
        self.required_capabilities.push(capability.into());
        self
    }

    #[must_use]
    pub fn with_host_path(mut self, path: impl Into<String>) -> Self {
        self.host_paths.push(path.into());
        self
    }
}

impl ToolResponse {
    /// # Errors
    /// Returns an error when the response exceeds the host output limit.
    pub fn validate_output(&self, limits: ResourceLimits) -> Result<(), PluginAuthorizationError> {
        let actual = self.output.to_string().len() + self.audit_label.len();
        if actual > limits.max_output_bytes {
            Err(PluginAuthorizationError::OutputTooLarge {
                actual,
                limit: limits.max_output_bytes,
            })
        } else {
            Ok(())
        }
    }
}

impl PluginManifest {
    #[must_use]
    pub fn process(name: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: version.into(),
            abi_version: ABI_VERSION,
            boundary: PluginBoundary::Process,
            capabilities: Vec::new(),
            limits: ResourceLimits::strict(),
        }
    }

    #[must_use]
    pub fn with_capability(mut self, capability: impl Into<String>) -> Self {
        self.capabilities.push(capability.into());
        self.capabilities.sort();
        self.capabilities.dedup();
        self
    }

    #[must_use]
    pub const fn with_limits(mut self, limits: ResourceLimits) -> Self {
        self.limits = limits;
        self
    }

    /// # Errors
    /// Returns an error when the manifest is empty or targets another ABI.
    pub const fn validate(&self) -> Result<(), PluginAuthorizationError> {
        if self.name.is_empty() {
            return Err(PluginAuthorizationError::EmptyManifestField { field: "name" });
        }
        if self.version.is_empty() {
            return Err(PluginAuthorizationError::EmptyManifestField { field: "version" });
        }
        if self.abi_version != ABI_VERSION {
            return Err(PluginAuthorizationError::AbiMismatch {
                expected: ABI_VERSION,
                actual: self.abi_version,
            });
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error when the manifest is invalid or a request needs a
    /// capability not declared by the plugin.
    pub fn validate_request(&self, request: &ToolRequest) -> Result<(), PluginAuthorizationError> {
        self.validate()?;
        for capability in &request.required_capabilities {
            if !self
                .capabilities
                .iter()
                .any(|declared| declared == capability)
            {
                return Err(PluginAuthorizationError::CapabilityNotDeclared {
                    capability: capability.clone(),
                });
            }
        }
        Ok(())
    }
}

fn is_host_path(path: &str) -> bool {
    let path = Path::new(path);
    path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}
