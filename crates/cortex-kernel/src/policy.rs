use std::collections::BTreeSet;

use cortex_types::{
    RiskLevel, ToolEffect,
    config::{CortexConfig, ToolRiskPolicy},
    plugin::{NativePluginIsolation, PluginManifest, PluginTrustTier},
};

const HAZARDOUS_BACKGROUND_TOOLS: &[&str] = &[
    "agent",
    "bash",
    "cron",
    "edit",
    "memory_save",
    "send_media",
    "write",
];

/// Severity for a static policy finding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum PolicySeverity {
    Info,
    Warning,
    Error,
}

impl PolicySeverity {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Warning => "warning",
            Self::Error => "error",
        }
    }
}

/// One policy-as-code lint finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyIssue {
    pub severity: PolicySeverity,
    pub code: String,
    pub message: String,
    pub remediation: String,
}

/// Result of static policy lint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicyLintReport {
    pub profile: String,
    pub issues: Vec<PolicyIssue>,
}

impl PolicyLintReport {
    #[must_use]
    pub fn passed(&self) -> bool {
        self.error_count() == 0
    }

    #[must_use]
    pub fn error_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == PolicySeverity::Error)
            .count()
    }

    #[must_use]
    pub fn warning_count(&self) -> usize {
        self.issues
            .iter()
            .filter(|issue| issue.severity == PolicySeverity::Warning)
            .count()
    }
}

/// Plugin manifest view consumed by policy lint.
#[derive(Debug, Clone)]
pub struct PolicyPluginView {
    pub name: String,
    pub manifest: Option<PluginManifest>,
    pub load_error: Option<String>,
}

impl PolicyPluginView {
    #[must_use]
    pub fn from_manifest(manifest: PluginManifest) -> Self {
        Self {
            name: manifest.name.clone(),
            manifest: Some(manifest),
            load_error: None,
        }
    }

    #[must_use]
    pub fn load_error(name: impl Into<String>, error: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            manifest: None,
            load_error: Some(error.into()),
        }
    }
}

/// Policy simulation request for one tool/effect decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySimulationRequest {
    pub actor: String,
    pub tool: String,
    pub effects: Vec<ToolEffect>,
    pub background: bool,
}

/// Policy simulation result for one tool/effect decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolicySimulationReport {
    pub actor: String,
    pub tool: String,
    pub risk_level: RiskLevel,
    pub allowed: bool,
    pub confirmation_required: bool,
    pub background_allowed: bool,
    pub reasons: Vec<String>,
}

/// Run static policy lint for a loaded instance config and enabled plugin views.
#[must_use]
pub fn lint_policy(config: &CortexConfig, plugins: &[PolicyPluginView]) -> PolicyLintReport {
    let mut issues = Vec::new();
    lint_risk_lists(config, &mut issues);
    lint_background_policies(config, &mut issues);
    lint_web_memory_policy(config, &mut issues);
    lint_plugins(config, plugins, &mut issues);
    if issues.is_empty() {
        issues.push(issue(
            PolicySeverity::Info,
            "POLICY_OK",
            "policy lint found no blocking issue",
            "keep explicit risk profiles for newly enabled plugins and tools",
        ));
    }
    PolicyLintReport {
        profile: policy_profile_name(config).to_string(),
        issues,
    }
}

