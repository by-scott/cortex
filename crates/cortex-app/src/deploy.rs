use std::fs;
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use crate::deploy_args::{parse_home_arg, parse_instance_id, parse_system_flag};
pub(crate) use crate::deploy_help::DeployCommandSpec;
use crate::deploy_help::{DeploySubcommand, parse_deploy_subcommand};
pub use crate::deploy_reset::cmd_reset;
pub use crate::deploy_unit::{generate_system_unit_file, generate_unit_file};

const SERVICE_NAME: &str = "cortex";

/// Resolve the systemd service name for a given instance.
pub(crate) fn service_name(base_dir: &Path, instance_id: Option<&str>, system: bool) -> String {
    let default_base = if system {
        PathBuf::from(SYSTEM_CORTEX_HOME)
    } else {
        PathBuf::from(resolve_cortex_home())
    };
    let instance_id = instance_id.unwrap_or("default");

    if base_dir == default_base {
        if instance_id == "default" {
            SERVICE_NAME.to_string()
        } else {
            format!("{SERVICE_NAME}@{instance_id}")
        }
    } else {
        let suffix = service_home_suffix(base_dir);
        if instance_id == "default" {
            format!("{SERVICE_NAME}-{suffix}")
        } else {
            format!("{SERVICE_NAME}-{suffix}@{instance_id}")
        }
    }
}

pub(crate) fn resolve_paths_from_args(args: &[String]) -> cortex_kernel::CortexPaths {
    resolve_paths(args, false)
}

pub(crate) fn resolve_paths(args: &[String], system: bool) -> cortex_kernel::CortexPaths {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    let base = if system {
        parse_home_arg(args).unwrap_or_else(|| SYSTEM_CORTEX_HOME.to_string())
    } else {
        parse_home_arg(args).unwrap_or_else(resolve_cortex_home)
    };
    cortex_kernel::CortexPaths::new(base, id)
}

fn service_home_suffix(base_dir: &Path) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in base_dir.to_string_lossy().bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Check if the base directory has any remaining instance directories.
/// If none remain, remove the base directory itself.
fn cleanup_base_if_empty(base: &Path, system: bool) {
    let Ok(metadata) = fs::metadata(base) else {
        return;
    };
    if !metadata.is_dir() {
        return;
    }

    let has_instance = !cortex_runtime::InstanceManager::new(base).list().is_empty();
    if !has_instance {
        let removed = if system {
            Command::new("sudo")
                .args(["rmdir", &base.to_string_lossy()])
                .output()
                .is_ok_and(|output| output.status.success())
        } else {
            fs::remove_dir_all(base).is_ok()
        };
        if removed {
            eprintln!("Removed empty base directory: {}", base.display());
        }
    }
}

fn user_unit_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home)
        .join(".config")
        .join("systemd")
        .join("user")
}

pub(crate) fn user_unit_path_for(svc_name: &str) -> PathBuf {
    user_unit_dir().join(format!("{svc_name}.service"))
}

pub(crate) fn system_unit_path_for(svc_name: &str) -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(format!("{svc_name}.service"))
}

fn resolve_cortex_bin() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("cortex"))
        .to_string_lossy()
        .to_string()
}

/// Resolve the `CORTEX_HOME` base directory from environment or default.
#[must_use]
pub fn resolve_cortex_home() -> String {
    if let Ok(v) = std::env::var("CORTEX_HOME") {
        return v;
    }
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    format!("{home}/.cortex")
}

pub(crate) const SYSTEM_CORTEX_HOME: &str = "/var/lib/cortex";

fn systemctl_user(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("systemctl")
        .arg("--user")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run systemctl: {e}"))
}

fn systemctl_system(args: &[&str]) -> Result<std::process::Output, String> {
    Command::new("sudo")
        .arg("systemctl")
        .args(args)
        .output()
        .map_err(|e| format!("failed to run sudo systemctl: {e}"))
}

pub(crate) fn systemctl(args: &[&str], system: bool) -> Result<std::process::Output, String> {
    if system {
        systemctl_system(args)
    } else {
        systemctl_user(args)
    }
}

pub(crate) fn check_linux() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        Ok(())
    } else {
        Err("service commands require Linux (systemd).".to_string())
    }
}

