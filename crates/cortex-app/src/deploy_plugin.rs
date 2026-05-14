use std::fs;
use std::path::Path;

/// `cortex plugin <sub> [args...]` manages installed plugins.
pub fn cmd_plugin(args: &[String]) -> Result<(), String> {
    use crate::plugin_manager;

    let plugin_args: &[String] = args
        .iter()
        .position(|a| a == "plugin")
        .map_or(args, |pos| &args[pos + 1..]);

    let sub = plugin_args.first().map_or("list", String::as_str);
    let paths = crate::deploy::resolve_paths_from_args(args);
    let cortex_home = paths.base_dir().clone();
    let home = cortex_home.as_path();
    let instance_id = crate::deploy::parse_instance_id(args);
    let instance = instance_id.as_deref().unwrap_or("default");
    let instance_home = paths.instance_home();

    match sub {
        "install" => plugin_install(args, plugin_args, home, &instance_home, instance)?,
        "enable" => plugin_enable(args, plugin_args, home, &instance_home, instance)?,
        "disable" => plugin_disable(args, plugin_args, home, &instance_home, instance)?,
        "uninstall" | "remove" => {
            plugin_uninstall(args, plugin_args, home, &paths, &instance_home, instance)?;
        }
        "list" | "ls" => {
            let plugins = plugin_manager::list(home);
            let enabled = read_enabled_plugins(&instance_home);
            if plugins.is_empty() {
                eprintln!("No plugins installed.");
            } else {
                for plugin in &plugins {
                    let native = if plugin.has_native { " [native]" } else { "" };
                    let status = if enabled.iter().any(|entry| entry == &plugin.name) {
                        " [enabled]"
                    } else {
                        ""
                    };
                    eprintln!(
                        "  {} v{}{}{} -- {} [{}; signature: {}; conformance: {}]",
                        plugin.name,
                        plugin.version,
                        native,
                        status,
                        plugin.description,
                        plugin.trust,
                        plugin.signature_state,
                        plugin.conformance_state
                    );
                }
            }
        }
        "review" => {
            let dir = plugin_args
                .get(1)
                .ok_or("usage: cortex plugin review <dir>")?;
            let review = plugin_manager::review_directory(Path::new(dir.as_str()))?;
            eprint!("{}", review.render());
        }
        "test" => {
            let dir = plugin_args
                .get(1)
                .ok_or("usage: cortex plugin test <dir>")?;
            match plugin_manager::test_directory(Path::new(dir.as_str())) {
                Ok(review) => eprint!("{}", review.render()),
                Err(report) => return Err(report),
            }
        }
        "pack" => {
            plugin_pack(plugin_args)?;
        }
        "keygen" => {
            plugin_keygen(plugin_args)?;
        }
        "sign" => {
            plugin_sign(plugin_args)?;
        }
        _ => {
            return Err(format!(
                "unknown plugin command: {sub}. Use: install, enable, disable, uninstall, list, review, test, keygen, sign, pack"
            ));
        }
    }
    Ok(())
}

fn plugin_pack(plugin_args: &[String]) -> Result<(), String> {
    let positionals = plugin_positionals(plugin_args);
    let dir = positionals
        .first()
        .map(|value| value.as_str())
        .ok_or("usage: cortex plugin pack <dir> [output.cpx]")?;
    let dir_path = Path::new(dir);
    let default_output = crate::plugin_manager::default_cpx_name(dir_path)?;
    let output = positionals
        .get(1)
        .map_or(default_output.as_str(), |value| value.as_str());
    crate::plugin_manager::pack(dir_path, Path::new(output))?;
    eprintln!("Packed plugin: {output}");
    Ok(())
}

fn plugin_keygen(plugin_args: &[String]) -> Result<(), String> {
    let positionals = plugin_positionals(plugin_args);
    let key_path = positionals
        .first()
        .map(|value| value.as_str())
        .ok_or("usage: cortex plugin keygen <private-key-path>")?;
    let report = crate::plugin_manager::generate_signing_key(Path::new(key_path))?;
    eprintln!("{report}");
    Ok(())
}

fn plugin_sign(plugin_args: &[String]) -> Result<(), String> {
    let positionals = plugin_positionals(plugin_args);
    let dir = positionals
        .first()
        .map(|value| value.as_str())
        .ok_or("usage: cortex plugin sign <dir> --key <private-key-path> [--publisher <id>]")?;
    let key = plugin_arg_value(plugin_args, "--key")
        .ok_or("usage: cortex plugin sign <dir> --key <private-key-path> [--publisher <id>]")?;
    let publisher = plugin_arg_value(plugin_args, "--publisher");
    let package = crate::plugin_manager::sign_directory(Path::new(dir), Path::new(key), publisher)?;
    let review = crate::plugin_manager::review_directory(Path::new(dir))?;
    eprint!("{}", review.render());
    eprintln!(
        "Signed plugin package: publisher={} algorithm={}",
        package.publisher_id, package.signature_algorithm
    );
    Ok(())
}

