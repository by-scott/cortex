use std::fs;
use std::os::unix::fs::FileTypeExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const SERVICE_NAME: &str = "cortex";
const PH_CORTEX_BIN: &str = "{cortex_bin}";
const PH_CORTEX_HOME: &str = "{cortex_home}";
const PH_CORTEX_ID: &str = "{cortex_id}";

const PH_PATH: &str = "{path}";

const USER_UNIT_TEMPLATE: &str = r"[Unit]
Description=Cortex Cognitive Runtime
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
Description=Cortex Cognitive Runtime
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

fn resolve_paths(args: &[String], system: bool) -> cortex_kernel::CortexPaths {
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

fn user_unit_path_for(svc_name: &str) -> PathBuf {
    user_unit_dir().join(format!("{svc_name}.service"))
}

fn system_unit_path_for(svc_name: &str) -> PathBuf {
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

fn systemctl(args: &[&str], system: bool) -> Result<std::process::Output, String> {
    if system {
        systemctl_system(args)
    } else {
        systemctl_user(args)
    }
}

fn check_linux() -> Result<(), String> {
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
    eprintln!("  Status:    cortex status");
    Ok(())
}

fn config_path_for_instance_path(instance_path: &Path) -> PathBuf {
    cortex_kernel::CortexPaths::from_instance_home(instance_path)
        .config_files()
        .config
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

/// `cortex status [--system] [--id ID]`
///
/// # Errors
/// Returns an error string if the status cannot be queried.
pub fn cmd_status(args: &[String]) -> Result<(), String> {
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
        eprintln!("Service not installed, run `cortex install{flag}` first.");
        return Ok(());
    }

    let out = systemctl(&["status", &svc], system)?;
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

    // Show HTTP address, socket, data dir, and config info
    eprintln!("  Data:   {instance_home}");
    eprintln!("  Socket: {}", socket_path.display());

    let config_path = config_path_for_instance_path(&instance_path);
    if let Ok(content) = fs::read_to_string(&config_path) {
        let mut in_daemon = false;
        let mut in_api = false;
        let mut addr_val = String::new();
        let mut provider_val = String::new();
        let mut model_val = String::new();
        let mut preset_val = String::new();
        for line in content.lines() {
            let t = line.trim();
            if t.starts_with("[daemon]") {
                in_daemon = true;
                in_api = false;
            } else if t.starts_with("[api]") {
                in_api = true;
                in_daemon = false;
            } else if t.starts_with('[') {
                in_daemon = false;
                in_api = false;
            }
            let extract = |line: &str| -> String {
                line.split('=')
                    .nth(1)
                    .map(|v| {
                        let v = v.trim();
                        // Strip inline TOML comments (# after closing quote)
                        let v = if v.starts_with('"') {
                            // Find closing quote, ignore everything after
                            v.get(1..)
                                .and_then(|s| s.find('"').map(|i| &s[..i]))
                                .unwrap_or_else(|| v.trim_matches('"'))
                        } else {
                            v.split('#').next().unwrap_or(v).trim()
                        };
                        v.to_string()
                    })
                    .unwrap_or_default()
            };
            if in_daemon && t.starts_with("addr") {
                addr_val = extract(t);
            }
            if in_api && t.starts_with("provider") && !t.starts_with("provider_") {
                provider_val = extract(t);
            }
            if in_api && t.starts_with("model") {
                model_val = extract(t);
            }
            if in_api && t.starts_with("preset") {
                preset_val = extract(t);
            }
        }
        if !addr_val.is_empty() && !addr_val.ends_with(":0") {
            eprintln!("  HTTP:   {addr_val}  (REST / RPC / SSE / Web UI)");
        }
        if !provider_val.is_empty() {
            let model_info = if model_val.is_empty() {
                String::new()
            } else {
                format!(" / {model_val}")
            };
            let preset_info = if preset_val.is_empty() {
                String::new()
            } else {
                format!(" ({preset_val})")
            };
            eprintln!("  LLM:    {provider_val}{model_info}{preset_info}");
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
        let config_path = config_path_for_instance_path(&home_path);
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

/// `cortex plugin <sub> [args...]` — manage .cpx plugins.
///
/// # Errors
/// Returns an error string if the plugin subcommand fails.
pub fn cmd_plugin(args: &[String]) -> Result<(), String> {
    use crate::plugin_manager;

    let plugin_args: &[String] = args
        .iter()
        .position(|a| a == "plugin")
        .map_or(args, |pos| &args[pos + 1..]);

    let sub = plugin_args.first().map_or("list", String::as_str);
    let paths = resolve_paths_from_args(plugin_args);
    let cortex_home = paths.base_dir().clone();
    let home = cortex_home.as_path();
    let instance_id = parse_instance_id(plugin_args);
    let instance = instance_id.as_deref().unwrap_or("default");
    let instance_home = paths.instance_home();

    match sub {
        "install" => plugin_install(plugin_args, home, &instance_home, instance)?,
        "enable" => plugin_enable(plugin_args, home, &instance_home, instance)?,
        "disable" => plugin_disable(plugin_args, home, &instance_home, instance)?,
        "uninstall" | "remove" => {
            plugin_uninstall(plugin_args, home, &paths, &instance_home, instance)?;
        }
        "list" | "ls" => {
            let plugins = plugin_manager::list(home);
            // Read instance enabled list for status display.
            let enabled = read_enabled_plugins(&instance_home);
            if plugins.is_empty() {
                eprintln!("No plugins installed.");
            } else {
                for p in &plugins {
                    let native = if p.has_native { " [native]" } else { "" };
                    let status = if enabled.iter().any(|e| e == &p.name) {
                        " [enabled]"
                    } else {
                        ""
                    };
                    eprintln!(
                        "  {} v{}{}{} -- {}",
                        p.name, p.version, native, status, p.description
                    );
                }
            }
        }
        "pack" => {
            let dir = plugin_args
                .get(1)
                .ok_or("usage: cortex plugin pack <dir> [output.cpx]")?;
            let dir_path = Path::new(dir.as_str());
            let default_output = plugin_manager::default_cpx_name(dir_path)?;
            let output = plugin_args
                .get(2)
                .map_or(default_output.as_str(), String::as_str);
            plugin_manager::pack(dir_path, Path::new(output))?;
            eprintln!("Packed plugin: {output}");
        }
        _ => {
            return Err(format!(
                "unknown plugin command: {sub}. Use: install, enable, disable, uninstall, list, pack"
            ));
        }
    }
    Ok(())
}

fn ensure_instance_home_exists(instance_home: &Path, instance: &str) -> Result<(), String> {
    if instance_home.exists() {
        Ok(())
    } else {
        Err(format!("instance '{instance}' does not exist"))
    }
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

fn plugin_install(
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let source = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin install <owner/repo|url|path> [--id <instance>]")?;
    ensure_instance_home_exists(instance_home, instance)?;
    let name = crate::plugin_manager::install(home, source)?;
    enable_plugin_in_config(instance_home, &name)?;
    eprintln!("Installed plugin: {name} (enabled for instance '{instance}')");
    hint_restart_if_running(plugin_args);
    Ok(())
}

fn plugin_enable(
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin enable <name> [--id <instance>]")?;
    ensure_instance_home_exists(instance_home, instance)?;
    ensure_plugin_installed(home, name)?;
    enable_plugin_in_config(instance_home, name)?;
    eprintln!("Enabled plugin: {name} (for instance '{instance}')");
    hint_restart_if_running(plugin_args);
    Ok(())
}

fn plugin_disable(
    plugin_args: &[String],
    home: &Path,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin disable <name> [--id <instance>]")?;
    ensure_instance_home_exists(instance_home, instance)?;
    ensure_plugin_installed(home, name)?;
    disable_plugin_in_config(instance_home, name)?;
    eprintln!("Disabled plugin: {name} (for instance '{instance}')");
    hint_restart_if_running(plugin_args);
    Ok(())
}

fn plugin_uninstall(
    plugin_args: &[String],
    home: &Path,
    paths: &cortex_kernel::CortexPaths,
    instance_home: &Path,
    instance: &str,
) -> Result<(), String> {
    let name = plugin_args
        .get(1)
        .ok_or("usage: cortex plugin uninstall <name> [--id <instance>] [--purge]")?;
    ensure_instance_home_exists(instance_home, instance)?;
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
    hint_restart_if_running(plugin_args);
    Ok(())
}

/// Add a plugin name to `[plugins].enabled` in an instance's `config.toml`.
fn enable_plugin_in_config(instance_home: &Path, plugin_name: &str) -> Result<(), String> {
    let config_path = config_path_for_instance_home(instance_home);
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    // Check if already enabled.
    if content.contains(&format!("\"{plugin_name}\""))
        && content.contains("[plugins]")
        && content.contains("enabled")
    {
        return Ok(()); // Already enabled.
    }

    let mut enabled = read_enabled_plugins(instance_home);
    if !enabled.iter().any(|e| e == plugin_name) {
        enabled.push(plugin_name.to_string());
    }

    write_enabled_plugins(&config_path, &content, &enabled)
}

/// Remove a plugin name from `[plugins].enabled` in an instance's `config.toml`.
fn disable_plugin_in_config(instance_home: &Path, plugin_name: &str) -> Result<(), String> {
    let config_path = config_path_for_instance_home(instance_home);
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    let mut enabled = read_enabled_plugins(instance_home);
    enabled.retain(|e| e != plugin_name);

    write_enabled_plugins(&config_path, &content, &enabled)
}

/// Read the `[plugins].enabled` array from an instance's `config.toml`.
pub(crate) fn read_enabled_plugins(instance_home: &Path) -> Vec<String> {
    let config_path = config_path_for_instance_home(instance_home);
    let Ok(content) = fs::read_to_string(&config_path) else {
        return Vec::new();
    };
    let mut in_plugins = false;
    for line in content.lines() {
        let t = line.trim();
        if t == "[plugins]" {
            in_plugins = true;
            continue;
        }
        if t.starts_with('[') && t != "[plugins]" {
            in_plugins = false;
        }
        if in_plugins && let Some(val) = t.strip_prefix("enabled") {
            let val = val.trim().strip_prefix('=').unwrap_or(val).trim();
            return val
                .trim_start_matches('[')
                .trim_end_matches(']')
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
    }
    Vec::new()
}

fn config_path_for_instance_home(instance_home: &Path) -> PathBuf {
    cortex_kernel::CortexPaths::from_instance_home(instance_home)
        .config_files()
        .config
}

/// Write the `[plugins].enabled` array to config.toml using line-level replacement.
fn write_enabled_plugins(
    config_path: &Path,
    content: &str,
    enabled: &[String],
) -> Result<(), String> {
    let enabled_line = format!(
        "enabled = [{}]",
        enabled
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ")
    );

    let mut lines: Vec<String> = Vec::new();
    let mut in_plugins = false;
    let mut replaced = false;

    for line in content.lines() {
        let t = line.trim();
        if t == "[plugins]" {
            in_plugins = true;
        } else if t.starts_with('[') {
            in_plugins = false;
        }
        if in_plugins && t.starts_with("enabled") {
            lines.push(enabled_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        // Append [plugins] section if missing.
        lines.push(String::new());
        lines.push("[plugins]".to_string());
        lines.push(enabled_line);
    }

    fs::write(config_path, lines.join("\n"))
        .map_err(|e| format!("cannot write {}: {e}", config_path.display()))
}

/// If the daemon is running, tell the user to restart for changes to take effect.
fn hint_restart_if_running(args: &[String]) {
    let instance_id = parse_instance_id(args);
    let system = parse_system_flag(args);
    let paths = resolve_paths(args, system);
    let svc = service_name(paths.base_dir(), instance_id.as_deref(), system);
    let exists = if system {
        system_unit_path_for(&svc).exists()
    } else {
        user_unit_path_for(&svc).exists()
    };
    if !exists {
        return;
    }
    let Ok(out) = systemctl(&["is-active", &svc], system) else {
        return;
    };
    if String::from_utf8_lossy(&out.stdout).trim() == "active" {
        eprintln!("Run `cortex restart` to apply changes.");
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
    Ps,
    Reset,
    Plugin,
    Channel,
    Actor,
    Node,
    Browser,
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
  --system        Install as system-level service (requires root)\n\n\
Environment variables (first install only):\n\
  CORTEX_API_KEY              LLM API key\n\
  CORTEX_PROVIDER             LLM provider (e.g. zai, anthropic, openai)\n\
  CORTEX_MODEL                LLM model name\n\
  CORTEX_BASE_URL             Custom provider base URL\n\
  CORTEX_LLM_PRESET           Preset (minimal, standard, cognitive, full)\n\
  CORTEX_EMBEDDING_PROVIDER   Embedding provider (e.g. ollama)\n\
  CORTEX_EMBEDDING_MODEL      Embedding model name\n\
  CORTEX_EMBEDDING_BASE_URL   Embedding provider base URL\n\
  CORTEX_BRAVE_KEY            Brave Search API key\n\n\
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
          current LLM provider/model/preset.",
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
  enable <name>       Enable an installed plugin for one instance\n\
  disable <name>      Disable an installed plugin for one instance\n\
  uninstall <name>    Disable for one instance; add --purge to remove files\n\
  list                List installed plugins with status\n\
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
        DeploySubcommand::Status => cmd_status(remaining_args),
        DeploySubcommand::Ps => cmd_ps(None),
        DeploySubcommand::Reset => cmd_reset(remaining_args),
        DeploySubcommand::Plugin => cmd_plugin(remaining_args),
        DeploySubcommand::Channel => {
            cmd_channel(remaining_args);
            Ok(())
        }
        DeploySubcommand::Actor => {
            cmd_actor(remaining_args);
            Ok(())
        }
        DeploySubcommand::Node => cmd_node(remaining_args),
        DeploySubcommand::Browser => cmd_browser(remaining_args),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NestedSubcommandInvocation<'a> {
    subcommand: Option<&'a str>,
    remaining: &'a [String],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NestedCommandSpec<T> {
    subcommand: T,
    names: &'static [&'static str],
    summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetailUsageSpec {
    usage: &'static str,
    summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelSubcommand {
    Telegram,
    Whatsapp,
    Qq,
    Pair,
    Subscribe,
    Unsubscribe,
    Approve,
    Allow,
    Deny,
    Unallow,
    Undeny,
    Revoke,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorSubcommand {
    Alias,
    Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingAction {
    List,
    Set,
    Unset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeSubcommand {
    Setup,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserSubcommand {
    Enable,
    Disable,
    Status,
}

const CHANNEL_SUBCOMMAND_SPECS: &[NestedCommandSpec<ChannelSubcommand>] = &[
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Telegram,
        names: &["telegram"],
        summary: "Show Telegram configuration info",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Whatsapp,
        names: &["whatsapp"],
        summary: "Show WhatsApp configuration info",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Qq,
        names: &["qq"],
        summary: "Show QQ configuration info",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Pair,
        names: &["pair"],
        summary: "Show pending/paired users",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Subscribe,
        names: &["subscribe"],
        summary: "Enable session subscription for a paired user",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Unsubscribe,
        names: &["unsubscribe"],
        summary: "Disable session subscription for a paired user",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Approve,
        names: &["approve"],
        summary: "Approve a user (platform: telegram|whatsapp|qq)",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Allow,
        names: &["allow"],
        summary: "Add user to whitelist",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Deny,
        names: &["deny"],
        summary: "Add user to blacklist",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Unallow,
        names: &["unallow"],
        summary: "Remove user from whitelist",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Undeny,
        names: &["undeny"],
        summary: "Remove user from blacklist",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Revoke,
        names: &["revoke"],
        summary: "Remove a paired user",
    },
    NestedCommandSpec {
        subcommand: ChannelSubcommand::Policy,
        names: &["policy"],
        summary: "Show or set policy (pairing|whitelist|open)",
    },
];

const ACTOR_SUBCOMMAND_SPECS: &[NestedCommandSpec<ActorSubcommand>] = &[
    NestedCommandSpec {
        subcommand: ActorSubcommand::Alias,
        names: &["alias"],
        summary: "List or change actor aliases",
    },
    NestedCommandSpec {
        subcommand: ActorSubcommand::Transport,
        names: &["transport"],
        summary: "List or change transport actor bindings",
    },
];

const ACTOR_DETAIL_SPECS: &[DetailUsageSpec] = &[
    DetailUsageSpec {
        usage: "alias list",
        summary: "List actor aliases",
    },
    DetailUsageSpec {
        usage: "alias set <from> <to>",
        summary: "Map one actor to a canonical actor",
    },
    DetailUsageSpec {
        usage: "alias unset <from>",
        summary: "Remove an actor alias",
    },
    DetailUsageSpec {
        usage: "transport list",
        summary: "List transport actor bindings",
    },
    DetailUsageSpec {
        usage: "transport set <name|all> <actor>",
        summary: "Bind transport(s) to actor",
    },
    DetailUsageSpec {
        usage: "transport unset <name>",
        summary: "Remove transport binding",
    },
];

const CHANNEL_DETAIL_SPECS: &[DetailUsageSpec] = &[
    DetailUsageSpec {
        usage: "pair [platform]",
        summary: "Show pair state",
    },
    DetailUsageSpec {
        usage: "subscribe <platform> <user_id>",
        summary: "Enable session broadcasts for a paired user",
    },
    DetailUsageSpec {
        usage: "unsubscribe <platform> <user_id>",
        summary: "Disable session broadcasts for a paired user",
    },
    DetailUsageSpec {
        usage: "approve <platform> <user_id> [--subscribe|--no-subscribe]",
        summary: "Approve a pending user and optionally change subscription",
    },
    DetailUsageSpec {
        usage: "revoke <platform> <user_id>",
        summary: "Revoke a paired user immediately",
    },
    DetailUsageSpec {
        usage: "allow <platform> <user_id>",
        summary: "Add a user to the whitelist",
    },
    DetailUsageSpec {
        usage: "deny <platform> <user_id>",
        summary: "Add a user to the blacklist",
    },
    DetailUsageSpec {
        usage: "unallow <platform> <user_id>",
        summary: "Remove a user from the whitelist",
    },
    DetailUsageSpec {
        usage: "undeny <platform> <user_id>",
        summary: "Remove a user from the blacklist",
    },
    DetailUsageSpec {
        usage: "policy <platform> [mode]",
        summary: "Show or set policy mode",
    },
];

const BINDING_ACTION_SPECS: &[NestedCommandSpec<BindingAction>] = &[
    NestedCommandSpec {
        subcommand: BindingAction::List,
        names: &["list"],
        summary: "List current bindings",
    },
    NestedCommandSpec {
        subcommand: BindingAction::Set,
        names: &["set"],
        summary: "Create or update a binding",
    },
    NestedCommandSpec {
        subcommand: BindingAction::Unset,
        names: &["unset"],
        summary: "Remove a binding",
    },
];

const NODE_SUBCOMMAND_SPECS: &[NestedCommandSpec<NodeSubcommand>] = &[
    NestedCommandSpec {
        subcommand: NodeSubcommand::Setup,
        names: &["setup"],
        summary: "Install Node.js and pnpm for MCP servers",
    },
    NestedCommandSpec {
        subcommand: NodeSubcommand::Status,
        names: &["status"],
        summary: "Show Node.js environment status",
    },
];

const BROWSER_SUBCOMMAND_SPECS: &[NestedCommandSpec<BrowserSubcommand>] = &[
    NestedCommandSpec {
        subcommand: BrowserSubcommand::Enable,
        names: &["enable"],
        summary: "Configure Chrome DevTools MCP server",
    },
    NestedCommandSpec {
        subcommand: BrowserSubcommand::Disable,
        names: &["disable"],
        summary: "Remove Chrome DevTools MCP server configuration",
    },
    NestedCommandSpec {
        subcommand: BrowserSubcommand::Status,
        names: &["status"],
        summary: "Show browser integration status",
    },
];

fn parse_nested_subcommand<'a>(args: &'a [String], root: &str) -> NestedSubcommandInvocation<'a> {
    let root_pos = args.iter().position(|arg| arg == root);
    let after_root = root_pos.map_or(args, |pos| &args[pos + 1..]);

    let mut index = 0;
    while index < after_root.len() {
        let arg = after_root[index].as_str();
        if arg == "--id" {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return NestedSubcommandInvocation {
            subcommand: Some(arg),
            remaining: &after_root[index + 1..],
        };
    }

    NestedSubcommandInvocation {
        subcommand: None,
        remaining: &[],
    }
}

fn parse_channel_subcommand(subcommand: Option<&str>) -> Option<ChannelSubcommand> {
    let subcommand = subcommand?;
    CHANNEL_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
}

fn parse_actor_subcommand(subcommand: Option<&str>) -> Option<ActorSubcommand> {
    let subcommand = subcommand?;
    ACTOR_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
}

fn parse_binding_action(action: Option<&str>) -> Option<BindingAction> {
    let action = action?;
    BINDING_ACTION_SPECS
        .iter()
        .find(|spec| spec.names.contains(&action))
        .map(|spec| spec.subcommand)
}

fn unknown_nested_subcommand_error<T>(
    root: &str,
    subcommand: &str,
    specs: &[NestedCommandSpec<T>],
) -> String
where
    T: Copy,
{
    let choices = specs
        .iter()
        .map(|spec| spec.names[0])
        .collect::<Vec<_>>()
        .join(", ");
    format!("unknown {root} command: {subcommand}. Use: {choices}")
}

fn parse_node_subcommand(subcommand: Option<&str>) -> Result<NodeSubcommand, String> {
    let Some(subcommand) = subcommand else {
        return Ok(NodeSubcommand::Status);
    };
    NODE_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
        .ok_or_else(|| unknown_nested_subcommand_error("node", subcommand, NODE_SUBCOMMAND_SPECS))
}

fn parse_browser_subcommand(subcommand: Option<&str>) -> Result<BrowserSubcommand, String> {
    let Some(subcommand) = subcommand else {
        return Ok(BrowserSubcommand::Status);
    };
    BROWSER_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
        .ok_or_else(|| {
            unknown_nested_subcommand_error("browser", subcommand, BROWSER_SUBCOMMAND_SPECS)
        })
}

// ── Channel subcommand ──────────────────────────────────────

/// `cortex channel <telegram|whatsapp|qq|pair> [options]`
///
/// Channels now run inside the daemon. This subcommand provides configuration
/// info and pairing management (file-based, no daemon connection needed).
///
/// # Errors
/// Returns an error string if the channel subcommand fails.
fn cmd_channel(args: &[String]) {
    let paths = resolve_paths_from_args(args);
    let instance_home = paths.instance_home();
    let invocation = parse_nested_subcommand(args, "channel");
    let remaining = invocation.remaining;

    match parse_channel_subcommand(invocation.subcommand) {
        Some(ChannelSubcommand::Telegram) => cmd_channel_telegram(&instance_home),
        Some(ChannelSubcommand::Whatsapp) => cmd_channel_whatsapp(&instance_home),
        Some(ChannelSubcommand::Qq) => cmd_channel_qq(&instance_home),
        Some(ChannelSubcommand::Pair) => cmd_channel_pair(remaining, &instance_home),
        Some(ChannelSubcommand::Subscribe) => {
            cmd_channel_subscription(remaining, &instance_home, true);
        }
        Some(ChannelSubcommand::Unsubscribe) => {
            cmd_channel_subscription(remaining, &instance_home, false);
        }
        Some(ChannelSubcommand::Approve) => cmd_channel_approve(remaining, &instance_home),
        Some(ChannelSubcommand::Allow) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Whitelist, true);
        }
        Some(ChannelSubcommand::Deny) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Blacklist, true);
        }
        Some(ChannelSubcommand::Unallow) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Whitelist, false);
        }
        Some(ChannelSubcommand::Undeny) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Blacklist, false);
        }
        Some(ChannelSubcommand::Revoke) => cmd_channel_revoke(remaining, &instance_home),
        Some(ChannelSubcommand::Policy) => cmd_channel_policy(remaining, &instance_home),
        None => print_channel_usage(),
    }
}

fn cmd_actor(args: &[String]) {
    let paths = resolve_paths_from_args(args);
    let store = cortex_kernel::ActorBindingsStore::from_paths(&paths);
    let invocation = parse_nested_subcommand(args, "actor");
    let remaining = invocation.remaining;

    match parse_actor_subcommand(invocation.subcommand) {
        Some(ActorSubcommand::Alias) => cmd_actor_alias(remaining, &store),
        Some(ActorSubcommand::Transport) => cmd_actor_transport(remaining, &store),
        None => print_actor_usage(),
    }
}

fn print_actor_usage() {
    eprintln!("Usage: cortex actor <subcommand>");
    eprintln!();
    eprintln!("Identity mapping for unified session ownership.");
    for spec in ACTOR_SUBCOMMAND_SPECS {
        eprintln!("  {:<28} {}", spec.names[0], spec.summary);
    }
    for spec in ACTOR_DETAIL_SPECS {
        eprintln!("  {:<28} {}", spec.usage, spec.summary);
    }
}

fn print_channel_usage() {
    eprintln!("Usage: cortex channel <subcommand>");
    eprintln!();
    eprintln!("Channels run inside the daemon automatically.");
    for spec in CHANNEL_SUBCOMMAND_SPECS {
        eprintln!("  {:<28} {}", spec.names[0], spec.summary);
    }
    for spec in CHANNEL_DETAIL_SPECS {
        eprintln!("  {:<28} {}", spec.usage, spec.summary);
    }
}

fn print_usage_line(usage: &str) {
    eprintln!("Usage: {usage}");
}

fn actor_action_usage(scope: &str, required_args: &[&str]) -> String {
    let suffix = if required_args.is_empty() {
        String::new()
    } else {
        format!(" {}", required_args.join(" "))
    };
    format!("cortex actor {scope}{suffix}")
}

fn channel_action_usage(scope: &str, required_args: &[&str]) -> String {
    let suffix = if required_args.is_empty() {
        String::new()
    } else {
        format!(" {}", required_args.join(" "))
    };
    format!("cortex channel {scope}{suffix}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyListKind {
    Whitelist,
    Blacklist,
}

impl PolicyListKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
        }
    }

    const fn store_list(self) -> cortex_runtime::channels::store::PolicyList {
        match self {
            Self::Whitelist => cortex_runtime::channels::store::PolicyList::Whitelist,
            Self::Blacklist => cortex_runtime::channels::store::PolicyList::Blacklist,
        }
    }
}

fn cmd_actor_alias(args: &[String], store: &cortex_kernel::ActorBindingsStore) {
    let Some(action) = parse_binding_action(args.first().map(String::as_str)) else {
        print_actor_usage();
        return;
    };
    match action {
        BindingAction::List => list_bindings(store.actor_aliases(), "Actor aliases"),
        BindingAction::Set => {
            if args.len() < 3 {
                print_usage_line(&actor_action_usage("alias set", &["<from>", "<to>"]));
                return;
            }
            store.set_actor_alias(&args[1], &args[2]);
            eprintln!("Actor alias set: {} -> {}", args[1], args[2]);
        }
        BindingAction::Unset => {
            if args.len() < 2 {
                print_usage_line(&actor_action_usage("alias unset", &["<from>"]));
                return;
            }
            if store.remove_actor_alias(&args[1]) {
                eprintln!("Actor alias removed: {}", args[1]);
            } else {
                eprintln!("Actor alias not found: {}", args[1]);
            }
        }
    }
}

fn cmd_actor_transport(args: &[String], store: &cortex_kernel::ActorBindingsStore) {
    let Some(action) = parse_binding_action(args.first().map(String::as_str)) else {
        print_actor_usage();
        return;
    };
    match action {
        BindingAction::List => list_bindings(store.transport_actors(), "Transport actor bindings"),
        BindingAction::Set => {
            if args.len() < 3 {
                print_usage_line(&actor_action_usage(
                    "transport set",
                    &["<name|all>", "<actor>"],
                ));
                return;
            }
            let name = &args[1];
            let actor = &args[2];
            if name == "all" || name == "*" {
                for transport in &["http", "rpc", "ws", "sock", "stdio"] {
                    store.set_transport_actor(transport, actor);
                }
                eprintln!("All transports bound to {actor}");
            } else {
                store.set_transport_actor(name, actor);
                eprintln!("Transport binding set: {name} -> {actor}");
            }
        }
        BindingAction::Unset => {
            if args.len() < 2 {
                print_usage_line(&actor_action_usage("transport unset", &["<name>"]));
                return;
            }
            if store.remove_transport_actor(&args[1]) {
                eprintln!("Transport binding removed: {}", args[1]);
            } else {
                eprintln!("Transport binding not found: {}", args[1]);
            }
        }
    }
}

fn list_bindings(map: std::collections::BTreeMap<String, String>, label: &str) {
    eprintln!("{label}:");
    if map.is_empty() {
        eprintln!("  (empty)");
        return;
    }
    for (key, value) in map {
        eprintln!("  {key} -> {value}");
    }
}

fn cmd_channel_telegram(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "telegram").auth;
    let has_token = auth_path.exists();

    eprintln!("Telegram channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (token present)");
        eprintln!("  The daemon will start Telegram polling/webhook automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_TELEGRAM_TOKEN=<token> and reinstall:");
        eprintln!("       CORTEX_TELEGRAM_TOKEN=123:ABC cortex install");
        eprintln!("    2. Or create channels/telegram/auth.json with {{\"bot_token\": \"...\"}}");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_whatsapp(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "whatsapp").auth;
    let has_token = auth_path.exists();

    eprintln!("WhatsApp channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (token present)");
        eprintln!("  The daemon will start WhatsApp webhook automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_WHATSAPP_TOKEN=<token> and reinstall:");
        eprintln!("       CORTEX_WHATSAPP_TOKEN=EAA... cortex install");
        eprintln!("    2. Or create channels/whatsapp/auth.json with credentials");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_qq(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "qq").auth;
    let has_token = auth_path.exists();

    eprintln!("QQ channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (AppID/AppSecret present)");
        eprintln!("  The daemon will start QQ Bot WebSocket automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_QQ_APP_ID / CORTEX_QQ_APP_SECRET and reinstall:");
        eprintln!("       CORTEX_QQ_APP_ID=123 CORTEX_QQ_APP_SECRET=xyz cortex install");
        eprintln!("    2. Or create channels/qq/auth.json with QQ credentials");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_pair(args: &[String], home: &Path) {
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let options = parse_channel_pair_options(args);
    let platforms: Vec<&str> = options
        .platform
        .as_deref()
        .map_or_else(|| vec!["telegram", "whatsapp", "qq"], |p| vec![p]);

    for p in platforms {
        let store = cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(p));
        eprintln!("=== {p} ===");
        let paired = store.paired_users();
        let pending = store.pending_pairs();

        if pending.is_empty() {
            eprintln!("  No pending pair requests.");
        } else {
            eprintln!("  Pending ({}):", pending.len());
            for pp in &pending {
                eprintln!(
                    "    User: {} ({}) -- Code: {} -- {}",
                    pp.user_id, pp.user_name, pp.code, pp.created_at
                );
            }
        }
        eprintln!("  Paired ({}):", paired.len());
        for pu in &paired {
            eprintln!(
                "    {} ({}) -- since {} -- subscription: {}",
                pu.user_id,
                pu.name,
                format_paired_at(&pu.paired_at),
                if pu.subscribe { "enabled" } else { "disabled" }
            );
        }
    }
}

struct ChannelPairOptions {
    platform: Option<String>,
}

fn parse_channel_pair_options(args: &[String]) -> ChannelPairOptions {
    let mut platform = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--id" => {
                let _ = iter.next();
            }
            other if other.starts_with("--") => {}
            other => {
                if platform.is_none() {
                    platform = Some(other.to_string());
                }
            }
        }
    }
    ChannelPairOptions { platform }
}