/// Simulate how policy would treat one tool invocation and declared effect set.
#[must_use]
pub fn simulate_policy(
    config: &CortexConfig,
    plugins: &[PolicyPluginView],
    request: &PolicySimulationRequest,
) -> PolicySimulationReport {
    let mut reasons = Vec::new();
    let mut effects = request.effects.clone();
    effects.extend(plugin_effects_for_tool(plugins, &request.tool));
    let mut risk_level = base_tool_policy_level(&request.tool);

    if let Some(effect_floor) = effects.iter().map(ToolEffect::risk_floor).max() {
        risk_level = risk_level.max(effect_floor);
        reasons.push(format!("effect floor: {effect_floor:?}"));
    } else {
        reasons.push(format!("base tool risk: {risk_level:?}"));
    }

    if let Some(block_reason) = risk_list_block_reason(config, &request.tool) {
        risk_level = RiskLevel::Block;
        reasons.push(block_reason);
    }

    if let Some(policy) = config.risk.tools.get(&request.tool) {
        risk_level = apply_tool_policy(risk_level, policy, &mut reasons);
    }

    let background_allowed = background_allowed(config, &request.tool, request.background);
    if request.background && !background_allowed {
        reasons.push("background execution denied by tool policy".to_string());
    }

    let allowed = background_allowed
        && risk_level != RiskLevel::Block
        && risk_level <= config.risk.auto_approve_up_to;
    let confirmation_required = background_allowed && risk_level != RiskLevel::Block && !allowed;
    if allowed {
        reasons.push(format!(
            "auto-approved because {risk_level:?} <= {:?}",
            config.risk.auto_approve_up_to
        ));
    } else if confirmation_required {
        reasons.push(format!(
            "requires confirmation because {risk_level:?} > {:?}",
            config.risk.auto_approve_up_to
        ));
    } else if risk_level == RiskLevel::Block {
        reasons.push("blocked by policy risk level".to_string());
    }

    PolicySimulationReport {
        actor: request.actor.clone(),
        tool: request.tool.clone(),
        risk_level,
        allowed,
        confirmation_required,
        background_allowed,
        reasons,
    }
}

fn lint_risk_lists(config: &CortexConfig, issues: &mut Vec<PolicyIssue>) {
    let allow = config.risk.allow.iter().collect::<BTreeSet<_>>();
    for denied in &config.risk.deny {
        if allow.contains(denied) {
            issues.push(issue(
                PolicySeverity::Error,
                "RISK_ALLOW_DENY_CONFLICT",
                format!("tool pattern '{denied}' appears in both risk.allow and risk.deny"),
                "remove the pattern from one list; deny always wins",
            ));
        }
    }
    if config.risk.auto_approve_up_to == RiskLevel::Block {
        issues.push(issue(
            PolicySeverity::Error,
            "RISK_AUTO_APPROVES_BLOCK",
            "risk.auto_approve_up_to is Block",
            "use Allow, Review, or RequireConfirmation; Block is a denial result, not an approval mode",
        ));
    }
}

fn lint_background_policies(config: &CortexConfig, issues: &mut Vec<PolicyIssue>) {
    for (tool, policy) in &config.risk.tools {
        if !policy.allow_background || policy.block {
            continue;
        }
        if HAZARDOUS_BACKGROUND_TOOLS
            .iter()
            .any(|candidate| pattern_matches(tool, candidate) || pattern_matches(candidate, tool))
        {
            issues.push(issue(
                PolicySeverity::Error,
                "BACKGROUND_HIGH_RISK_TOOL",
                format!("tool '{tool}' is allowed to run in the background"),
                "remove allow_background or block the tool; high-impact background actions need explicit foreground review",
            ));
        }
    }
}

fn lint_web_memory_policy(config: &CortexConfig, issues: &mut Vec<PolicyIssue>) {
    let web_fetch_enabled = !config.tools.disabled.iter().any(|tool| tool == "web_fetch");
    if web_fetch_enabled && config.memory.auto_extract {
        issues.push(issue(
            PolicySeverity::Warning,
            "NETWORK_EVIDENCE_AUTO_MEMORY",
            "web_fetch is enabled while memory.auto_extract is true",
            "keep taint gates enabled and require confirmation before promoting network-derived claims to durable memory",
        ));
    }
}

fn lint_plugins(
    config: &CortexConfig,
    plugins: &[PolicyPluginView],
    issues: &mut Vec<PolicyIssue>,
) {
    let open_permissions = config.risk.auto_approve_up_to >= RiskLevel::RequireConfirmation;
    for plugin in plugins {
        let Some(manifest) = &plugin.manifest else {
            issues.push(issue(
                PolicySeverity::Error,
                "PLUGIN_MANIFEST_UNAVAILABLE",
                format!(
                    "enabled plugin '{}' cannot be read: {}",
                    plugin.name,
                    plugin
                        .load_error
                        .as_deref()
                        .unwrap_or("missing manifest or parse error")
                ),
                "repair or disable the plugin before starting the daemon",
            ));
            continue;
        };
        lint_plugin_manifest(config, manifest, open_permissions, issues);
    }
}