fn ensure_plugin_installed(home: &Path, name: &str) -> Result<(), String> {
    if crate::plugin_manager::list(home)
        .iter()
        .any(|plugin| plugin.name == name)
    {
        Ok(())
    } else {
        Err(format!("plugin '{name}' is not installed"))
    }
}

fn plugin_requires_restart(home: &Path, name: &str) -> bool {
    let manifest_path = home.join("plugins").join(name).join("manifest.toml");
    std::fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|text| toml::from_str::<cortex_types::plugin::PluginManifest>(&text).ok())
        .and_then(|manifest| manifest.native)
        .is_some_and(|native| {
            native.isolation == cortex_types::plugin::NativePluginIsolation::TrustedInProcess
        })
}

fn hint_plugin_apply_if_running(args: &[String], home: &Path, name: &str, enabling: bool) {
    let instance_id = crate::deploy::parse_instance_id(args);
    let system = crate::deploy::parse_system_flag(args);
    let paths = crate::deploy::resolve_paths(args, system);
    let svc = crate::deploy::service_name(paths.base_dir(), instance_id.as_deref(), system);
    let exists = if system {
        crate::deploy::system_unit_path_for(&svc).exists()
    } else {
        crate::deploy::user_unit_path_for(&svc).exists()
    };
    if !exists {
        return;
    }
    let Ok(out) = crate::deploy::systemctl(&["is-active", &svc], system) else {
        return;
    };
    if String::from_utf8_lossy(&out.stdout).trim() != "active" {
        return;
    }

    let requires_restart = plugin_requires_restart(home, name);
    if requires_restart && enabling {
        eprintln!(
            "Trusted in-process native plugins still require `cortex restart` to load new code."
        );
    } else if requires_restart {
        eprintln!(
            "Plugin tool visibility updates apply now; restart only if you need native code fully unloaded."
        );
    } else {
        eprintln!("If the daemon is running, plugin tool changes will hot-reload shortly.");
    }
}

fn plugin_install(
    args: &[String],
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let positionals = plugin_positionals(plugin_args);
    let source = positionals
        .first()
        .ok_or("usage: cortex plugin install <owner/repo|url|path> [--id <instance>] [--yes]")?;
    let source = source.as_str();
    crate::deploy::ensure_instance_home_exists(instance_home, instance)?;
    if Path::new(source).is_dir() {
        let review = crate::plugin_manager::review_directory(Path::new(source))?;
        eprint!("{}", review.render());
    }
    let unknown_publisher = if plugin_flag(plugin_args, "--yes") {
        crate::plugin_manager::UnknownPublisherPolicy::TrustVerified
    } else {
        crate::plugin_manager::UnknownPublisherPolicy::Prompt
    };
    let policy = if Path::new(source).is_dir() {
        crate::plugin_manager::PluginInstallPolicy::developer_default()
    } else {
        crate::plugin_manager::PluginInstallPolicy::release_default(unknown_publisher)
    };
    let name = crate::plugin_manager::install_with_policy(home, source, policy)?;
    enable_plugin_in_config(instance_home, &name)?;
    crate::deploy::reload_running_daemon_config(args);
    eprintln!("Installed plugin: {name} (enabled for instance '{instance}')");
    hint_plugin_apply_if_running(args, home, &name, true);
    Ok(())
}

fn plugin_flag(plugin_args: &[String], flag: &str) -> bool {
    plugin_args.iter().any(|arg| arg == flag)
}

fn plugin_arg_value<'a>(plugin_args: &'a [String], flag: &str) -> Option<&'a str> {
    plugin_args
        .iter()
        .position(|arg| arg == flag)
        .and_then(|index| plugin_args.get(index + 1))
        .map(String::as_str)
        .or_else(|| {
            let prefix = format!("{flag}=");
            plugin_args.iter().find_map(|arg| arg.strip_prefix(&prefix))
        })
}

fn plugin_positionals(plugin_args: &[String]) -> Vec<&String> {
    let mut positionals = Vec::new();
    let mut skip_next = false;
    for arg in plugin_args.iter().skip(1) {
        if skip_next {
            skip_next = false;
            continue;
        }
        match arg.as_str() {
            "--id" | "--home" | "--key" | "--publisher" => {
                skip_next = true;
            }
            "--yes" | "--system" | "--purge" => {}
            value
                if value.starts_with("--id=")
                    || value.starts_with("--home=")
                    || value.starts_with("--key=")
                    || value.starts_with("--publisher=") => {}
            value if value.starts_with('-') => {}
            _ => positionals.push(arg),
        }
    }
    positionals
}

