use std::fs;

use crate::deploy::{
    config_path_for_instance_home, parse_instance_id, resolve_paths_from_args, service_name,
    systemctl,
};

/// `cortex reset [--id ID] [--force] [--factory]`
///
/// Two modes:
/// - Default: clear data (sessions, memory, data, prompts, skills) but
///   preserve `config.toml` so the user doesn't lose their configuration.
/// - `--factory`: full factory reset — delete everything and recreate
///   from scratch (identical to first-ever launch).
///
/// `--force` / `-f` skips confirmation prompts and auto-stops the daemon.
///
/// # Errors
/// Returns an error string if the reset fails.
pub fn cmd_reset(args: &[String]) -> Result<(), String> {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    let paths = resolve_paths_from_args(args);
    let home_path = paths.instance_home();
    let force = args.iter().any(|a| a == "--force" || a == "-f");
    let factory = args.iter().any(|a| a == "--factory");

    if !home_path.exists() {
        eprintln!("Instance '{id}' does not exist.");
        return Ok(());
    }

    let daemon_running = home_path.join("data/cortex.sock").exists();
    if daemon_running {
        if !force {
            eprintln!("Warning: daemon is running. It will be stopped before reset.");
            eprint!("Continue? [y/N] ");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .map_err(|e| format!("read input: {e}"))?;
            if input.trim().to_lowercase() != "y" {
                eprintln!("Cancelled.");
                return Ok(());
            }
        }
        let svc = service_name(paths.base_dir(), instance_id.as_deref(), false);
        let _ = systemctl(&["stop", &svc], false);
        std::thread::sleep(std::time::Duration::from_secs(1));
    }

    if !force {
        let mode_desc = if factory {
            "FACTORY RESET: delete everything (including config) and recreate from scratch"
        } else {
            "Reset: clear data, memory, sessions, prompts, and skills (config.toml preserved)"
        };
        eprint!(
            "{mode_desc}\nInstance '{id}' at {}\nConfirm? [y/N] ",
            home_path.display()
        );
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("read input: {e}"))?;
        if input.trim().to_lowercase() != "y" {
            eprintln!("Cancelled.");
            return Ok(());
        }
    }

    let _ = fs::remove_file(home_path.join("data/cortex.sock"));

    if factory {
        fs::remove_dir_all(&home_path)
            .map_err(|e| format!("failed to delete {}: {e}", home_path.display()))?;
        eprintln!("Factory reset: {}", home_path.display());
    } else {
        let config_path = config_path_for_instance_home(&home_path);
        let config_backup = fs::read(&config_path).ok();
        fs::remove_dir_all(&home_path)
            .map_err(|e| format!("failed to delete {}: {e}", home_path.display()))?;
        cortex_kernel::ensure_home_dirs(&home_path)
            .map_err(|e| format!("failed to recreate: {e}"))?;
        if let Some(config_data) = config_backup {
            cortex_kernel::atomic_write(&config_path, &config_data)
                .map_err(|e| format!("failed to restore config.toml: {e}"))?;
        }
        eprintln!("Instance '{id}' reset complete — config preserved.");
    }
    if daemon_running {
        eprintln!("Restart daemon: cortex restart");
    }
    Ok(())
}