fn lint_plugin_manifest(
    config: &CortexConfig,
    manifest: &PluginManifest,
    open_permissions: bool,
    issues: &mut Vec<PolicyIssue>,
) {
    if let Err(error) = manifest.validate_governance() {
        issues.push(issue(
            PolicySeverity::Error,
            "PLUGIN_GOVERNANCE_INVALID",
            error,
            "fix manifest trust, sandbox, and capability declarations before enabling the plugin",
        ));
    }
    for warning in manifest.governance_warnings() {
        issues.push(issue(
            PolicySeverity::Warning,
            "PLUGIN_PACKAGE_GOVERNANCE_GAP",
            format!("plugin '{}': {warning}", manifest.name),
            "add publisher identity, signature, SBOM, risk profile, and conformance certificate",
        ));
    }
    if open_permissions && manifest.trust == PluginTrustTier::UnreviewedProcess {
        issues.push(issue(
            PolicySeverity::Error,
            "OPEN_PERMISSION_UNREVIEWED_PLUGIN",
            format!(
                "open permission mode with unreviewed plugin '{}'",
                manifest.name
            ),
            "review the plugin, lower permission mode, or disable the plugin",
        ));
    }
    lint_plugin_risk_profiles(config, manifest, issues);
}

fn lint_plugin_risk_profiles(
    config: &CortexConfig,
    manifest: &PluginManifest,
    issues: &mut Vec<PolicyIssue>,
) {
    let tool_names = manifest_tool_names(manifest);
    let missing = if manifest.package.risk_profile.trim().is_empty() {
        missing_risk_profiles(config, &tool_names)
    } else {
        Vec::new()
    };
    if manifest.native.is_some() && !missing.is_empty() {
        issues.push(issue(
            PolicySeverity::Error,
            "NATIVE_PLUGIN_WITHOUT_RISK_PROFILE",
            format!(
                "plugin '{}' has native/process tool boundary without risk profile for: {}",
                manifest.name,
                missing.join(", ")
            ),
            "add [risk.tools.<name>] entries for every plugin tool before enabling it",
        ));
    }
    if manifest.capabilities.secrets {
        let unsafe_tools = missing_secret_profiles(config, &tool_names);
        if !unsafe_tools.is_empty() {
            issues.push(issue(
                PolicySeverity::Error,
                "PLUGIN_SECRET_ACCESS_UNGATED",
                format!(
                    "plugin '{}' requests secrets without block/confirmation policy for: {}",
                    manifest.name,
                    unsafe_tools.join(", ")
                ),
                "block the tool or set require_confirmation = true for each secret-capable tool",
            ));
        }
    }
    if manifest.capabilities.background && !missing.is_empty() {
        issues.push(issue(
            PolicySeverity::Error,
            "PLUGIN_BACKGROUND_WITHOUT_RISK_PROFILE",
            format!(
                "plugin '{}' requests background execution without risk profile for: {}",
                manifest.name,
                missing.join(", ")
            ),
            "add explicit risk profiles and avoid allow_background for mutating plugin tools",
        ));
    }
}

fn manifest_tool_names(manifest: &PluginManifest) -> Vec<String> {
    if let Some(native) = &manifest.native
        && native.isolation == NativePluginIsolation::Process
        && !native.tools.is_empty()
    {
        return native.tools.iter().map(|tool| tool.name.clone()).collect();
    }
    vec![manifest.name.clone()]
}

fn missing_risk_profiles(config: &CortexConfig, tool_names: &[String]) -> Vec<String> {
    tool_names
        .iter()
        .filter(|name| !config.risk.tools.contains_key(name.as_str()))
        .cloned()
        .collect()
}

