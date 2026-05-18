use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::process::Command;

use cortex_types::RiskLevel;
use serde::Serialize;

/// `cortex doctor [--system] [--id ID]`
///
/// # Errors
/// Returns an error string only for unexpected command failures. Readiness
/// findings are rendered as report items so the command remains useful before
/// install or when the daemon is stopped.
pub fn cmd_doctor(args: &[String]) -> Result<(), String> {
    let output = if parse_json_flag(args) {
        DoctorOutput::Json
    } else {
        DoctorOutput::Text
    };
    let report = build_doctor_report(args, output);
    report.finish()
}

fn parse_json_flag(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--json")
}

fn build_doctor_report(args: &[String], output: DoctorOutput) -> DoctorReport {
    let system = crate::deploy::parse_system_flag(args);
    let instance_id = crate::deploy::parse_instance_id(args);
    let paths = crate::deploy::resolve_paths(args, system);
    let svc = crate::deploy::service_name(paths.base_dir(), instance_id.as_deref(), system);
    let instance_home = paths.instance_home();
    let socket_path = paths.socket_path();
    let config_path = paths.config_path();
    let unit_path = if system {
        crate::deploy::system_unit_path_for(&svc)
    } else {
        crate::deploy::user_unit_path_for(&svc)
    };
    let mut report = DoctorReport::new(
        output,
        paths.instance_id(),
        &instance_home,
        if system { "system" } else { "user" },
    );

    if output.is_text() {
        eprintln!("Cortex doctor");
        eprintln!("  Instance: {}", paths.instance_id());
        eprintln!("  Home:     {}", instance_home.display());
        eprintln!("  Mode:     {}", if system { "system" } else { "user" });
        eprintln!();
    }

    report.item(
        if cfg!(target_os = "linux") {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Fail
        },
        "OS",
        if cfg!(target_os = "linux") {
            "Linux detected"
        } else {
            "service-managed Cortex currently expects Linux"
        },
    );
    report.item(
        if command_available("systemctl") {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Fail
        },
        "systemd",
        "systemctl command availability",
    );
    report.item(
        if instance_home.exists() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        },
        "instance",
        if instance_home.exists() {
            "instance directory exists"
        } else {
            "instance directory is missing; run cortex install or create the instance"
        },
    );
    report.item(
        if unit_path.exists() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        },
        "service",
        format!(
            "{} ({svc}; {})",
            unit_state_label(&unit_path, system),
            unit_path.display()
        ),
    );
    report.item(
        socket_level(&socket_path),
        "socket",
        socket_detail(&socket_path, &instance_home),
    );

    let Some((config, config_content)) = doctor_read_config(&config_path, &mut report) else {
        return report;
    };

    report_api(&config, &paths, &mut report);
    report_permission(config.risk.auto_approve_up_to, &mut report);
    report_plugins(&config, &paths, &mut report);
    report_channels(&paths, &mut report);
    report_policy(&config, &paths, &mut report);
    report_protected_roots(&paths, &mut report);

    if crate::deploy::read_config_risk_level(&config_content).is_none() {
        report.item(
            DoctorLevel::Warn,
            "permission config",
            "risk.auto_approve_up_to not found in config; runtime default will apply",
        );
    }

    report
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorLevel {
    Ok,
    Warn,
    Fail,
}