fn cmd_channel_subscription(args: &[String], home: &Path, subscribe: bool) {
    if args.len() < 2 {
        let scope = if subscribe {
            "subscribe"
        } else {
            "unsubscribe"
        };
        print_usage_line(&channel_action_usage(scope, &["<platform>", "<user_id>"]));
        eprintln!("  platform: telegram|whatsapp|qq");
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));
    match store.set_pair_subscription(user_id, subscribe) {
        Ok(user) => eprintln!(
            "Channel subscription {} for {platform} user {} ({}). Restart the daemon to apply if it is already running.",
            if subscribe { "enabled" } else { "disabled" },
            user.user_id,
            user.name
        ),
        Err(cortex_runtime::channels::store::ChannelStoreError::PairedUserNotFound(_)) => {
            eprintln!("Paired user {user_id} not found on {platform}.");
        }
        Err(err) => eprintln!("Failed to update subscription for {user_id} on {platform}: {err}"),
    }
}

/// Convert a `paired_at` value like `"1776434889s"` to a human-readable UTC string.
fn format_paired_at(raw: &str) -> String {
    let secs_str = raw.trim_end_matches('s');
    let Ok(secs) = secs_str.parse::<u64>() else {
        return raw.to_string();
    };
    let ts = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    let dt: chrono::DateTime<chrono::Local> = ts.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn cmd_channel_approve(args: &[String], home: &Path) {
    if args.len() < 2 {
        print_usage_line(&channel_action_usage(
            "approve",
            &["<platform>", "<user_id>", "[--subscribe|--no-subscribe]"],
        ));
        eprintln!("  platform: telegram|whatsapp|qq");
        eprintln!("  user_id:  the user's platform ID (shown in 'cortex channel pair')");
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let subscribe = parse_subscription_flag(&args[2..]);
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let dir = paths.channel_dir(platform);
    let store = cortex_runtime::channels::store::ChannelStore::open_dir(dir.clone());

    if !dir.exists() {
        eprintln!("No channel directory for '{platform}'. Is the channel configured?");
        return;
    }
    match store.approve_pending_pair(user_id) {
        Ok(user) => {
            eprintln!("Approved: {} ({}) on {platform}.", user.user_id, user.name);
            eprintln!("The user can now chat. (Takes effect immediately, no restart needed.)");
            if let Some(enabled) = subscribe {
                match store.set_pair_subscription(user_id, enabled) {
                    Ok(updated) => eprintln!(
                        "Channel subscription {} for {platform} user {} ({}). Restart the daemon to apply if it is already running.",
                        if enabled { "enabled" } else { "disabled" },
                        updated.user_id,
                        updated.name
                    ),
                    Err(err) => eprintln!(
                        "Approved user, but failed to update subscription for {user_id} on {platform}: {err}"
                    ),
                }
            }
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::AlreadyPaired(_)) => {
            eprintln!("User {user_id} is already paired on {platform}.");
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PendingUserNotFound(_)) => {
            eprintln!("Pending pair request not found for {user_id} on {platform}.");
        }
        Err(err) => eprintln!("Failed to approve {user_id} on {platform}: {err}"),
    }
}

fn parse_subscription_flag(args: &[String]) -> Option<bool> {
    args.iter().find_map(|arg| match arg.as_str() {
        "--subscribe" => Some(true),
        "--no-subscribe" => Some(false),
        _ => None,
    })
}

fn cmd_channel_revoke(args: &[String], home: &Path) {
    if args.len() < 2 {
        print_usage_line(&channel_action_usage(
            "revoke",
            &["<platform>", "<user_id>"],
        ));
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));
    if !store.revoke_pair(user_id) {
        eprintln!("User {user_id} not found in paired users on {platform}.");
        return;
    }
    eprintln!("Revoked: {user_id} on {platform}. Takes effect immediately.");
}

/// Add or remove a user from a policy list (whitelist or blacklist).
fn cmd_channel_list_op(args: &[String], home: &Path, list: PolicyListKind, add: bool) {
    if args.len() < 2 {
        let command = if add {
            format!("allow-{}", list.as_str())
        } else {
            format!("deny-{}", list.as_str())
        };
        print_usage_line(&channel_action_usage(
            &command,
            &["<platform>", "<user_id>"],
        ));
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));

    match store.mutate_policy_list(list.store_list(), user_id, add) {
        Ok(_) => {
            let action = if add { "Added" } else { "Removed" };
            eprintln!("{action} {user_id} {} on {platform}.", list.as_str());
            eprintln!("Takes effect immediately, no restart needed.");
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PolicyEntryExists { .. }) => {
            eprintln!("{user_id} already in {} on {platform}.", list.as_str());
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PolicyEntryMissing { .. }) => {
            eprintln!("{user_id} not found in {} on {platform}.", list.as_str());
        }
        Err(err) => eprintln!("Failed to update {} on {platform}: {err}", list.as_str()),
    }
}