fn wait_for_daemon_ready(paths: &cortex_kernel::CortexPaths, system: bool) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    let socket_path = paths.socket_path();
    while std::time::Instant::now() < deadline {
        if socket_path.exists() {
            let ready = if system {
                cortex_runtime::DaemonClient::connect_socket(&socket_path).is_ok()
                    || fs::metadata(&socket_path)
                        .is_ok_and(|metadata| metadata.file_type().is_socket())
            } else {
                cortex_runtime::DaemonClient::connect_socket(&socket_path).is_ok()
            };
            if ready {
                return Ok(());
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }

    Err(format!(
        "daemon did not become ready within timeout (socket: {})",
        socket_path.display()
    ))
}

pub(crate) fn refresh_user_launcher_for_home(
    home_dir: &Path,
    cortex_bin: &str,
) -> Result<(), String> {
    let local_bin_dir = home_dir.join(".local/bin");
    fs::create_dir_all(&local_bin_dir)
        .map_err(|e| format!("failed to create {}: {e}", local_bin_dir.display()))?;
    let launcher_path = local_bin_dir.join("cortex");
    let launcher_real = launcher_path
        .canonicalize()
        .unwrap_or_else(|_| launcher_path.clone());
    let cortex_real = PathBuf::from(cortex_bin)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(cortex_bin));

    if launcher_real == cortex_real {
        return Ok(());
    }

    if launcher_path.exists() || launcher_path.is_symlink() {
        fs::remove_file(&launcher_path)
            .map_err(|e| format!("failed to replace {}: {e}", launcher_path.display()))?;
    }
    unix_fs::symlink(cortex_bin, &launcher_path).map_err(|e| {
        format!(
            "failed to link {} -> {cortex_bin}: {e}",
            launcher_path.display()
        )
    })
}

fn refresh_user_launcher(cortex_bin: &str) -> Result<(), String> {
    let home_dir = std::env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or_else(|| "cannot resolve home directory for launcher update".to_string())?;
    refresh_user_launcher_for_home(&home_dir, cortex_bin)
}

fn deploy_user(cortex_bin: &str, args: &[String]) -> Result<(), String> {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    // Validate instance ID before any filesystem operations
    if let Some(ref raw_id) = instance_id
        && let Err(e) = crate::cli::validate_instance_id(raw_id)
    {
        return Err(e);
    }
    let paths = resolve_paths_from_args(args);
    let permission_level = crate::deploy_permission::parse_install_permission_level(args)?
        .unwrap_or_else(crate::deploy_permission::default_permission_level);
    let base = paths.base_dir().to_string_lossy().to_string();
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), false);

    if id != "default" {
        let mgr = cortex_runtime::InstanceManager::new(&PathBuf::from(&base));
        mgr.ensure_instance(id)
            .map_err(|e| format!("failed to create instance directory: {e}"))?;
    }

    // Pre-generate config.toml from env vars (before daemon starts).
    // The daemon process won't inherit the caller's env vars via systemd,
    // so we must generate config here while env vars are available.
    let instance_home = paths.instance_home();
    cortex_kernel::ensure_home_dirs(&instance_home)
        .map_err(|e| format!("failed to create instance dirs: {e}"))?;

    // Ensure global plugins directory exists.
    let plugins_dir = paths.plugins_dir();
    let _ = fs::create_dir_all(&plugins_dir);

    let config_path = paths.config_path();
    let has_env_config = std::env::var("CORTEX_API_KEY").is_ok()
        || std::env::var("CORTEX_PROVIDER").is_ok()
        || std::env::var("CORTEX_MODEL").is_ok()
        || std::env::var("CORTEX_BASE_URL").is_ok()
        || std::env::var("CORTEX_LLM_PRESET").is_ok()
        || std::env::var("CORTEX_EMBEDDING_PROVIDER").is_ok()
        || std::env::var("CORTEX_EMBEDDING_MODEL").is_ok()
        || std::env::var("CORTEX_EMBEDDING_BASE_URL").is_ok()
        || std::env::var("CORTEX_EMBEDDING_API_KEY").is_ok()
        || std::env::var("CORTEX_SHOW_THINKING").is_ok()
        || std::env::var("CORTEX_STRIP_THINK_TAGS").is_ok()
        || std::env::var("CORTEX_BRAVE_KEY").is_ok()
        || std::env::var("CORTEX_TELEGRAM_TOKEN").is_ok()
        || std::env::var("CORTEX_WHATSAPP_TOKEN").is_ok()
        || std::env::var("CORTEX_QQ_APP_ID").is_ok()
        || std::env::var("CORTEX_QQ_APP_SECRET").is_ok();
    if !config_path.exists() || has_env_config {
        // Regenerate config when env vars are provided (even if config exists)
        // to ensure install always applies the caller's configuration.
        if config_path.exists() && has_env_config {
            let _ = fs::remove_file(&config_path);
        }
        cortex_kernel::ensure_base_dirs(paths.base_dir())
            .map_err(|e| format!("ensure base: {e}"))?;
        let (providers, resolved) =
            cortex_kernel::load_providers_for_paths(&paths).unwrap_or_default();
        let _ = cortex_kernel::load_config_for_paths(&paths, resolved.as_deref(), &providers);
    }
    crate::deploy_permission::update_install_permission_level(&config_path, permission_level)?;

    // CORTEX_HOME = base path (e.g. ~/.cortex), --id selects instance.
    let unit_content = generate_unit_file(cortex_bin, &base, id);
    let unit_dir = user_unit_dir();
    fs::create_dir_all(&unit_dir).map_err(|e| format!("failed to create systemd user dir: {e}"))?;
    let upath = user_unit_path_for(&svc);

    if upath.exists() {
        let _ = systemctl(&["stop", &svc], false);
        eprintln!("Stopped existing service, reinstalling...");
    }

    fs::write(&upath, unit_content).map_err(|e| format!("failed to write unit file: {e}"))?;
    systemctl(&["daemon-reload"], false)?;

    let enable = systemctl(&["enable", &svc], false)?;
    if !enable.status.success() {
        return Err(format!(
            "enable failed: {}",
            String::from_utf8_lossy(&enable.stderr)
        ));
    }

    let start = systemctl(&["start", &svc], false)?;
    if !start.status.success() {
        return Err(format!(
            "start failed: {}",
            String::from_utf8_lossy(&start.stderr)
        ));
    }
    wait_for_daemon_ready(&paths, false)?;
    refresh_user_launcher(cortex_bin)?;

    let user = std::env::var("USER").unwrap_or_default();
    if !user.is_empty() {
        let _ = Command::new("loginctl")
            .args(["enable-linger", &user])
            .output();
    }

    eprintln!("Installed successfully!");
    eprintln!("  Service:   {svc}");
    eprintln!("  Unit file: {}", upath.display());
    eprintln!("  Binary:    {cortex_bin}");
    eprintln!("  Data dir:  {}", paths.data_dir().display());
    eprintln!(
        "  Permission: {} (auto-approve up to {permission_level:?})",
        crate::deploy_permission::permission_level_label(permission_level)
    );
    eprintln!("  Status:    cortex status");
    Ok(())
}