impl DoctorLevel {
    const fn label(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Warn => "warn",
            Self::Fail => "fail",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DoctorOutput {
    Text,
    Json,
}

impl DoctorOutput {
    const fn is_text(self) -> bool {
        matches!(self, Self::Text)
    }
}

#[derive(Default, Serialize)]
struct DoctorSummary {
    ok: usize,
    warn: usize,
    fail: usize,
}

#[derive(Serialize)]
struct DoctorFinding {
    level: &'static str,
    label: String,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    remediation: Option<String>,
}

#[derive(Serialize)]
struct DoctorReport {
    instance: String,
    home: String,
    mode: &'static str,
    summary: DoctorSummary,
    findings: Vec<DoctorFinding>,
    #[serde(skip)]
    output: DoctorOutput,
}

impl DoctorReport {
    fn new(output: DoctorOutput, instance: &str, home: &Path, mode: &'static str) -> Self {
        Self {
            instance: instance.to_string(),
            home: home.display().to_string(),
            mode,
            summary: DoctorSummary::default(),
            findings: Vec::new(),
            output,
        }
    }

    fn item(&mut self, level: DoctorLevel, label: &str, detail: impl AsRef<str>) {
        match level {
            DoctorLevel::Ok => self.summary.ok += 1,
            DoctorLevel::Warn => self.summary.warn += 1,
            DoctorLevel::Fail => self.summary.fail += 1,
        }
        let detail = detail.as_ref().to_string();
        let remediation = doctor_remediation(level, label, &detail);
        if self.output.is_text() {
            eprintln!("[{}] {label}: {detail}", level.label());
            if let Some(fix) = remediation.as_deref() {
                eprintln!("    fix: {fix}");
            }
        }
        self.findings.push(DoctorFinding {
            level: level.label(),
            label: label.to_string(),
            detail,
            remediation,
        });
    }

    fn finish(&self) -> Result<(), String> {
        if self.output.is_text() {
            eprintln!();
            eprintln!(
                "Summary: {} ok, {} warning(s), {} failure(s)",
                self.summary.ok, self.summary.warn, self.summary.fail
            );
            return Ok(());
        }

        println!("{}", self.to_json_string()?);
        Ok(())
    }

    fn to_json_string(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self)
            .map_err(|err| format!("cannot encode doctor JSON report: {err}"))
    }
}

fn doctor_remediation(level: DoctorLevel, label: &str, detail: &str) -> Option<String> {
    if level == DoctorLevel::Ok {
        return None;
    }

    let fix = match label {
        "OS" => {
            "Run service-managed Cortex on Linux, or use a non-service workflow where supported."
        }
        "systemd" => {
            "Install systemd/systemctl or run on a Linux environment with systemd user services."
        }
        "instance" => {
            "Run `cortex install`, or create a bounded fixture with `cortex demo --id demo`."
        }
        "service" => {
            "Run `cortex install`, then `cortex start`; use `cortex status` for service health."
        }
        "socket" => {
            "Run `cortex start`; if the socket remains stale, stop the daemon and remove the stale socket after review."
        }
        "config" if detail.starts_with("cannot parse") => {
            "Fix the TOML syntax in the reported config file, then rerun `cortex doctor`."
        }
        "config" => {
            "Run `cortex install` or `cortex demo` to create a config, then rerun `cortex doctor`."
        }
        "provider key" => {
            "Configure the provider key, or use a local provider such as Ollama/vLLM that does not require a remote API key."
        }
        "permission mode" => {
            "Use `cortex permission balanced` or `cortex permission strict` for normal work."
        }
        "plugins" => {
            "Disable missing plugins in config or install/review the referenced plugin packages."
        }
        "native plugins" => {
            "Treat trusted native ABI plugins as daemon-local code; remove or disable any native plugin you do not trust."
        }
        "policy lint" => {
            "Run `cortex policy lint` for detailed findings, then fix policy or plugin trust posture."
        }
        "protected runtime root" => {
            "Create the instance with `cortex install` or keep tool workspaces outside the runtime root."
        }
        "permission config" => {
            "Set `risk.auto_approve_up_to` explicitly with `cortex permission balanced` or `cortex permission strict`."
        }
        _ => "Review this finding before enabling broader permissions or external side effects.",
    };

    Some(fix.to_string())
}

fn command_available(name: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).is_file()))
}

fn unit_state_label(unit_path: &Path, system: bool) -> &'static str {
    if !unit_path.exists() {
        return "not installed";
    }
    let mut command = Command::new("systemctl");
    if !system {
        command.arg("--user");
    }
    let active = command
        .args(["is-active", "--quiet"])
        .arg(
            unit_path
                .file_stem()
                .and_then(|name| name.to_str())
                .unwrap_or(""),
        )
        .status()
        .is_ok_and(|status| status.success());
    if active { "active" } else { "installed" }
}

fn socket_level(socket_path: &Path) -> DoctorLevel {
    if !socket_path.exists() {
        return DoctorLevel::Warn;
    }
    if fs::metadata(socket_path).is_ok_and(|metadata| metadata.file_type().is_socket()) {
        DoctorLevel::Ok
    } else {
        DoctorLevel::Fail
    }
}

fn socket_detail(socket_path: &Path, instance_home: &Path) -> String {
    if !socket_path.exists() {
        return format!("not present ({})", socket_path.display());
    }
    if cortex_runtime::DaemonClient::is_daemon_running(instance_home) {
        format!("daemon reachable ({})", socket_path.display())
    } else {
        format!(
            "socket exists but daemon did not answer ({})",
            socket_path.display()
        )
    }
}

fn doctor_read_config(
    config_path: &Path,
    report: &mut DoctorReport,
) -> Option<(cortex_types::config::CortexConfig, String)> {
    let Ok(content) = fs::read_to_string(config_path) else {
        report.item(
            DoctorLevel::Fail,
            "config",
            format!("missing or unreadable {}", config_path.display()),
        );
        return None;
    };
    match toml::from_str::<cortex_types::config::CortexConfig>(&content) {
        Ok(config) => {
            report.item(DoctorLevel::Ok, "config", config_path.display().to_string());
            Some((config, content))
        }
        Err(err) => {
            report.item(
                DoctorLevel::Fail,
                "config",
                format!("cannot parse {}: {err}", config_path.display()),
            );
            None
        }
    }
}

