use std::fs;
use std::os::unix::fs as unix_fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_NAME: &str = "cortex";
const PH_CORTEX_BIN: &str = "{cortex_bin}";
const PH_CORTEX_HOME: &str = "{cortex_home}";
const PH_CORTEX_ID: &str = "{cortex_id}";

const PH_PATH: &str = "{path}";

const USER_UNIT_TEMPLATE: &str = r"[Unit]
Description=Cortex Cognitive Harness
After=network.target

[Service]
Type=simple
ExecStart={cortex_bin} --daemon --id {cortex_id}
Environment=CORTEX_HOME={cortex_home}
Environment=PATH={path}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=default.target
";

const SYSTEM_UNIT_TEMPLATE: &str = r"[Unit]
Description=Cortex Cognitive Harness
After=network.target

[Service]
Type=simple
User=cortex
ExecStart={cortex_bin} --daemon --id {cortex_id}
Environment=CORTEX_HOME={cortex_home}
Environment=PATH={path}
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
";

/// Generate systemd user service unit file content with resolved paths.
#[must_use]
pub fn generate_unit_file(cortex_bin: &str, cortex_home: &str, instance_id: &str) -> String {
    // Capture the caller's PATH so verify_contract and other tools can find cargo etc.
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin".into());
    USER_UNIT_TEMPLATE
        .replace(PH_CORTEX_BIN, cortex_bin)
        .replace(PH_CORTEX_HOME, cortex_home)
        .replace(PH_CORTEX_ID, instance_id)
        .replace(PH_PATH, &path_env)
}

/// Generate systemd system-level service unit file content with resolved paths.
#[must_use]
pub fn generate_system_unit_file(cortex_bin: &str, cortex_home: &str, instance_id: &str) -> String {
    let path_env = std::env::var("PATH").unwrap_or_else(|_| "/usr/local/bin:/usr/bin".into());
    SYSTEM_UNIT_TEMPLATE
        .replace(PH_CORTEX_BIN, cortex_bin)
        .replace(PH_CORTEX_HOME, cortex_home)
        .replace(PH_CORTEX_ID, instance_id)
        .replace(PH_PATH, &path_env)
}

/// Parse `--system` flag from argument list.
#[must_use]
pub fn parse_system_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--system")
}

/// Parse `--id <ID>` from argument list.
#[must_use]
pub fn parse_instance_id(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--id")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Parse `--home <PATH>` from argument list.
#[must_use]
pub fn parse_home_arg(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--home")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

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

    // Always stop the daemon first if it's running.
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

    // Remove Unix socket first (fs::remove_dir_all may fail on sockets).
    let _ = fs::remove_file(home_path.join("data/cortex.sock"));

    if factory {
        // Factory reset: delete everything (including config) so the next
        // `cortex install` treats this as a first-time setup and applies
        // env vars (CORTEX_API_KEY, etc.) to generate a fresh config.
        fs::remove_dir_all(&home_path)
            .map_err(|e| format!("failed to delete {}: {e}", home_path.display()))?;
        eprintln!("Factory reset: {}", home_path.display());
    } else {
        // Default reset: preserve config.toml, clear everything else.
        let config_path = config_path_for_instance_home(&home_path);
        let config_backup = fs::read(&config_path).ok();
        fs::remove_dir_all(&home_path)
            .map_err(|e| format!("failed to delete {}: {e}", home_path.display()))?;
        cortex_kernel::ensure_home_dirs(&home_path)
            .map_err(|e| format!("failed to recreate: {e}"))?;
        if let Some(config_data) = config_backup {
            fs::write(config_path, config_data)
                .map_err(|e| format!("failed to restore config.toml: {e}"))?;
        }
        eprintln!("Instance '{id}' reset complete — config preserved.");
    }
    if daemon_running {
        eprintln!("Restart daemon: cortex restart");
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeploySubcommand {
    Install,
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
    Demo,
    Doctor,
    Ps,
    Reset,
    Plugin,
    Channel,
    Actor,
    Node,
    Browser,
    Permission,
    Config,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DeployCommandSpec {
    subcommand: DeploySubcommand,
    names: &'static [&'static str],
    summary: &'static str,
    help: Option<&'static str>,
}

impl DeployCommandSpec {
    #[must_use]
    pub const fn primary_name(self) -> &'static str {
        self.names[0]
    }

    #[must_use]
    pub const fn names(self) -> &'static [&'static str] {
        self.names
    }

    #[must_use]
    pub const fn summary(self) -> &'static str {
        self.summary
    }

    #[must_use]
    pub const fn help(self) -> Option<&'static str> {
        self.help
    }
}

