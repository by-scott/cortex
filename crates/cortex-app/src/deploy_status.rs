use std::fs;
use std::path::Path;

use cortex_types::RiskLevel;

/// `cortex status [--system] [--id ID]`
///
/// # Errors
/// Returns an error string if the status cannot be queried.
pub fn cmd_status(args: &[String]) -> Result<(), String> {
    crate::deploy::check_linux()?;
    let system = crate::deploy::parse_system_flag(args);
    let instance_id = crate::deploy::parse_instance_id(args);
    let paths = crate::deploy::resolve_paths(args, system);
    let svc = crate::deploy::service_name(paths.base_dir(), instance_id.as_deref(), system);

    if !(if system {
        crate::deploy::system_unit_path_for(&svc).exists()
    } else {
        crate::deploy::user_unit_path_for(&svc).exists()
    }) {
        let flag = if system { " --system" } else { "" };
        eprintln!("Service not installed, run `cortex install{flag}` first.");
        return Ok(());
    }

    let out = crate::deploy::systemctl(&["status", &svc], system)?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    let active_line = stdout
        .lines()
        .find(|l| l.contains("Active:"))
        .unwrap_or("Active: unknown");
    let pid_line = stdout.lines().find(|l| l.contains("Main PID:"));

    let mode = if system { "system" } else { "user" };
    let instance_path = paths.instance_home();
    let socket_path = paths.socket_path();
    let instance_home = instance_path.to_string_lossy().to_string();

    eprintln!("Cortex {mode} service status ({svc}):");
    eprintln!("  {}", active_line.trim());
    if let Some(pid) = pid_line {
        eprintln!("  {}", pid.trim());
    }

    eprintln!("  Data:   {instance_home}");
    eprintln!("  Socket: {}", socket_path.display());

    let config_path = crate::deploy::config_path_for_instance_home(&instance_path);
    if let Ok(content) = fs::read_to_string(&config_path) {
        let config_summary = read_status_config(&content);
        let live_status = read_live_status(&socket_path);
        print_status_details(&config_summary, &live_status, &content);
    }
    Ok(())
}

fn print_status_details(
    config_summary: &StatusConfigSummary,
    live_status: &LiveStatusSummary,
    config_content: &str,
) {
    if !config_summary.addr.is_empty() && !config_summary.addr.ends_with(":0") {
        eprintln!(
            "  HTTP:   {}  (REST / RPC / SSE / Web UI)",
            config_summary.addr
        );
    }
    if !config_summary.provider.is_empty() {
        let model_info = if config_summary.model.is_empty() {
            String::new()
        } else {
            format!(" / {}", config_summary.model)
        };
        let preset_info = if config_summary.preset.is_empty() {
            String::new()
        } else {
            format!(" ({})", config_summary.preset)
        };
        eprintln!(
            "  LLM:    {}{model_info}{preset_info}",
            config_summary.provider
        );
    }
    if let Some(level) = live_status
        .permission_level
        .as_deref()
        .map(str::to_owned)
        .or_else(|| read_config_risk_level(config_content).map(|level| format!("{level:?}")))
    {
        eprintln!(
            "  \u{1f6e1}\u{fe0f} Permission: {}",
            permission_level_label_from_risk(&level)
        );
    }
    print_live_token_status(live_status);
}

fn print_live_token_status(live_status: &LiveStatusSummary) {
    if let (Some(input), Some(output)) = (
        live_status.last_call_input_tokens,
        live_status.last_call_output_tokens,
    ) {
        eprintln!(
            "  \u{1fa9f} Context: call {} in / {} out",
            format_token_count(input),
            format_token_count(output)
        );
    }
    if let Some(total) = live_status.total_tokens {
        let session_tokens = live_status
            .session_tokens
            .map_or_else(|| "n/a".to_string(), format_token_count);
        eprintln!(
            "  \u{1f9ee} Tokens: total {} / session {session_tokens}",
            format_token_count(total),
        );
    }
}

#[derive(Default)]
struct StatusConfigSummary {
    addr: String,
    provider: String,
    model: String,
    preset: String,
}