fn report_api(
    config: &cortex_types::config::CortexConfig,
    paths: &cortex_kernel::CortexPaths,
    report: &mut DoctorReport,
) {
    let provider = config.api.provider.as_str();
    let model = if config.api.model.is_empty() {
        "(provider default)"
    } else {
        config.api.model.as_str()
    };
    report.item(
        DoctorLevel::Ok,
        "model",
        format!(
            "provider={provider}; model={model}; preset={:?}",
            config.api.preset
        ),
    );
    report.item(
        if config.api.api_key.is_empty() && provider != "ollama" {
            DoctorLevel::Warn
        } else {
            DoctorLevel::Ok
        },
        "provider key",
        if config.api.api_key.is_empty() {
            "not configured in config"
        } else {
            "configured (redacted)"
        },
    );
    let base_url = provider_base_url(paths, provider);
    let local = base_url
        .as_deref()
        .is_some_and(|url| is_local_model_endpoint(provider, url));
    report.item(
        DoctorLevel::Ok,
        "local model/vLLM",
        match (local, base_url) {
            (true, Some(url)) => format!("local-compatible endpoint detected: {url}"),
            (false, Some(url)) => format!("not local; provider endpoint: {url}"),
            (_, None) => {
                "provider endpoint unknown; providers.toml not found or missing entry".to_string()
            }
        },
    );
}

fn provider_base_url(paths: &cortex_kernel::CortexPaths, provider: &str) -> Option<String> {
    let content = fs::read_to_string(paths.providers_path()).ok()?;
    let value = toml::from_str::<toml::Value>(&content).ok()?;
    value
        .get(provider)?
        .get("base_url")?
        .as_str()
        .map(str::to_string)
}

fn is_local_model_endpoint(provider: &str, base_url: &str) -> bool {
    let haystack = format!(
        "{} {}",
        provider.to_ascii_lowercase(),
        base_url.to_ascii_lowercase()
    );
    ["localhost", "127.0.0.1", "0.0.0.0", "ollama", "vllm"]
        .iter()
        .any(|needle| haystack.contains(needle))
}

fn report_permission(level: RiskLevel, report: &mut DoctorReport) {
    report.item(
        if level == RiskLevel::RequireConfirmation {
            DoctorLevel::Warn
        } else {
            DoctorLevel::Ok
        },
        "permission mode",
        format!(
            "{} (auto-approve up to {level:?})",
            crate::deploy_permission::permission_level_label(level)
        ),
    );
}

fn report_plugins(
    config: &cortex_types::config::CortexConfig,
    paths: &cortex_kernel::CortexPaths,
    report: &mut DoctorReport,
) {
    let installed = crate::plugin_manager::list(paths.base_dir());
    let enabled = &config.plugins.enabled;
    let missing_enabled: Vec<&String> = enabled
        .iter()
        .filter(|name| !installed.iter().any(|plugin| &plugin.name == *name))
        .collect();
    let native_count = installed.iter().filter(|plugin| plugin.has_native).count();
    report.item(
        if missing_enabled.is_empty() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        },
        "plugins",
        format!(
            "{} installed, {} enabled, {} missing enabled",
            installed.len(),
            enabled.len(),
            missing_enabled.len()
        ),
    );
    if native_count > 0 {
        report.item(
            DoctorLevel::Warn,
            "native plugins",
            format!("{native_count} trusted native plugin(s) present; native ABI is not sandboxed"),
        );
    }
}

fn report_channels(paths: &cortex_kernel::CortexPaths, report: &mut DoctorReport) {
    let configured: Vec<&str> = ["telegram", "whatsapp", "qq"]
        .into_iter()
        .filter(|platform| paths.channel_auth_path(platform).exists())
        .collect();
    report.item(
        DoctorLevel::Ok,
        "channels",
        if configured.is_empty() {
            "none configured".to_string()
        } else {
            format!("configured: {}", configured.join(", "))
        },
    );
}

fn report_policy(
    config: &cortex_types::config::CortexConfig,
    paths: &cortex_kernel::CortexPaths,
    report: &mut DoctorReport,
) {
    let plugins = crate::deploy_policy::read_policy_plugins(paths, config);
    let policy = cortex_kernel::lint_policy(config, &plugins);
    let level = if policy.error_count() > 0 {
        DoctorLevel::Fail
    } else if policy.warning_count() > 0 {
        DoctorLevel::Warn
    } else {
        DoctorLevel::Ok
    };
    report.item(
        level,
        "policy lint",
        format!(
            "{} error(s), {} warning(s)",
            policy.error_count(),
            policy.warning_count()
        ),
    );
}

fn report_protected_roots(paths: &cortex_kernel::CortexPaths, report: &mut DoctorReport) {
    report.item(
        if paths.instance_home().exists() {
            DoctorLevel::Ok
        } else {
            DoctorLevel::Warn
        },
        "protected runtime root",
        format!(
            "instance root identified for runtime policy checks: {}",
            paths.instance_home().display()
        ),
    );
}
