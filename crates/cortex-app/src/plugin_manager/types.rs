use std::fmt::Write as _;

use cortex_types::plugin::PluginConformanceCheck;

/// Metadata about an installed plugin, parsed from its manifest.
pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub capabilities: Vec<String>,
    pub trust: String,
    pub signature_state: String,
    pub conformance_state: String,
    pub has_native: bool,
}

/// Install/conformance review for one plugin package.
#[derive(Debug, Clone)]
pub struct PluginReview {
    pub name: String,
    pub version: String,
    pub trust: String,
    pub requested_capabilities: Vec<String>,
    pub signature_state: String,
    pub conformance_state: String,
    pub recommended_risk_profile: Vec<String>,
    pub warnings: Vec<String>,
    pub checks: Vec<PluginConformanceCheck>,
}

impl PluginReview {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.checks.iter().all(|check| check.passed)
    }

    #[must_use]
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(out, "Plugin review: {} v{}", self.name, self.version);
        let _ = writeln!(out, "  Trust: {}", self.trust);
        let _ = writeln!(out, "  Signature: {}", self.signature_state);
        let _ = writeln!(out, "  Conformance: {}", self.conformance_state);
        out.push_str("  Requested capabilities:\n");
        if self.requested_capabilities.is_empty() {
            out.push_str("    - no host capabilities declared\n");
        } else {
            for line in &self.requested_capabilities {
                let _ = writeln!(out, "    - {line}");
            }
        }
        if !self.recommended_risk_profile.is_empty() {
            out.push_str("  Recommended risk profile:\n");
            for line in &self.recommended_risk_profile {
                let _ = writeln!(out, "    {line}");
            }
        }
        if !self.warnings.is_empty() {
            out.push_str("  Warnings:\n");
            for warning in &self.warnings {
                let _ = writeln!(out, "    - {warning}");
            }
        }
        if !self.checks.is_empty() {
            out.push_str("  Checks:\n");
            for check in &self.checks {
                let status = if check.passed { "ok" } else { "fail" };
                if check.message.is_empty() {
                    let _ = writeln!(out, "    - {status}: {}", check.name);
                } else {
                    let _ = writeln!(out, "    - {status}: {} ({})", check.name, check.message);
                }
            }
        }
        out
    }
}
