use std::path::Path;

/// `cortex config list|get|set`
///
/// # Errors
/// Returns an error string if the instance does not exist, the config cannot be
/// read, or the requested key is not supported for CLI mutation.
pub fn cmd_config(args: &[String]) -> Result<(), String> {
    let system = crate::deploy::parse_system_flag(args);
    let paths = crate::deploy::resolve_paths(args, system);
    let instance_home = paths.instance_home();
    crate::deploy::ensure_instance_home_exists(&instance_home, paths.instance_id())?;

    let invocation = parse_config_invocation(args);
    match invocation.subcommand {
        None | Some("list") => {
            let (providers, resolved) = cortex_kernel::load_providers_for_paths(&paths)
                .map_err(|err| format!("failed to load providers.toml: {err}"))?;
            let config =
                cortex_kernel::load_config_for_paths(&paths, resolved.as_deref(), &providers);
            print!(
                "{}",
                cortex_kernel::format_config_summary(&config, &providers)
            );
            Ok(())
        }
        Some("get") => {
            let Some(section) = positional_config_arg(invocation.remaining, 0) else {
                return Err("usage: cortex config get <section>".to_string());
            };
            let (providers, resolved) = cortex_kernel::load_providers_for_paths(&paths)
                .map_err(|err| format!("failed to load providers.toml: {err}"))?;
            let config =
                cortex_kernel::load_config_for_paths(&paths, resolved.as_deref(), &providers);
            let section_text = cortex_kernel::format_config_section(&config, &providers, section)?;
            print!("{section_text}");
            Ok(())
        }
        Some("set") => {
            let Some(key) = positional_config_arg(invocation.remaining, 0) else {
                return Err("usage: cortex config set <key> <value>".to_string());
            };
            let Some(value) = positional_config_arg(invocation.remaining, 1) else {
                return Err("usage: cortex config set <key> <value>".to_string());
            };
            let message = update_supported_config_key(&paths.config_path(), key, value)?;
            crate::deploy::reload_running_daemon_config(args);
            eprintln!("{message}");
            if system {
                eprintln!("Restart the system daemon to apply the updated config.");
            } else {
                eprintln!("If the daemon is running, this applies shortly.");
            }
            Ok(())
        }
        Some(other) => Err(format!(
            "unknown config subcommand '{other}' (use list|get|set)"
        )),
    }
}

#[derive(Debug, Clone, Copy)]
struct ConfigInvocation<'a> {
    subcommand: Option<&'a str>,
    remaining: &'a [String],
}

fn parse_config_invocation(args: &[String]) -> ConfigInvocation<'_> {
    let root_pos = args.iter().position(|arg| arg == "config");
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
        return ConfigInvocation {
            subcommand: Some(arg),
            remaining: &after_root[index + 1..],
        };
    }

    ConfigInvocation {
        subcommand: None,
        remaining: &[],
    }
}

fn positional_config_arg(args: &[String], target_index: usize) -> Option<&str> {
    let mut position = 0usize;
    let mut index = 0usize;
    while index < args.len() {
        let arg = args[index].as_str();
        if matches!(arg, "--id" | "--home") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        if position == target_index {
            return Some(arg);
        }
        position += 1;
        index += 1;
    }
    None
}

fn update_supported_config_key(
    config_path: &Path,
    key: &str,
    value: &str,
) -> Result<String, String> {
    match key {
        "turn.show_thinking" | "show_thinking" => {
            let show = parse_config_bool(value)?;
            cortex_kernel::update_config_toml_value(
                config_path,
                "turn",
                "strip_think_tags",
                if show { "false" } else { "true" },
            )?;
            Ok(format!(
                "Thinking output {}.",
                if show { "enabled" } else { "hidden" }
            ))
        }
        "turn.strip_think_tags" | "strip_think_tags" => {
            let strip = parse_config_bool(value)?;
            cortex_kernel::update_config_toml_value(
                config_path,
                "turn",
                "strip_think_tags",
                if strip { "true" } else { "false" },
            )?;
            Ok(format!(
                "Thinking output {}.",
                if strip { "hidden" } else { "enabled" }
            ))
        }
        "embedding.api_key" => {
            let literal = serde_json::to_string(value)
                .map_err(|err| format!("failed to encode API key as TOML string: {err}"))?;
            cortex_kernel::update_config_toml_value(config_path, "embedding", "api_key", &literal)?;
            Ok("Embedding API key updated.".to_string())
        }
        _ => Err(format!(
            "unsupported config key '{key}' (supported: turn.show_thinking, turn.strip_think_tags, embedding.api_key)"
        )),
    }
}

fn parse_config_bool(value: &str) -> Result<bool, String> {
    cortex_kernel::parse_bool_like(value)
        .ok_or_else(|| format!("invalid boolean '{value}' (use true/false, on/off, show/hide)"))
}