/// `cortex install [--user|--system] [--id ID]`
///
/// # Errors
/// Returns an error string if installation fails.
pub fn cmd_deploy(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let cortex_bin = resolve_cortex_bin();

    if system {
        let paths = resolve_paths(args, true);
        let cortex_home = paths.base_dir().to_string_lossy().to_string();
        let id = paths.instance_id();
        let svc = service_name(paths.base_dir(), Some(id), true);
        let unit_content = generate_system_unit_file(&cortex_bin, &cortex_home, id);
        let upath = system_unit_path_for(&svc);

        if upath.exists() {
            let _ = systemctl(&["stop", &svc], true);
            eprintln!("Stopped existing service, reinstalling...");
        }

        let tee = Command::new("sudo")
            .args(["tee", &upath.to_string_lossy()])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(ref mut stdin) = child.stdin {
                    stdin.write_all(unit_content.as_bytes())?;
                }
                child.wait()
            })
            .map_err(|e| format!("failed to write system unit: {e}"))?;

        if !tee.success() {
            return Err("failed to write system unit (insufficient permissions?)".to_string());
        }

        systemctl(&["daemon-reload"], true)?;
        let enable = systemctl(&["enable", &svc], true)?;
        if !enable.status.success() {
            return Err(format!(
                "enable failed: {}",
                String::from_utf8_lossy(&enable.stderr)
            ));
        }
        let start = systemctl(&["start", &svc], true)?;
        if !start.status.success() {
            return Err(format!(
                "start failed: {}",
                String::from_utf8_lossy(&start.stderr)
            ));
        }
        wait_for_daemon_ready(&paths, true)?;

        eprintln!("System-level install successful!");
        eprintln!("  Service:   {svc}");
        eprintln!("  Unit file: {}", upath.display());
        eprintln!("  Binary:    {cortex_bin}");
        eprintln!("  Data dir:  {cortex_home}");
        eprintln!("  Note: ensure cortex user exists: sudo useradd -r -s /bin/false cortex");
        eprintln!(
            "  Note: ensure data dir: sudo mkdir -p {cortex_home} && sudo chown cortex:cortex {cortex_home}"
        );
        eprintln!("  Status:    cortex status --system");
    } else {
        deploy_user(&cortex_bin, args)?;
    }

    Ok(())
}