fn missing_secret_profiles(config: &CortexConfig, tool_names: &[String]) -> Vec<String> {
    tool_names
        .iter()
        .filter(|name| {
            !config
                .risk
                .tools
                .get(name.as_str())
                .is_some_and(|policy| policy.block || policy.require_confirmation)
        })
        .cloned()
        .collect()
}

fn plugin_effects_for_tool(plugins: &[PolicyPluginView], tool_name: &str) -> Vec<ToolEffect> {
    let mut effects = Vec::new();
    for manifest in plugins
        .iter()
        .filter_map(|plugin| plugin.manifest.as_ref())
        .filter(|manifest| {
            manifest_tool_names(manifest)
                .iter()
                .any(|name| name == tool_name)
        })
    {
        effects.extend(manifest.capabilities.declared_effects());
        if let Some(native) = &manifest.native {
            effects.extend(
                native
                    .tools
                    .iter()
                    .filter(|tool| tool.name == tool_name)
                    .flat_map(|tool| tool.effects.clone()),
            );
        }
    }
    effects
}

fn base_tool_policy_level(tool: &str) -> RiskLevel {
    match tool {
        "read" | "audit" | "memory_graph" | "prompt_inspect" => RiskLevel::Allow,
        "memory_search" | "web_fetch" | "web_search" => RiskLevel::Review,
        "agent" | "bash" | "cron" | "edit" | "send_media" | "write" => {
            RiskLevel::RequireConfirmation
        }
        _ => RiskLevel::RequireConfirmation,
    }
}

fn risk_list_block_reason(config: &CortexConfig, tool: &str) -> Option<String> {
    if config
        .risk
        .deny
        .iter()
        .any(|pattern| pattern_matches(pattern, tool))
    {
        return Some(format!("blocked by risk.deny for tool '{tool}'"));
    }
    if !config.risk.allow.is_empty()
        && !config
            .risk
            .allow
            .iter()
            .any(|pattern| pattern_matches(pattern, tool))
    {
        return Some(format!(
            "blocked because tool '{tool}' is outside risk.allow"
        ));
    }
    None
}

fn apply_tool_policy(
    current: RiskLevel,
    policy: &ToolRiskPolicy,
    reasons: &mut Vec<String>,
) -> RiskLevel {
    if policy.block {
        reasons.push("tool-specific policy sets block = true".to_string());
        return RiskLevel::Block;
    }
    if policy.require_confirmation && current < RiskLevel::RequireConfirmation {
        reasons.push("tool-specific policy requires confirmation".to_string());
        return RiskLevel::RequireConfirmation;
    }
    current
}

fn background_allowed(config: &CortexConfig, tool: &str, requested: bool) -> bool {
    !requested
        || config
            .risk
            .tools
            .get(tool)
            .is_some_and(|policy| policy.allow_background && !policy.block)
}

fn policy_profile_name(config: &CortexConfig) -> &'static str {
    if config.risk.auto_approve_up_to == RiskLevel::Allow {
        "high-security"
    } else if config.auth.enabled && !config.plugins.enabled.is_empty() {
        "team-shared"
    } else if config.tools.disabled.iter().any(|tool| tool == "web_fetch") {
        "research-offline"
    } else if config.risk.auto_approve_up_to >= RiskLevel::RequireConfirmation {
        "personal-open"
    } else {
        "personal-local"
    }
}

fn issue(
    severity: PolicySeverity,
    code: impl Into<String>,
    message: impl Into<String>,
    remediation: impl Into<String>,
) -> PolicyIssue {
    PolicyIssue {
        severity,
        code: code.into(),
        message: message.into(),
        remediation: remediation.into(),
    }
}

fn pattern_matches(pattern: &str, value: &str) -> bool {
    if pattern == "*" {
        return true;
    }
    let Some((prefix, suffix)) = pattern.split_once('*') else {
        return pattern == value;
    };
    value.starts_with(prefix) && value.ends_with(suffix)
}

#[cfg(test)]
mod tests {
    use cortex_types::{
        EffectConfirmation, PluginSandboxLevel, RiskLevel,
        config::{CortexConfig, ToolRiskPolicy},
        plugin::{
            NativeLibConfig, NativePluginIsolation, PluginManifest, PluginSandboxProfile,
            PluginTrustTier, ProcessToolConfig,
        },
    };