const DEPLOY_COMMAND_SPECS: &[DeployCommandSpec] = &[
    DeployCommandSpec {
        subcommand: DeploySubcommand::Install,
        names: &["install"],
        summary: "Install as systemd service",
        help: Some(
            "cortex install — Install as a systemd user service and start the daemon.\n\n\
Usage: cortex install [OPTIONS]\n\n\
Options:\n\
  --id <ID>       Instance ID (default: default)\n\
  --system        Install as system-level service (requires root)\n\
  --permission-level <strict|balanced|open>\n\
                  Tool confirmation policy: strict=Allow only, balanced=Review,\n\
                  open=all non-blocking tools without confirmation.\n\
                  Defaults to balanced when omitted.\n\n\
Environment variables (first install only):\n\
  CORTEX_API_KEY              LLM API key\n\
  CORTEX_PROVIDER             LLM provider (e.g. zai, anthropic, openai)\n\
  CORTEX_MODEL                LLM model name\n\
  CORTEX_BASE_URL             Custom provider base URL\n\
  CORTEX_LLM_PRESET           Preset (minimal, standard, cognitive, full)\n\
  CORTEX_EMBEDDING_PROVIDER   Embedding provider (e.g. ollama)\n\
  CORTEX_EMBEDDING_MODEL      Embedding model name\n\
  CORTEX_EMBEDDING_BASE_URL   Embedding provider base URL\n\
  CORTEX_EMBEDDING_API_KEY    Embedding provider API key\n\
  CORTEX_SHOW_THINKING        Enable provider thinking request/output (default false)\n\
  CORTEX_BRAVE_KEY            Brave Search API key\n\n\
  CORTEX_PERMISSION_LEVEL     Same values as --permission-level\n\n\
If a service already exists it will be stopped and reinstalled.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Uninstall,
        names: &["uninstall"],
        summary: "Remove service",
        help: Some(
            "cortex uninstall — Remove the systemd service.\n\n\
Usage: cortex uninstall [OPTIONS]\n\n\
Options:\n\
  --id <ID>     Instance ID (default: default)\n\
  --purge       Also delete all instance data (config, memory, sessions)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Start,
        names: &["start"],
        summary: "Start daemon",
        help: Some(
            "cortex start — Start the daemon via systemd.\n\nUsage: cortex start [--id <ID>]",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Stop,
        names: &["stop"],
        summary: "Stop daemon",
        help: Some("cortex stop — Stop the daemon via systemd.\n\nUsage: cortex stop [--id <ID>]"),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Restart,
        names: &["restart"],
        summary: "Restart daemon",
        help: Some(
            "cortex restart — Restart the daemon via systemd.\n\nUsage: cortex restart [--id <ID>]",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Status,
        names: &["status"],
        summary: "Show daemon status",
        help: Some(
            "cortex status — Show daemon status.\n\n\
Usage: cortex status [--id <ID>]\n\n\
Displays: active state, PID, socket path, data directory, HTTP address,\n\
          current LLM provider/model/preset, permission mode, context and token usage.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Demo,
        names: &["demo"],
        summary: "Create a local first-run demo fixture",
        help: Some(
            "cortex demo — Create a local first-run demo fixture.\n\n\
Usage: cortex demo [--id <ID>] [--home <PATH>] [--force]\n\n\
Creates a user-local instance (default id: demo), an external demo workspace,\n\
an Ollama-oriented config, empty MCP config, and a local-coding demo skill.\n\
The command does not start services, enable plugins, broaden permissions, or\n\
modify protected runtime state outside the selected demo instance. Use --force\n\
to refresh demo-owned files when the target instance already exists.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Doctor,
        names: &["doctor"],
        summary: "Run local readiness checks",
        help: Some(
            "cortex doctor — Run local readiness checks without changing runtime state.\n\n\
Usage: cortex doctor [--id <ID>] [--system] [--json]\n\n\
Checks OS/systemd availability, instance paths, service/socket state, config,\n\
provider key posture, permission mode, enabled plugins, channel auth,\n\
policy lint findings, protected runtime root paths, and local model endpoint hints.\n\
Use --json for a machine-readable report with remediation hints.\n\
Findings are operator guidance; policy/risk gates are not sandbox containment.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Ps,
        names: &["ps"],
        summary: "List all instances",
        help: Some(
            "cortex ps — List all instances with their status.\n\n\
Usage: cortex ps\n\n\
Shows instance name, status (running/stopped/uninstalled), and socket path.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Reset,
        names: &["reset"],
        summary: "Clear data (keep config); --factory for full wipe",
        help: Some(
            "cortex reset — Clear instance data while preserving configuration.\n\n\
Usage: cortex reset [OPTIONS]\n\n\
Options:\n\
  --id <ID>     Instance ID (default: default)\n\
  --force, -f   Skip confirmation and auto-stop the daemon if running\n\
  --factory     Factory reset: delete everything including config and\n\
                recreate the instance from scratch\n\n\
By default, reset preserves config.toml and clears data, memory,\n\
sessions, prompts, and skills. With --factory, the entire instance\n\
directory is deleted and recreated as if freshly installed.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Plugin,
        names: &["plugin"],
        summary: "Manage plugins",
        help: Some(
            "cortex plugin — Manage plugins.\n\n\
Subcommands:\n\
  install <source>    Install from .cpx file, URL, directory, or name[@version]\n\
                      Names resolve to GitHub: github.com/by-scott/cortex-plugin-<name>\n\
                      Packaged installs require a valid Ed25519 package signature;\n\
                      add --yes after reviewing a new verified publisher key\n\
  enable <name>       Enable an installed plugin for one instance\n\
  disable <name>      Disable an installed plugin for one instance\n\
  uninstall <name>    Disable for one instance; add --purge to remove files\n\
  list                List installed plugins with status\n\
  review <dir>        Show capability, signature, sandbox, and risk summary\n\
  test <dir>          Run the local plugin conformance kit\n\
  keygen <path>       Create a local Ed25519 plugin signing key\n\
  sign <dir> --key <path> [--publisher <id>]\n\
                      Write signed package.toml metadata for publishing\n\
  pack <dir> [out]    Create .cpx archive; default is <repo>-v<version>-<platform>.cpx",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Actor,
        names: &["actor"],
        summary: "Manage actor aliases and transport bindings",
        help: Some(
            "cortex actor — Identity mapping for unified session ownership.\n\n\
Subcommands:\n\
  alias list                    List actor aliases\n\
  alias set <from> <to>         Map one actor to a canonical actor\n\
  alias unset <from>            Remove an actor alias\n\
  transport list                List transport actor bindings\n\
  transport set <name|all> <actor>  Bind transport to actor (all = http,rpc,ws,sock,stdio)\n\
  transport unset <name>            Remove transport binding\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Channel,
        names: &["channel"],
        summary: "Manage channel pairing and policy",
        help: Some(
            "cortex channel — Messaging channel management.\n\n\
Channels run inside the daemon automatically when auth.json exists.\n\n\
Subcommands:\n\
  telegram              Show Telegram configuration info\n\
  whatsapp              Show WhatsApp configuration info\n\
  qq                    Show QQ configuration info\n\
  pair [platform]       Show pending/paired users\n\
  subscribe <plat> <id> Enable session subscription for a paired user\n\
  unsubscribe <plat> <id>\n\
                        Disable session subscription for a paired user\n\
  approve <plat> <id> [--subscribe|--no-subscribe]\n\
                        Approve a user and optionally configure subscription\n\
  revoke <plat> <id>    Remove a paired user\n\
  allow <plat> <id>     Add user to whitelist\n\
  deny <plat> <id>      Add user to blacklist\n\
  unallow <plat> <id>   Remove from whitelist\n\
  undeny <plat> <id>    Remove from blacklist\n\
  policy <plat> [mode]  Show/set policy (pairing|whitelist|open)\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\n\
Environment variables:\n\
  CORTEX_TELEGRAM_TOKEN  Telegram bot token\n\
  CORTEX_WHATSAPP_TOKEN  WhatsApp access token\n\
  CORTEX_QQ_APP_ID       QQ Bot AppID\n\
  CORTEX_QQ_APP_SECRET   QQ Bot AppSecret\n\
  CORTEX_QQ_MARKDOWN     QQ markdown output (default: true)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Node,
        names: &["node"],
        summary: "Manage Node.js tools for MCP servers",
        help: Some(
            "cortex node — Node.js environment management.\n\n\
Subcommands:\n\
  setup                 Install Node.js and pnpm for MCP servers\n\
  status                Show Node.js environment status\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Browser,
        names: &["browser"],
        summary: "Manage browser integration",
        help: Some(
            "cortex browser — Browser integration management.\n\n\
Subcommands:\n\
  enable                Configure Chrome DevTools MCP server\n\
  disable               Remove Chrome DevTools MCP server configuration\n\
  status                Show browser integration status\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Permission,
        names: &["permission"],
        summary: "Show or change the permission mode",
        help: Some(
            "cortex permission — Show or change the tool confirmation mode.\n\n\
Usage: cortex permission [strict|balanced|open] [OPTIONS]\n\n\
Modes:\n\
  strict     Auto-approve only Allow\n\
  balanced   Auto-approve through Review (default)\n\
  open       Auto-approve all non-blocking tools\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\
  --system   Update the system instance config (restart required to apply)\n\n\
Without a mode, prints the current setting.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Config,
        names: &["config"],
        summary: "View or update selected config keys",
        help: Some(
            "cortex config — View or update selected instance config keys.\n\n\
Usage:\n\
  cortex config list [--id <ID>]\n\
  cortex config get <section> [--id <ID>]\n\
  cortex config set <key> <value> [--id <ID>]\n\n\
Supported writable keys:\n\
  turn.show_thinking        true enables provider thinking request/output\n\
  turn.strip_think_tags     true disables provider thinking output (default)\n\
  embedding.api_key         embedding provider API key\n\n\
Changes are written to config.toml and hot-reloaded when the user daemon is running.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Policy,
        names: &["policy"],
        summary: "Lint and simulate runtime policy",
        help: Some(
            "cortex policy — Policy-as-code checks for the current instance.\n\n\
Subcommands:\n\
  lint                         Check config and enabled plugins\n\
  simulate <tool> [OPTIONS]    Explain one tool/effect decision\n\n\
Simulation options:\n\
  --tool <NAME>                Tool name; alternative to positional <tool>\n\
  --actor <ACTOR>              Actor label for the report\n\
  --effect <KIND[:TARGET]>     Declared effect; repeatable\n\
  --background                 Simulate background execution\n\n\
Effect kinds include read_file, read_secret, write_file, delete_file,\n\
run_process, network_request, send_message, spend_money, deploy,\n\
modify_credential, persist_memory, publish_content, schedule_task,\n\
generate_media, introspect_runtime, delegate_work.\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\
  --system   Read the system instance config",
        ),
    },
];

pub(crate) const fn deploy_command_specs() -> &'static [DeployCommandSpec] {
    DEPLOY_COMMAND_SPECS
}

fn parse_deploy_subcommand(cmd: &str) -> Option<DeploySubcommand> {
    deploy_command_specs()
        .iter()
        .find(|spec| spec.names().contains(&cmd))
        .map(|spec| spec.subcommand)
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
        DeploySubcommand::Demo => crate::deploy_demo::cmd_demo(remaining_args),
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
