use std::fs;
use std::path::Path;

use cortex_types::{ToolEffect, ToolEffectKind};

#[derive(Debug, Clone, PartialEq, Eq)]
struct PolicyInvocation<'a> {
    subcommand: Option<&'a str>,
    remaining: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommandSpec<T> {
    subcommand: T,
    names: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicySubcommand {
    Lint,
    Simulate,
}

const POLICY_SUBCOMMAND_SPECS: &[CommandSpec<PolicySubcommand>] = &[
    CommandSpec {
        subcommand: PolicySubcommand::Lint,
        names: &["lint"],
    },
    CommandSpec {
        subcommand: PolicySubcommand::Simulate,
        names: &["simulate", "explain"],
    },
];

/// `cortex policy lint|simulate`
///
/// # Errors
/// Returns an error string if the instance is missing, config cannot be read,
/// plugin manifests cannot be represented, or the requested simulation is
/// invalid.
pub fn cmd_policy(args: &[String]) -> Result<(), String> {
    let system = crate::deploy::parse_system_flag(args);
    let paths = crate::deploy::resolve_paths(args, system);
    crate::deploy::ensure_instance_home_exists(&paths.instance_home(), paths.instance_id())?;
    let config = read_policy_config(&paths)?;
    let plugins = read_policy_plugins(&paths, &config);
    let invocation = parse_policy_invocation(args);

    match parse_policy_subcommand(invocation.subcommand)? {
        PolicySubcommand::Lint => cmd_policy_lint(&config, &plugins),
        PolicySubcommand::Simulate => cmd_policy_simulate(&config, &plugins, invocation.remaining),
    }
}

pub fn read_policy_plugins(
    paths: &cortex_kernel::CortexPaths,
    config: &cortex_types::config::CortexConfig,
) -> Vec<cortex_kernel::PolicyPluginView> {
    let base =
        cortex_runtime::plugin_loader::plugin_base_dir(&paths.instance_home(), &config.plugins);
    config
        .plugins
        .enabled
        .iter()
        .map(|name| read_policy_plugin_manifest(&base, name))
        .collect()
}

fn cmd_policy_lint(
    config: &cortex_types::config::CortexConfig,
    plugins: &[cortex_kernel::PolicyPluginView],
) -> Result<(), String> {
    let report = cortex_kernel::lint_policy(config, plugins);
    render_policy_lint(&report);
    if report.passed() {
        Ok(())
    } else {
        Err(format!(
            "policy lint failed: {} error(s), {} warning(s)",
            report.error_count(),
            report.warning_count()
        ))
    }
}

fn cmd_policy_simulate(
    config: &cortex_types::config::CortexConfig,
    plugins: &[cortex_kernel::PolicyPluginView],
    args: &[String],
) -> Result<(), String> {
    let request = parse_policy_simulation(args)?;
    let report = cortex_kernel::simulate_policy(config, plugins, &request);
    render_policy_simulation(&report);
    Ok(())
}

fn read_policy_config(
    paths: &cortex_kernel::CortexPaths,
) -> Result<cortex_types::config::CortexConfig, String> {
    let path = paths.config_path();
    let content = fs::read_to_string(&path)
        .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
    toml::from_str(&content).map_err(|err| format!("cannot parse {}: {err}", path.display()))
}

fn read_policy_plugin_manifest(base: &Path, name: &str) -> cortex_kernel::PolicyPluginView {
    let plugin_dir = base.join(name);
    match cortex_runtime::plugin_loader::read_installed_manifest(&plugin_dir) {
        Ok(manifest) => cortex_kernel::PolicyPluginView::from_manifest(manifest),
        Err(err) => cortex_kernel::PolicyPluginView::load_error(
            name,
            format!(
                "cannot read installed manifest {}: {err}",
                plugin_dir.display()
            ),
        ),
    }
}

fn parse_policy_invocation(args: &[String]) -> PolicyInvocation<'_> {
    let root_pos = args.iter().position(|arg| arg == "policy");
    let after_root = root_pos.map_or(args, |pos| &args[pos + 1..]);

    let mut index = 0usize;
    while index < after_root.len() {
        let arg = after_root[index].as_str();
        if matches!(arg, "--id" | "--home") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return PolicyInvocation {
            subcommand: Some(arg),
            remaining: &after_root[index + 1..],
        };
    }

    PolicyInvocation {
        subcommand: None,
        remaining: &[],
    }
}

fn parse_policy_subcommand(subcommand: Option<&str>) -> Result<PolicySubcommand, String> {
    let Some(subcommand) = subcommand else {
        return Ok(PolicySubcommand::Lint);
    };
    POLICY_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
        .ok_or_else(|| unknown_policy_subcommand_error(subcommand))
}