fn plugin_enable(
    args: &[String],
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin enable <name> [--id <instance>]")?;
    crate::deploy::ensure_instance_home_exists(instance_home, instance)?;
    ensure_plugin_installed(home, name)?;
    enable_plugin_in_config(instance_home, name)?;
    crate::deploy::reload_running_daemon_config(args);
    eprintln!("Enabled plugin: {name} (for instance '{instance}')");
    hint_plugin_apply_if_running(args, home, name, true);
    Ok(())
}

fn plugin_disable(
    args: &[String],
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin disable <name> [--id <instance>]")?;
    crate::deploy::ensure_instance_home_exists(instance_home, instance)?;
    ensure_plugin_installed(home, name)?;
    disable_plugin_in_config(instance_home, name)?;
    crate::deploy::reload_running_daemon_config(args);
    eprintln!("Disabled plugin: {name} (for instance '{instance}')");
    hint_plugin_apply_if_running(args, home, name, false);
    Ok(())
}

fn plugin_uninstall(
    args: &[String],
    plugin_args: &[String],
    home: &Path,
    paths: &cortex_kernel::CortexPaths,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin uninstall <name> [--id <instance>] [--purge]")?;
    crate::deploy::ensure_instance_home_exists(instance_home, instance)?;
    let global_exists = paths.plugins_dir().join(name.as_str()).exists();
    let enabled = read_enabled_plugins(instance_home);
    let in_config = enabled.iter().any(|entry| entry == name);
    if !global_exists && !in_config {
        return Err(format!("plugin '{name}' is not installed"));
    }
    disable_plugin_in_config(instance_home, name)?;
    eprintln!("Disabled plugin: {name} (for instance '{instance}')");
    if plugin_args.iter().any(|arg| arg == "--purge") {
        crate::plugin_manager::uninstall(home, name)?;
        eprintln!("Removed plugin files: {name}");
    }
    crate::deploy::reload_running_daemon_config(args);
    hint_plugin_apply_if_running(args, home, name, false);
    Ok(())
}

fn enable_plugin_in_config(instance_home: &Path, plugin_name: &str) -> Result<(), String> {
    let config_path = crate::deploy::config_path_for_instance_home(instance_home);
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    if content.contains(&format!("\"{plugin_name}\""))
        && content.contains("[plugins]")
        && content.contains("enabled")
    {
        return Ok(());
    }

    let mut enabled = read_enabled_plugins(instance_home);
    if !enabled.iter().any(|entry| entry == plugin_name) {
        enabled.push(plugin_name.to_string());
    }

    write_enabled_plugins(&config_path, &content, &enabled)
}

fn disable_plugin_in_config(instance_home: &Path, plugin_name: &str) -> Result<(), String> {
    let config_path = crate::deploy::config_path_for_instance_home(instance_home);
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    let mut enabled = read_enabled_plugins(instance_home);
    enabled.retain(|entry| entry != plugin_name);

    write_enabled_plugins(&config_path, &content, &enabled)
}

fn read_enabled_plugins(instance_home: &Path) -> Vec<String> {
    let config_path = crate::deploy::config_path_for_instance_home(instance_home);
    let Ok(content) = fs::read_to_string(&config_path) else {
        return Vec::new();
    };
    let mut in_plugins = false;
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[plugins]" {
            in_plugins = true;
            continue;
        }
        if trimmed.starts_with('[') && trimmed != "[plugins]" {
            in_plugins = false;
        }
        if in_plugins && let Some(value) = trimmed.strip_prefix("enabled") {
            let value = value.trim().strip_prefix('=').unwrap_or(value).trim();
            return value
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|item| item.trim().trim_matches('"').to_string())
                .filter(|item| !item.is_empty())
                .collect();
        }
    }
    Vec::new()
}

fn write_enabled_plugins(
    config_path: &Path,
    content: &str,
    enabled: &[String],
) -> Result<(), String> {
    let enabled_line = format!(
        "enabled = [{}]",
        enabled
            .iter()
            .map(|plugin| format!("\"{plugin}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut lines: Vec<String> = Vec::new();
    let mut in_plugins = false;
    let mut replaced = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[plugins]" {
            in_plugins = true;
        } else if trimmed.starts_with('[') {
            in_plugins = false;
        }
        if in_plugins && trimmed.starts_with("enabled") {
            lines.push(enabled_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        lines.push(String::new());
        lines.push("[plugins]".to_string());
        lines.push(enabled_line);
    }

    fs::write(config_path, lines.join("\n"))
        .map_err(|err| format!("cannot write {}: {err}", config_path.display()))
}
