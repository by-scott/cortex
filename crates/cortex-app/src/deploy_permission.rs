use std::fs;
use std::path::Path;

use cortex_types::RiskLevel;

/// Parse the permission level requested for a first install.
///
/// # Errors
/// Returns an error when `--permission-level` is missing its value, the value is
/// unknown, or `CORTEX_PERMISSION_LEVEL` is not valid UTF-8.
pub fn parse_install_permission_level(args: &[String]) -> Result<Option<RiskLevel>, String> {
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg != "--permission-level" {
            continue;
        }
        let Some(level) = iter.next() else {
            return Err(
                "missing value for --permission-level (use strict|balanced|open)".to_string(),
            );
        };
        return parse_permission_level_value(level).map(Some);
    }

    match std::env::var("CORTEX_PERMISSION_LEVEL") {
        Ok(level) if !level.trim().is_empty() => {
            parse_permission_level_value(level.trim()).map(Some)
        }
        Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => {
            Err("CORTEX_PERMISSION_LEVEL must be valid UTF-8".to_string())
        }
    }
}

#[must_use]
pub const fn default_permission_level() -> RiskLevel {
    RiskLevel::Review
}

fn parse_permission_level_value(value: &str) -> Result<RiskLevel, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "strict" | "allow" => Ok(RiskLevel::Allow),
        "balanced" | "review" => Ok(RiskLevel::Review),
        "open" | "relaxed" | "requireconfirmation" | "require-confirmation" => {
            Ok(RiskLevel::RequireConfirmation)
        }
        other => Err(format!(
            "invalid permission level '{other}' (use strict|balanced|open)"
        )),
    }
}

#[must_use]
pub const fn permission_level_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Allow => "strict",
        RiskLevel::Review => "balanced",
        RiskLevel::RequireConfirmation => "open",
        RiskLevel::Block => "block",
    }
}

/// Write the runtime risk auto-approval level into an instance config file.
///
/// # Errors
/// Returns an error when the config file cannot be read or written.
pub fn update_install_permission_level(config_path: &Path, level: RiskLevel) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|err| format!("cannot read {}: {err}", config_path.display()))?;
    let level_line = format!("auto_approve_up_to = \"{level:?}\"");
    let mut lines = Vec::new();
    let mut in_risk = false;
    let mut replaced = false;
    let mut inserted_inside_risk = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[risk]" {
            in_risk = true;
            lines.push(line.to_string());
            continue;
        }
        if in_risk && trimmed.starts_with('[') {
            if !replaced {
                lines.push(level_line.clone());
                replaced = true;
                inserted_inside_risk = true;
            }
            in_risk = false;
        }
        if in_risk && trimmed.starts_with("auto_approve_up_to") {
            lines.push(level_line.clone());
            replaced = true;
            continue;
        }
        lines.push(line.to_string());
    }

    if in_risk && !replaced {
        lines.push(level_line.clone());
        replaced = true;
        inserted_inside_risk = true;
    }

    if !replaced && !inserted_inside_risk {
        lines.push(String::new());
        lines.push("[risk]".to_string());
        lines.push(level_line);
    }

    cortex_kernel::atomic_write_text(config_path, lines.join("\n"))
        .map_err(|err| format!("cannot write {}: {err}", config_path.display()))
}

fn current_permission_level(instance_home: &Path) -> RiskLevel {
    let config_path = crate::deploy::config_path_for_instance_home(instance_home);
    fs::read_to_string(&config_path)
        .ok()
        .and_then(|content| crate::deploy_status::read_config_risk_level(&content))
        .unwrap_or_else(default_permission_level)
}

/// `cortex permission [strict|balanced|open]`
///
/// # Errors
/// Returns an error string if the instance does not exist, the mode is invalid,
/// or the instance configuration cannot be updated.
pub fn cmd_permission(args: &[String]) -> Result<(), String> {
    crate::deploy::check_linux()?;
    let system = crate::deploy::parse_system_flag(args);
    let paths = crate::deploy::resolve_paths(args, system);
    let instance_home = paths.instance_home();
    crate::deploy::ensure_instance_home_exists(&instance_home, paths.instance_id())?;

    let mut mode = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--id" | "--home" => {
                let _ = iter.next();
            }
            "--system" | "permission" => {}
            other if other.starts_with("--") => {}
            other => {
                mode = Some(parse_permission_level_value(other)?);
                break;
            }
        }
    }

    let config_path = crate::deploy::config_path_for_instance_home(&instance_home);
    if let Some(level) = mode {
        update_install_permission_level(&config_path, level)?;
        crate::deploy::reload_running_daemon_config(args);
        eprintln!(
            "Permission mode set to {} (auto-approve up to {level:?}) for instance '{}'.",
            permission_level_label(level),
            paths.instance_id()
        );
        if system {
            eprintln!("Restart the system daemon to apply the new permission mode.");
        } else {
            eprintln!("If the daemon is running, this applies shortly.");
        }
    } else {
        let level = current_permission_level(&instance_home);
        eprintln!(
            "Permission mode: {} (auto-approve up to {level:?})",
            permission_level_label(level)
        );
    }
    Ok(())
}