fn unknown_policy_subcommand_error(subcommand: &str) -> String {
    let choices = POLICY_SUBCOMMAND_SPECS
        .iter()
        .map(|spec| spec.names[0])
        .collect::<Vec<_>>()
        .join(", ");
    format!("unknown policy command: {subcommand}. Use: {choices}")
}

fn parse_policy_simulation(
    args: &[String],
) -> Result<cortex_kernel::PolicySimulationRequest, String> {
    let tool = flag_value(args, "--tool")
        .cloned()
        .or_else(|| first_positional(args, &["--tool", "--actor", "--effect"]).cloned())
        .ok_or("usage: cortex policy simulate <tool> [--effect <kind[:target]>]")?;
    let actor = flag_value(args, "--actor")
        .cloned()
        .unwrap_or_else(|| "local:operator".to_string());
    let effects = repeated_flag_values(args, "--effect")
        .map(|effect| parse_policy_effect(effect))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(cortex_kernel::PolicySimulationRequest {
        actor,
        tool,
        effects,
        background: args.iter().any(|arg| arg == "--background"),
    })
}

fn flag_value<'a>(args: &'a [String], flag: &str) -> Option<&'a String> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|index| args.get(index + 1))
}

fn repeated_flag_values<'a>(
    args: &'a [String],
    flag: &'a str,
) -> impl Iterator<Item = &'a String> + 'a {
    args.iter()
        .enumerate()
        .filter_map(move |(index, arg)| (arg == flag).then(|| args.get(index + 1)).flatten())
}

fn first_positional<'a>(args: &'a [String], value_flags: &[&str]) -> Option<&'a String> {
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if value_flags.iter().any(|flag| arg == flag) {
            skip_next = true;
            continue;
        }
        if !arg.starts_with('-') {
            return Some(arg);
        }
    }
    None
}

fn parse_policy_effect(raw: &str) -> Result<ToolEffect, String> {
    let (kind, target) = raw
        .split_once(':')
        .map_or((raw, ""), |(kind, target)| (kind, target));
    let kind = parse_policy_effect_kind(kind)?;
    Ok(ToolEffect::new(kind).with_target(target))
}

fn parse_policy_effect_kind(raw: &str) -> Result<ToolEffectKind, String> {
    match raw.trim().replace('-', "_").to_ascii_lowercase().as_str() {
        "read" | "read_file" => Ok(ToolEffectKind::ReadFile),
        "read_secret" | "secret" => Ok(ToolEffectKind::ReadSecret),
        "write" | "write_file" => Ok(ToolEffectKind::WriteFile),
        "delete" | "delete_file" => Ok(ToolEffectKind::DeleteFile),
        "run" | "run_process" | "process" => Ok(ToolEffectKind::RunProcess),
        "network" | "network_request" => Ok(ToolEffectKind::NetworkRequest),
        "send" | "send_message" => Ok(ToolEffectKind::SendMessage),
        "spend" | "spend_money" => Ok(ToolEffectKind::SpendMoney),
        "deploy" => Ok(ToolEffectKind::Deploy),
        "modify_credential" | "credential" => Ok(ToolEffectKind::ModifyCredential),
        "persist_memory" | "memory" => Ok(ToolEffectKind::PersistMemory),
        "publish" | "publish_content" => Ok(ToolEffectKind::PublishContent),
        "schedule" | "schedule_task" => Ok(ToolEffectKind::ScheduleTask),
        "generate_media" | "media" => Ok(ToolEffectKind::GenerateMedia),
        "introspect" | "introspect_runtime" => Ok(ToolEffectKind::IntrospectRuntime),
        "delegate" | "delegate_work" => Ok(ToolEffectKind::DelegateWork),
        other => Err(format!("unknown effect kind '{other}'")),
    }
}

fn render_policy_lint(report: &cortex_kernel::PolicyLintReport) {
    eprintln!("Policy profile: {}", report.profile);
    eprintln!(
        "Policy lint: {} error(s), {} warning(s)",
        report.error_count(),
        report.warning_count()
    );
    for issue in &report.issues {
        render_policy_issue(issue);
    }
}

fn render_policy_issue(issue: &cortex_kernel::PolicyIssue) {
    eprintln!(
        "  [{}] {}: {}",
        issue.severity.as_str(),
        issue.code,
        issue.message
    );
    eprintln!("      fix: {}", issue.remediation);
}

fn render_policy_simulation(report: &cortex_kernel::PolicySimulationReport) {
    eprintln!("Policy simulation:");
    eprintln!("  actor: {}", report.actor);
    eprintln!("  tool: {}", report.tool);
    eprintln!("  risk: {:?}", report.risk_level);
    eprintln!("  allowed: {}", report.allowed);
    eprintln!("  confirmation_required: {}", report.confirmation_required);
    eprintln!("  background_allowed: {}", report.background_allowed);
    for reason in &report.reasons {
        eprintln!("  - {reason}");
    }
}