    use super::{PolicyPluginView, PolicySimulationRequest, lint_policy, simulate_policy};

    fn process_manifest(name: &str, tool: &str) -> PluginManifest {
        let mut manifest = PluginManifest::new(
            name,
            "1.0.0",
            "test plugin",
            "test",
            "1.5.10",
            cortex_types::PluginType::Tool,
        );
        manifest.trust = PluginTrustTier::UnreviewedProcess;
        manifest.sandbox = PluginSandboxProfile {
            level: PluginSandboxLevel::ChildProcess,
            ..PluginSandboxProfile::default()
        };
        manifest.capabilities.provides = vec!["tools".to_string()];
        manifest.native = Some(NativeLibConfig {
            library: String::new(),
            abi_version: None,
            isolation: NativePluginIsolation::Process,
            tools: vec![ProcessToolConfig {
                name: tool.to_string(),
                description: "runs work".to_string(),
                input_schema: serde_json::json!({"type":"object"}),
                command: "bin/tool".to_string(),
                args: Vec::new(),
                working_dir: None,
                allow_host_paths: false,
                inherit_env: Vec::new(),
                env: std::collections::BTreeMap::new(),
                timeout_secs: Some(5),
                max_output_bytes: Some(4096),
                max_memory_bytes: None,
                max_cpu_secs: None,
                effects: vec![
                    cortex_types::ToolEffect::new(cortex_types::ToolEffectKind::WriteFile)
                        .with_confirmation(EffectConfirmation::Always),
                ],
            }],
        });
        manifest
    }

    #[test]
    fn policy_lint_rejects_open_unreviewed_plugin_without_risk_profile() {
        let mut config = CortexConfig::default();
        config.risk.auto_approve_up_to = RiskLevel::RequireConfirmation;
        config.plugins.enabled = vec!["danger".to_string()];
        let plugin = PolicyPluginView::from_manifest(process_manifest("danger", "danger_write"));

        let report = lint_policy(&config, &[plugin]);

        assert!(!report.passed());
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "OPEN_PERMISSION_UNREVIEWED_PLUGIN")
        );
        assert!(
            report
                .issues
                .iter()
                .any(|issue| issue.code == "NATIVE_PLUGIN_WITHOUT_RISK_PROFILE")
        );
    }

    #[test]
    fn policy_lint_accepts_packaged_plugin_risk_profile() {
        let mut config = CortexConfig::default();
        config.plugins.enabled = vec!["packaged".to_string()];
        let mut manifest = process_manifest("packaged", "packaged_write");
        manifest.package.risk_profile = "risk.toml".to_string();
        let plugin = PolicyPluginView::from_manifest(manifest);

        let report = lint_policy(&config, &[plugin]);

        assert!(
            !report
                .issues
                .iter()
                .any(|issue| issue.code == "NATIVE_PLUGIN_WITHOUT_RISK_PROFILE"),
            "{:?}",
            report.issues
        );
    }

    #[test]
    fn policy_simulation_explains_confirmation_and_background_denial() {
        let mut config = CortexConfig::default();
        config.risk.auto_approve_up_to = RiskLevel::Review;
        config.risk.tools.insert(
            "deploy".to_string(),
            ToolRiskPolicy {
                require_confirmation: true,
                ..ToolRiskPolicy::default()
            },
        );
        let request = PolicySimulationRequest {
            actor: "user:alice".to_string(),
            tool: "deploy".to_string(),
            effects: vec![cortex_types::ToolEffect::new(
                cortex_types::ToolEffectKind::Deploy,
            )],
            background: true,
        };

        let report = simulate_policy(&config, &[], &request);

        assert!(!report.allowed);
        assert_eq!(report.risk_level, RiskLevel::Block);
        assert!(!report.background_allowed);
        assert!(
            report
                .reasons
                .iter()
                .any(|reason| reason.contains("effect floor"))
        );
    }
}