/// `cortex uninstall [--purge] [--system]`
///
/// # Errors
/// Returns an error string if the removal fails.
pub fn cmd_undeploy(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let purge = args.iter().any(|a| a == "--purge");
    let instance_id = parse_instance_id(args);
    let paths = resolve_paths(args, system);
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), system);

    // Stop and disable the correct service (instance-specific).
    let status = systemctl(&["is-enabled", &svc], system);
    if status.is_ok_and(|s| s.status.success()) {
        let _ = systemctl(&["stop", &svc], system);
        let _ = systemctl(&["disable", &svc], system);
        // Remove the unit file for non-default instances.
        if system {
            let _ = Command::new("sudo")
                .args(["rm", "-f", &system_unit_path_for(&svc).to_string_lossy()])
                .output();
        } else {
            let _ = fs::remove_file(user_unit_path_for(&svc));
        }
        let _ = systemctl(&["daemon-reload"], system);
        eprintln!("Service stopped and removed.");
    } else {
        eprintln!("Service not installed.");
    }

    // Without --purge, only remove socket file — all data and config preserved.
    // `cortex ps` uses socket presence to detect running instances.
    if !purge {
        let socket_path = paths.socket_path();
        let _ = fs::remove_file(socket_path);
    }

    if purge {
        let instance_home = paths.instance_home();
        let base_dir = paths.base_dir().to_string_lossy().to_string();
        let home_path = instance_home.clone();
        if home_path.exists() {
            // Remove socket first (fs::remove_dir_all may fail on Unix sockets).
            let socket = paths.socket_path();
            let _ = fs::remove_file(&socket);
            if system {
                let _ = Command::new("sudo")
                    .args(["rm", "-rf", &instance_home.to_string_lossy()])
                    .output();
            } else {
                fs::remove_dir_all(&home_path)
                    .map_err(|e| format!("failed to clean instance dir: {e}"))?;
            }
            // Remove base if no instances remain
            cleanup_base_if_empty(&PathBuf::from(&base_dir), system);
            eprintln!("Cleaned instance: {}", instance_home.display());
        }
    }

    Ok(())
}