/// Show or set the channel access policy mode.
fn cmd_channel_policy(args: &[String], home: &Path) {
    if args.is_empty() {
        print_usage_line(&channel_action_usage("policy", &["<platform>", "[mode]"]));
        eprintln!("  Modes: pairing (default), whitelist, open");
        return;
    }
    let platform = args[0].as_str();
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));

    if let Some(new_mode) = args.get(1) {
        match store.update_policy_mode(new_mode) {
            Ok(_) => {
                eprintln!("Policy for {platform} set to '{new_mode}'. Takes effect immediately.");
            }
            Err(cortex_runtime::channels::store::ChannelStoreError::InvalidPolicyMode(_)) => {
                eprintln!("Invalid mode '{new_mode}'. Use: pairing, whitelist, open");
            }
            Err(err) => eprintln!("Failed to update policy for {platform}: {err}"),
        }
    } else {
        let policy = store.policy();
        let wl = policy.whitelist.len();
        let bl = policy.blacklist.len();
        eprintln!("{platform} policy:");
        eprintln!("  mode: {}", policy.mode);
        eprintln!("  whitelist: {wl} user(s)");
        eprintln!("  blacklist: {bl} user(s)");
        if wl > 0 {
            for user in &policy.whitelist {
                eprintln!("    + {user}");
            }
        }
        if bl > 0 {
            for user in &policy.blacklist {
                eprintln!("    - {user}");
            }
        }
    }
}

// ── Node.js management ────────────────────────────────────

fn cmd_node(args: &[String]) -> Result<(), String> {
    let paths = resolve_paths_from_args(args);
    let data_dir = paths.data_dir();

    match parse_node_subcommand(parse_nested_subcommand(args, "node").subcommand)? {
        NodeSubcommand::Setup => crate::node_manager::cmd_node_setup(&data_dir),
        NodeSubcommand::Status => {
            crate::node_manager::cmd_node_status(&data_dir);
            Ok(())
        }
    }
}

// ── Browser management ────────────────────────────────────

fn cmd_browser(args: &[String]) -> Result<(), String> {
    let paths = resolve_paths_from_args(args);
    let home = paths.instance_home();
    let data_dir = paths.data_dir();

    match parse_browser_subcommand(parse_nested_subcommand(args, "browser").subcommand)? {
        BrowserSubcommand::Enable => crate::node_manager::cmd_browser_enable(&home, &data_dir),
        BrowserSubcommand::Disable => crate::node_manager::cmd_browser_disable(&home),
        BrowserSubcommand::Status => {
            crate::node_manager::cmd_browser_status(&home);
            Ok(())
        }
    }
}