#[derive(Default)]
struct LiveStatusSummary {
    total_tokens: Option<u64>,
    session_tokens: Option<u64>,
    last_call_input_tokens: Option<u64>,
    last_call_output_tokens: Option<u64>,
    permission_level: Option<String>,
}

fn extract_toml_value(line: &str) -> String {
    line.split('=')
        .nth(1)
        .map(|value| {
            let value = value.trim();
            if value.starts_with('"') {
                value
                    .get(1..)
                    .and_then(|trimmed| trimmed.find('"').map(|idx| &trimmed[..idx]))
                    .unwrap_or_else(|| value.trim_matches('"'))
                    .to_string()
            } else {
                value.split('#').next().unwrap_or(value).trim().to_string()
            }
        })
        .unwrap_or_default()
}

fn read_status_config(content: &str) -> StatusConfigSummary {
    let mut summary = StatusConfigSummary::default();
    let mut in_daemon = false;
    let mut in_api = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("[daemon]") {
            in_daemon = true;
            in_api = false;
            continue;
        }
        if trimmed.starts_with("[api]") {
            in_api = true;
            in_daemon = false;
            continue;
        }
        if trimmed.starts_with('[') {
            in_daemon = false;
            in_api = false;
            continue;
        }

        if in_daemon && trimmed.starts_with("addr") {
            summary.addr = extract_toml_value(trimmed);
        }
        if in_api && trimmed.starts_with("provider") && !trimmed.starts_with("provider_") {
            summary.provider = extract_toml_value(trimmed);
        }
        if in_api && trimmed.starts_with("model") {
            summary.model = extract_toml_value(trimmed);
        }
        if in_api && trimmed.starts_with("preset") {
            summary.preset = extract_toml_value(trimmed);
        }
    }

    summary
}

fn read_live_status(socket_path: &Path) -> LiveStatusSummary {
    let Ok(client) = cortex_runtime::DaemonClient::connect_socket(socket_path) else {
        return LiveStatusSummary::default();
    };
    let Ok(status) = client.status() else {
        return LiveStatusSummary::default();
    };

    LiveStatusSummary {
        total_tokens: status
            .get("metrics")
            .and_then(|metrics| metrics.get("total_tokens"))
            .and_then(serde_json::Value::as_u64),
        session_tokens: status
            .get("metrics")
            .and_then(|metrics| metrics.get("session_tokens"))
            .and_then(serde_json::Value::as_u64),
        last_call_input_tokens: status
            .get("metrics")
            .and_then(|metrics| metrics.get("last_call_input_tokens"))
            .and_then(serde_json::Value::as_u64),
        last_call_output_tokens: status
            .get("metrics")
            .and_then(|metrics| metrics.get("last_call_output_tokens"))
            .and_then(serde_json::Value::as_u64),
        permission_level: status
            .get("risk")
            .and_then(|risk| risk.get("auto_approve_up_to"))
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
    }
}

pub fn read_config_risk_level(content: &str) -> Option<RiskLevel> {
    let mut in_risk = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[risk]" {
            in_risk = true;
            continue;
        }
        if in_risk && trimmed.starts_with('[') {
            break;
        }
        if in_risk && trimmed.starts_with("auto_approve_up_to") {
            let value = trimmed.split('=').nth(1)?.trim().trim_matches('"');
            return match value {
                "Allow" => Some(RiskLevel::Allow),
                "Review" => Some(RiskLevel::Review),
                "RequireConfirmation" => Some(RiskLevel::RequireConfirmation),
                "Block" => Some(RiskLevel::Block),
                _ => None,
            };
        }
    }
    None
}

fn permission_level_label_from_risk(level: &str) -> &'static str {
    match level {
        "Allow" => "strict",
        "Review" => "balanced",
        "RequireConfirmation" => "open",
        "Block" => "block",
        _ => "custom",
    }
}

fn format_token_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{}.{}M", value / 1_000_000, (value % 1_000_000) / 100_000)
    } else if value >= 1_000 {
        format!("{}.{}K", value / 1_000, (value % 1_000) / 100)
    } else {
        value.to_string()
    }
}