/// `cortex start [--system] [--id ID]`
///
/// # Errors
/// Returns an error string if the service cannot be started.
pub fn cmd_start(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let instance_id = parse_instance_id(args);
    let paths = resolve_paths(args, system);
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), system);

    if !(if system {
        system_unit_path_for(&svc).exists()
    } else {
        user_unit_path_for(&svc).exists()
    }) {
        let flag = if system { " --system" } else { "" };
        return Err(format!(
            "service not installed, run `cortex install{flag}` first."
        ));
    }

    let out = systemctl(&["start", &svc], system)?;
    if out.status.success() {
        wait_for_daemon_ready(&paths, system)?;
        eprintln!("Service started: {svc}");
        Ok(())
    } else {
        Err(format!(
            "start failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// `cortex stop [--system] [--id ID]`
///
/// # Errors
/// Returns an error string if the service cannot be stopped.
pub fn cmd_stop(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let instance_id = parse_instance_id(args);
    let paths = resolve_paths(args, system);
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), system);

    let out = systemctl(&["stop", &svc], system)?;
    if out.status.success() {
        eprintln!("Service stopped: {svc}");
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if stderr.contains("not loaded") || stderr.contains("not found") {
            eprintln!("Service not running.");
        } else {
            return Err(format!("stop failed: {stderr}"));
        }
    }
    Ok(())
}

/// `cortex restart [--system]`
///
/// # Errors
/// Returns an error string if the service cannot be restarted.
pub fn cmd_restart(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let instance_id = parse_instance_id(args);
    let paths = resolve_paths(args, system);
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), system);

    if !(if system {
        system_unit_path_for(&svc).exists()
    } else {
        user_unit_path_for(&svc).exists()
    }) {
        let flag = if system { " --system" } else { "" };
        return Err(format!(
            "service not installed, run `cortex install{flag}` first."
        ));
    }

    let out = systemctl(&["restart", &svc], system)?;
    if out.status.success() {
        wait_for_daemon_ready(&paths, system)?;
        eprintln!("Service restarted: {svc}");
        Ok(())
    } else {
        Err(format!(
            "restart failed: {}",
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// `cortex ps` -- list all instances with running status.
///
/// # Errors
/// Returns an error string if instance discovery fails.
pub fn cmd_ps(home_override: Option<String>) -> Result<(), String> {
    check_linux()?;
    // Respect --home from process args (parsed before subcommand dispatch)
    let cortex_home = home_override
        .or_else(|| {
            let args: Vec<String> = std::env::args().collect();
            args.windows(2)
                .find(|w| w[0] == "--home")
                .map(|w| w[1].clone())
        })
        .unwrap_or_else(resolve_cortex_home);
    let base = PathBuf::from(&cortex_home);
    let mgr = cortex_runtime::InstanceManager::new(&base);
    let instances = mgr.list();

    eprintln!("{:<12} {:<10} PATH", "INSTANCE", "STATUS");
    eprintln!("{}", "-".repeat(50));

    for inst in &instances {
        // Skip instance dirs that lack config (e.g. leftover after purge).
        if !inst.config_exists {
            continue;
        }
        let socket_path = inst.home_path.join("data/cortex.sock");
        let svc = service_name(
            base.as_path(),
            Some(inst.id.as_str()).filter(|id| *id != "default"),
            false,
        );
        let has_service = user_unit_path_for(&svc).exists();
        let running = cortex_runtime::DaemonClient::is_daemon_running(&inst.home_path);
        let status = if running {
            "running"
        } else if has_service {
            "stopped"
        } else {
            "uninstalled"
        };
        eprintln!("{:<12} {:<10} {}", inst.id, status, socket_path.display());
    }
    Ok(())
}

pub(crate) fn reload_running_daemon_config(args: &[String]) {
    let system = parse_system_flag(args);
    if system {
        return;
    }
    let paths = resolve_paths(args, system);
    let Ok(client) = cortex_runtime::DaemonClient::connect_socket(&paths.socket_path()) else {
        return;
    };
    let _ = client.send_rpc("admin/reload-config", &serde_json::json!({}));
}

pub(crate) fn config_path_for_instance_home(instance_home: &Path) -> PathBuf {
    cortex_kernel::CortexPaths::from_instance_home(instance_home)
        .config_files()
        .config
}

pub(crate) fn ensure_instance_home_exists(
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    if instance_home.exists() {
        Ok(())
    } else {
        Err(format!("instance '{instance}' does not exist"))
    }
}

/// Dispatch subcommand. Returns `Some(Ok/Err)` if handled, `None` if not a deploy subcommand.
#[must_use]
pub fn dispatch(cmd: &str, remaining_args: &[String]) -> Option<Result<(), String>> {
    parse_deploy_subcommand(cmd)
        .map(|subcommand| dispatch_deploy_subcommand(subcommand, remaining_args))
}

pub(crate) const fn deploy_command_specs() -> &'static [DeployCommandSpec] {
    crate::deploy_help::deploy_command_specs()
}

fn dispatch_deploy_subcommand(
    subcommand: DeploySubcommand,
    remaining_args: &[String],
) -> Result<(), String> {
    match subcommand {
        DeploySubcommand::Install => cmd_deploy(remaining_args),
        DeploySubcommand::Uninstall => cmd_undeploy(remaining_args),
        DeploySubcommand::Start => cmd_start(remaining_args),
        DeploySubcommand::Stop => cmd_stop(remaining_args),
        DeploySubcommand::Restart => cmd_restart(remaining_args),
        DeploySubcommand::Status => crate::deploy_status::cmd_status(remaining_args),
        DeploySubcommand::Doctor => crate::deploy_doctor::cmd_doctor(remaining_args),
        DeploySubcommand::Ps => cmd_ps(None),
        DeploySubcommand::Reset => cmd_reset(remaining_args),
        DeploySubcommand::Plugin => crate::deploy_plugin::cmd_plugin(remaining_args),
        DeploySubcommand::Channel => {
            crate::deploy_channel::cmd_channel(remaining_args);
            Ok(())
        }
        DeploySubcommand::Actor => {
            crate::deploy_actor::cmd_actor(remaining_args);
            Ok(())
        }
        DeploySubcommand::Node => crate::deploy_node::cmd_node(remaining_args),
        DeploySubcommand::Browser => crate::deploy_node::cmd_browser(remaining_args),
        DeploySubcommand::Permission => crate::deploy_permission::cmd_permission(remaining_args),
        DeploySubcommand::Config => crate::deploy_config::cmd_config(remaining_args),
        DeploySubcommand::Policy => crate::deploy_policy::cmd_policy(remaining_args),
    }
}
