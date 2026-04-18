use std::fs;
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

/// Resolve the systemd service name for a given instance.
fn service_name(instance_id: Option<&str>) -> String {
    match instance_id {
        Some(id) if id != "default" => format!("{SERVICE_NAME}@{id}"),
        _ => SERVICE_NAME.to_string(),
    }
}

/// Resolve instance home: `{base}/{id}` (default id = "default").
fn resolve_instance_home(instance_id: Option<&str>) -> String {
    let base = resolve_cortex_home();
    let id = instance_id.unwrap_or("default");
    PathBuf::from(&base).join(id).to_string_lossy().to_string()
}

/// Check if the base directory has any remaining instance directories.
/// If none remain, remove the base directory itself.
fn cleanup_base_if_empty(base: &Path) {
    let Ok(entries) = fs::read_dir(base) else {
        return;
    };
    let has_instance = entries.flatten().any(|e| {
        e.file_type().is_ok_and(|ft| ft.is_dir()) && e.file_name() != "." && e.file_name() != ".."
    });
    if !has_instance {
        let _ = fs::remove_dir_all(base);
        eprintln!("Removed empty base directory: {}", base.display());
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

fn user_unit_path() -> PathBuf {
    user_unit_path_for(SERVICE_NAME)
}

fn system_unit_path() -> PathBuf {
    PathBuf::from("/etc/systemd/system").join(format!("{SERVICE_NAME}.service"))
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

const SYSTEM_CORTEX_HOME: &str = "/var/lib/cortex";

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
        Err("deploy commands require Linux (systemd).".to_string())
    }
}

fn unit_exists(instance_id: Option<&str>, system: bool) -> bool {
    let svc = service_name(instance_id);
    if system {
        system_unit_path().exists()
    } else {
        user_unit_path_for(&svc).exists()
    }
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
    let base = resolve_cortex_home();
    let svc = service_name(instance_id.as_deref());

    if id != "default" {
        let mgr = cortex_runtime::InstanceManager::new(&PathBuf::from(&base));
        mgr.ensure_instance(id)
            .map_err(|e| format!("failed to create instance directory: {e}"))?;
    }

    // Pre-generate config.toml from env vars (before daemon starts).
    // The daemon process won't inherit the caller's env vars via systemd,
    // so we must generate config here while env vars are available.
    let instance_home = PathBuf::from(&base).join(id);
    cortex_kernel::ensure_home_dirs(&instance_home)
        .map_err(|e| format!("failed to create instance dirs: {e}"))?;

    // Ensure global plugins directory exists.
    let plugins_dir = PathBuf::from(&base).join("plugins");
    let _ = fs::create_dir_all(&plugins_dir);

    let config_path = instance_home.join("config.toml");
    let has_env_config = std::env::var("CORTEX_API_KEY").is_ok()
        || std::env::var("CORTEX_PROVIDER").is_ok()
        || std::env::var("CORTEX_MODEL").is_ok()
        || std::env::var("CORTEX_TELEGRAM_TOKEN").is_ok()
        || std::env::var("CORTEX_WHATSAPP_TOKEN").is_ok();
    if !config_path.exists() || has_env_config {
        // Regenerate config when env vars are provided (even if config exists)
        // to ensure install always applies the caller's configuration.
        if config_path.exists() && has_env_config {
            let _ = fs::remove_file(&config_path);
        }
        let base_path = PathBuf::from(&base);
        cortex_kernel::ensure_base_dirs(&base_path).map_err(|e| format!("ensure base: {e}"))?;
        let (providers, resolved) = cortex_kernel::load_providers(&base_path).unwrap_or_default();
        let _ = cortex_kernel::load_config(&instance_home, resolved.as_deref(), &providers);
    }

    // CORTEX_HOME = base path (e.g. ~/.cortex), --id selects instance.
    let unit_content = generate_unit_file(cortex_bin, &base, id);
    let unit_dir = user_unit_dir();
    fs::create_dir_all(&unit_dir).map_err(|e| format!("failed to create systemd user dir: {e}"))?;
    let upath = user_unit_path_for(&svc);

    if upath.exists() {
        let _ = systemctl(&["stop", &svc], false);
        eprintln!("Stopped existing service, redeploying...");
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

    let user = std::env::var("USER").unwrap_or_default();
    if !user.is_empty() {
        let _ = Command::new("loginctl")
            .args(["enable-linger", &user])
            .output();
    }

    eprintln!("Deployed successfully!");
    eprintln!("  Service:   {svc}");
    eprintln!("  Unit file: {}", upath.display());
    eprintln!("  Binary:    {cortex_bin}");
    eprintln!("  Data dir:  {}", PathBuf::from(&base).join(id).display());
    eprintln!("  Status:    cortex status");
    Ok(())
}

/// `cortex deploy [--user|--system] [--id ID]`
///
/// # Errors
/// Returns an error string if the deployment fails.
pub fn cmd_deploy(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let cortex_bin = resolve_cortex_bin();

    if system {
        let cortex_home = SYSTEM_CORTEX_HOME.to_string();
        let id = parse_instance_id(args);
        let id = id.as_deref().unwrap_or("default");
        let unit_content = generate_system_unit_file(&cortex_bin, &cortex_home, id);
        let upath = system_unit_path();

        if unit_exists(Some(id), true) {
            let _ = systemctl(&["stop", SERVICE_NAME], true);
            eprintln!("Stopped existing service, redeploying...");
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
        let enable = systemctl(&["enable", SERVICE_NAME], true)?;
        if !enable.status.success() {
            return Err(format!(
                "enable failed: {}",
                String::from_utf8_lossy(&enable.stderr)
            ));
        }
        let start = systemctl(&["start", SERVICE_NAME], true)?;
        if !start.status.success() {
            return Err(format!(
                "start failed: {}",
                String::from_utf8_lossy(&start.stderr)
            ));
        }

        eprintln!("System-level deploy successful!");
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

/// `cortex undeploy [--purge] [--system]`
///
/// # Errors
/// Returns an error string if the removal fails.
pub fn cmd_undeploy(args: &[String]) -> Result<(), String> {
    check_linux()?;
    let system = parse_system_flag(args);
    let purge = args.iter().any(|a| a == "--purge");
    let instance_id = parse_instance_id(args);
    let svc = service_name(instance_id.as_deref());

    // Stop and disable the correct service (instance-specific).
    let status = systemctl(&["is-enabled", &svc], system);
    if status.is_ok_and(|s| s.status.success()) {
        let _ = systemctl(&["stop", &svc], system);
        let _ = systemctl(&["disable", &svc], system);
        // Remove the unit file for non-default instances.
        if instance_id.as_deref().is_some_and(|id| id != "default") {
            let _ = fs::remove_file(user_unit_path_for(&svc));
        } else if system {
            let _ = Command::new("sudo")
                .args(["rm", "-f", &system_unit_path().to_string_lossy()])
                .output();
        } else {
            let _ = fs::remove_file(user_unit_path());
        }
        let _ = systemctl(&["daemon-reload"], system);
        eprintln!("Service stopped and removed.");
    } else {
        eprintln!("Service not deployed.");
    }

    // Without --purge, only remove socket file — all data and config preserved.
    // `cortex ps` uses socket presence to detect running instances.
    if !purge {
        let instance_home = resolve_instance_home(instance_id.as_deref());
        let home_path = PathBuf::from(&instance_home);
        let _ = fs::remove_file(home_path.join("data/cortex.sock"));
    }

    if purge {
        let instance_home = resolve_instance_home(instance_id.as_deref());
        let base_dir = if system {
            SYSTEM_CORTEX_HOME.to_string()
        } else {
            resolve_cortex_home()
        };
        let home_path = PathBuf::from(&instance_home);
        if home_path.exists() {
            // Remove socket first (fs::remove_dir_all may fail on Unix sockets).
            let socket = home_path.join("data/cortex.sock");
            let _ = fs::remove_file(&socket);
            if system {
                let _ = Command::new("sudo")
                    .args(["rm", "-rf", &instance_home])
                    .output();
            } else {
                fs::remove_dir_all(&home_path)
                    .map_err(|e| format!("failed to clean instance dir: {e}"))?;
            }
            // Remove base if no instances remain
            cleanup_base_if_empty(&PathBuf::from(&base_dir));
            eprintln!("Cleaned instance: {instance_home}");
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
    let svc = service_name(instance_id.as_deref());

    if !unit_exists(instance_id.as_deref(), system) {
        let flag = if system { " --system" } else { "" };
        return Err(format!(
            "service not deployed, run `cortex deploy{flag}` first."
        ));
    }

    let out = systemctl(&["start", &svc], system)?;
    if out.status.success() {
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
    let svc = service_name(instance_id.as_deref());

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
    let svc = service_name(instance_id.as_deref());

    if !unit_exists(instance_id.as_deref(), system) {
        let flag = if system { " --system" } else { "" };
        eprintln!("Service not deployed, run `cortex deploy{flag}` first.");
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
    let instance_home = resolve_instance_home(instance_id.as_deref());
    let instance_path = PathBuf::from(&instance_home);
    let socket_path = instance_path.join("data/cortex.sock");

    eprintln!("Cortex {mode} service status ({svc}):");
    eprintln!("  {}", active_line.trim());
    if let Some(pid) = pid_line {
        eprintln!("  {}", pid.trim());
    }

    // Show HTTP address, socket, data dir, and config info
    eprintln!("  Data:   {instance_home}");
    eprintln!("  Socket: {}", socket_path.display());

    let config_path = instance_path.join("config.toml");
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
    let svc = service_name(instance_id.as_deref());

    if !unit_exists(instance_id.as_deref(), system) {
        let flag = if system { " --system" } else { "" };
        return Err(format!(
            "service not deployed, run `cortex deploy{flag}` first."
        ));
    }

    let out = systemctl(&["restart", &svc], system)?;
    if out.status.success() {
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
        let svc = service_name(Some(inst.id.as_str()).filter(|id| *id != "default"));
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
    let home_path = PathBuf::from(resolve_instance_home(Some(id)));
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
        let svc = service_name(instance_id.as_deref());
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
        let config_backup = fs::read(home_path.join("config.toml")).ok();
        fs::remove_dir_all(&home_path)
            .map_err(|e| format!("failed to delete {}: {e}", home_path.display()))?;
        cortex_kernel::ensure_home_dirs(&home_path)
            .map_err(|e| format!("failed to recreate: {e}"))?;
        if let Some(config_data) = config_backup {
            fs::write(home_path.join("config.toml"), config_data)
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
    let cortex_home = resolve_cortex_home();
    let home = Path::new(&cortex_home);
    let instance_id = parse_instance_id(plugin_args);
    let instance = instance_id.as_deref().unwrap_or("default");
    let instance_home = resolve_instance_home(instance_id.as_deref());

    match sub {
        "install" => {
            let source = plugin_args
                .get(1)
                .ok_or("usage: cortex plugin install <owner/repo|url|path> [--id <instance>]")?;
            // Validate instance exists.
            if !Path::new(&instance_home).exists() {
                return Err(format!("instance '{instance}' does not exist"));
            }
            // Install plugin files to global plugins/.
            let name = plugin_manager::install(home, source)?;
            // Enable in instance config.
            enable_plugin_in_config(&instance_home, &name)?;
            eprintln!("Installed plugin: {name} (enabled for instance '{instance}')");
            hint_restart_if_running(plugin_args);
        }
        "uninstall" | "remove" => {
            let name = plugin_args
                .get(1)
                .ok_or("usage: cortex plugin uninstall <name> [--id <instance>] [--purge]")?;
            if !Path::new(&instance_home).exists() {
                return Err(format!("instance '{instance}' does not exist"));
            }
            // Check that the plugin actually exists before claiming success.
            let global_exists = home.join("plugins").join(name.as_str()).exists();
            let enabled = read_enabled_plugins(&instance_home);
            let in_config = enabled.iter().any(|e| e == name);
            if !global_exists && !in_config {
                return Err(format!("plugin '{name}' is not installed"));
            }
            // Disable in instance config.
            disable_plugin_in_config(&instance_home, name)?;
            eprintln!("Disabled plugin: {name} (for instance '{instance}')");
            // --purge: also delete global files.
            if plugin_args.iter().any(|a| a == "--purge") {
                plugin_manager::uninstall(home, name)?;
                eprintln!("Removed plugin files: {name}");
            }
            hint_restart_if_running(plugin_args);
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
            let default_output = format!(
                "{}.cpx",
                dir_path.file_name().unwrap_or_default().to_string_lossy()
            );
            let output = plugin_args
                .get(2)
                .map_or(default_output.as_str(), String::as_str);
            plugin_manager::pack(dir_path, Path::new(output))?;
            eprintln!("Packed plugin: {output}");
        }
        _ => {
            return Err(format!(
                "unknown plugin command: {sub}. Use: install, uninstall, list, pack"
            ));
        }
    }
    Ok(())
}

/// Add a plugin name to `[plugins].enabled` in an instance's `config.toml`.
fn enable_plugin_in_config(instance_home: &str, plugin_name: &str) -> Result<(), String> {
    let config_path = Path::new(instance_home).join("config.toml");
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
fn disable_plugin_in_config(instance_home: &str, plugin_name: &str) -> Result<(), String> {
    let config_path = Path::new(instance_home).join("config.toml");
    let content = fs::read_to_string(&config_path).unwrap_or_default();

    let mut enabled = read_enabled_plugins(instance_home);
    enabled.retain(|e| e != plugin_name);

    write_enabled_plugins(&config_path, &content, &enabled)
}

/// Read the `[plugins].enabled` array from an instance's `config.toml`.
fn read_enabled_plugins(instance_home: &str) -> Vec<String> {
    let config_path = Path::new(instance_home).join("config.toml");
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
    if !unit_exists(instance_id.as_deref(), system) {
        return;
    }
    let svc = service_name(instance_id.as_deref());
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
    match cmd {
        "install" | "deploy" => Some(cmd_deploy(remaining_args)),
        "uninstall" | "undeploy" => Some(cmd_undeploy(remaining_args)),
        "start" => Some(cmd_start(remaining_args)),
        "stop" => Some(cmd_stop(remaining_args)),
        "restart" => Some(cmd_restart(remaining_args)),
        "status" => Some(cmd_status(remaining_args)),
        "ps" => Some(cmd_ps(None)),
        "reset" => Some(cmd_reset(remaining_args)),
        "plugin" => Some(cmd_plugin(remaining_args)),
        "channel" => {
            cmd_channel(remaining_args);
            Some(Ok(()))
        }
        "node" => Some(cmd_node(remaining_args)),
        "browser" => Some(cmd_browser(remaining_args)),
        _ => None,
    }
}

// ── Channel subcommand ──────────────────────────────────────

/// `cortex channel <telegram|whatsapp|pair> [options]`
///
/// Channels now run inside the daemon. This subcommand provides configuration
/// info and pairing management (file-based, no daemon connection needed).
///
/// # Errors
/// Returns an error string if the channel subcommand fails.
fn cmd_channel(args: &[String]) {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    let instance_home = PathBuf::from(resolve_instance_home(Some(id)));

    // Find the position of "channel" in args, then the first non-flag
    // arg after it is the sub-command.
    let channel_pos = args.iter().position(|a| a == "channel");
    let after_channel = channel_pos.map_or(args, |p| &args[p + 1..]);

    // Skip flag pairs like --id <value>
    let mut sub = None;
    let mut sub_idx = after_channel.len();
    let mut i = 0;
    while i < after_channel.len() {
        let a = &after_channel[i];
        if a == "--id" {
            i += 2; // skip flag + value
            continue;
        }
        if a.starts_with('-') {
            i += 1;
            continue;
        }
        sub = Some(a.as_str());
        sub_idx = i;
        break;
    }
    let sub = sub.unwrap_or("help");
    let remaining = if sub_idx < after_channel.len() {
        &after_channel[sub_idx + 1..]
    } else {
        &[]
    };

    match sub {
        "telegram" => cmd_channel_telegram(&instance_home),
        "whatsapp" => cmd_channel_whatsapp(&instance_home),
        "pair" => cmd_channel_pair(remaining, &instance_home),
        "approve" => cmd_channel_approve(remaining, &instance_home),
        "allow" => cmd_channel_list_op(remaining, &instance_home, "whitelist", true),
        "deny" => cmd_channel_list_op(remaining, &instance_home, "blacklist", true),
        "unallow" => cmd_channel_list_op(remaining, &instance_home, "whitelist", false),
        "undeny" => cmd_channel_list_op(remaining, &instance_home, "blacklist", false),
        "revoke" => cmd_channel_revoke(remaining, &instance_home),
        "policy" => cmd_channel_policy(remaining, &instance_home),
        _ => {
            eprintln!("Usage: cortex channel <subcommand>");
            eprintln!();
            eprintln!("Channels run inside the daemon automatically.");
            eprintln!("  telegram                   Show Telegram configuration info");
            eprintln!("  whatsapp                   Show WhatsApp configuration info");
            eprintln!("  pair [platform]            Show pending/paired users");
            eprintln!("  approve <platform> <id>    Approve a user (skip pairing code)");
            eprintln!("  allow <platform> <id>      Add user to whitelist");
            eprintln!("  deny <platform> <id>       Add user to blacklist");
            eprintln!("  unallow <platform> <id>    Remove user from whitelist");
            eprintln!("  undeny <platform> <id>     Remove user from blacklist");
            eprintln!("  revoke <platform> <id>     Remove a paired user");
            eprintln!("  policy <platform> [mode]   Show or set policy (pairing|whitelist|open)");
        }
    }
}

fn cmd_channel_telegram(home: &Path) {
    let auth_path = home.join("channels").join("telegram").join("auth.json");
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
        eprintln!("       CORTEX_TELEGRAM_TOKEN=123:ABC cortex deploy");
        eprintln!("    2. Or create channels/telegram/auth.json with {{\"bot_token\": \"...\"}}");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_whatsapp(home: &Path) {
    let auth_path = home.join("channels").join("whatsapp").join("auth.json");
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
        eprintln!("       CORTEX_WHATSAPP_TOKEN=EAA... cortex deploy");
        eprintln!("    2. Or create channels/whatsapp/auth.json with credentials");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_pair(args: &[String], home: &Path) {
    let platform = args.first().map(String::as_str);
    let platforms: Vec<&str> = match platform {
        Some(p) if p != "--id" => vec![p],
        _ => vec!["telegram", "whatsapp"],
    };

    for p in platforms {
        let dir = home.join("channels").join(p);
        eprintln!("=== {p} ===");

        // Read paired_users.json directly
        let paired: Vec<serde_json::Value> = fs::read_to_string(dir.join("paired_users.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        // Read pending_pairs.json directly
        let pending: Vec<serde_json::Value> = fs::read_to_string(dir.join("pending_pairs.json"))
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();

        if pending.is_empty() {
            eprintln!("  No pending pair requests.");
        } else {
            eprintln!("  Pending ({}):", pending.len());
            for pp in &pending {
                let uid = pp
                    .get("user_id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                let uname = pp
                    .get("user_name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                let code = pp
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                let created = pp
                    .get("created_at")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("?");
                eprintln!("    User: {uid} ({uname}) -- Code: {code} -- {created}");
            }
        }
        eprintln!("  Paired ({}):", paired.len());
        for pu in &paired {
            let uid = pu
                .get("user_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let name = pu
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("?");
            let since = pu
                .get("paired_at")
                .and_then(serde_json::Value::as_str)
                .map_or_else(|| "?".to_string(), format_paired_at);
            eprintln!("    {uid} ({name}) -- since {since}");
        }
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
        eprintln!("Usage: cortex channel approve <platform> <user_id>");
        eprintln!("  platform: telegram, whatsapp");
        eprintln!("  user_id:  the user's platform ID (shown in 'cortex channel pair')");
        return;
    }
    let platform = &args[0];
    let user_id = &args[1];
    let dir = home.join("channels").join(platform.as_str());

    if !dir.exists() {
        eprintln!("No channel directory for '{platform}'. Is the channel configured?");
        return;
    }

    // Read current paired users
    let paired_path = dir.join("paired_users.json");
    let mut paired: Vec<serde_json::Value> = fs::read_to_string(&paired_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    // Check if already paired
    if paired
        .iter()
        .any(|u| u.get("user_id").and_then(serde_json::Value::as_str) == Some(user_id.as_str()))
    {
        eprintln!("User {user_id} is already paired on {platform}.");
        return;
    }

    // Try to find name from pending pairs
    let pending_path = dir.join("pending_pairs.json");
    let mut pending: Vec<serde_json::Value> = fs::read_to_string(&pending_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let name = pending
        .iter()
        .find(|p| p.get("user_id").and_then(serde_json::Value::as_str) == Some(user_id.as_str()))
        .and_then(|p| p.get("user_name").and_then(serde_json::Value::as_str))
        .unwrap_or(user_id.as_str())
        .to_string();

    // Remove from pending
    pending
        .retain(|p| p.get("user_id").and_then(serde_json::Value::as_str) != Some(user_id.as_str()));
    if let Ok(json) = serde_json::to_string_pretty(&pending) {
        let _ = fs::write(&pending_path, json);
    }

    // Add to paired
    paired.push(serde_json::json!({
        "user_id": user_id,
        "name": name,
        "paired_at": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or_else(|_| "unknown".to_string(), |d| format!("{}s", d.as_secs())),
    }));
    if let Ok(json) = serde_json::to_string_pretty(&paired) {
        let _ = fs::write(&paired_path, json);
    }

    eprintln!("Approved: {user_id} ({name}) on {platform}.");
    eprintln!("The user can now chat. (Takes effect immediately, no restart needed.)");
}

fn cmd_channel_revoke(args: &[String], home: &Path) {
    if args.len() < 2 {
        eprintln!("Usage: cortex channel revoke <platform> <user_id>");
        return;
    }
    let platform = &args[0];
    let user_id = &args[1];
    let paired_path = home
        .join("channels")
        .join(platform.as_str())
        .join("paired_users.json");

    let mut paired: Vec<serde_json::Value> = fs::read_to_string(&paired_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();

    let before = paired.len();
    paired
        .retain(|u| u.get("user_id").and_then(serde_json::Value::as_str) != Some(user_id.as_str()));

    if paired.len() == before {
        eprintln!("User {user_id} not found in paired users on {platform}.");
        return;
    }

    if let Ok(json) = serde_json::to_string_pretty(&paired) {
        let _ = fs::write(&paired_path, json);
    }
    eprintln!("Revoked: {user_id} on {platform}. Takes effect immediately.");
}

/// Add or remove a user from a policy list (whitelist or blacklist).
fn cmd_channel_list_op(args: &[String], home: &Path, list_name: &str, add: bool) {
    if args.len() < 2 {
        let verb = if add { "add to" } else { "remove from" };
        eprintln!("Usage: cortex channel <cmd> <platform> <user_id>  ({verb} {list_name})");
        return;
    }
    let platform = &args[0];
    let user_id = &args[1];
    let dir = home.join("channels").join(platform.as_str());
    let policy_path = dir.join("policy.json");

    let mut policy: serde_json::Value = fs::read_to_string(&policy_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(
            || serde_json::json!({"mode": "pairing", "whitelist": [], "blacklist": []}),
        );

    // Ensure the list exists
    if policy.get(list_name).is_none() {
        policy[list_name] = serde_json::json!([]);
    }

    let list = policy[list_name].as_array_mut().unwrap();

    if add {
        if list.iter().any(|v| v.as_str() == Some(user_id.as_str())) {
            eprintln!("{user_id} already in {list_name} on {platform}.");
            return;
        }
        list.push(serde_json::Value::String(user_id.clone()));
        eprintln!("Added {user_id} to {list_name} on {platform}.");
    } else {
        let before = list.len();
        list.retain(|v| v.as_str() != Some(user_id.as_str()));
        if list.len() == before {
            eprintln!("{user_id} not found in {list_name} on {platform}.");
            return;
        }
        eprintln!("Removed {user_id} from {list_name} on {platform}.");
    }

    let _ = fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string_pretty(&policy) {
        let _ = fs::write(&policy_path, json);
    }
    eprintln!("Takes effect immediately, no restart needed.");
}

/// Show or set the channel access policy mode.
fn cmd_channel_policy(args: &[String], home: &Path) {
    if args.is_empty() {
        eprintln!("Usage: cortex channel policy <platform> [mode]");
        eprintln!("  Modes: pairing (default), whitelist, open");
        return;
    }
    let platform = &args[0];
    let dir = home.join("channels").join(platform.as_str());
    let policy_path = dir.join("policy.json");

    let mut policy: serde_json::Value = fs::read_to_string(&policy_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(
            || serde_json::json!({"mode": "pairing", "whitelist": [], "blacklist": []}),
        );

    if let Some(new_mode) = args.get(1) {
        let valid = ["pairing", "whitelist", "open"];
        if !valid.contains(&new_mode.as_str()) {
            eprintln!("Invalid mode '{new_mode}'. Use: pairing, whitelist, open");
            return;
        }
        policy["mode"] = serde_json::Value::String(new_mode.clone());
        let _ = fs::create_dir_all(&dir);
        if let Ok(json) = serde_json::to_string_pretty(&policy) {
            let _ = fs::write(&policy_path, json);
        }
        eprintln!("Policy for {platform} set to '{new_mode}'. Takes effect immediately.");
    } else {
        let mode = policy
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("pairing");
        let wl = policy
            .get("whitelist")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        let bl = policy
            .get("blacklist")
            .and_then(serde_json::Value::as_array)
            .map_or(0, Vec::len);
        eprintln!("{platform} policy:");
        eprintln!("  mode: {mode}");
        eprintln!("  whitelist: {wl} user(s)");
        eprintln!("  blacklist: {bl} user(s)");
        if wl > 0 {
            for u in policy["whitelist"].as_array().unwrap() {
                eprintln!("    + {}", u.as_str().unwrap_or("?"));
            }
        }
        if bl > 0 {
            for u in policy["blacklist"].as_array().unwrap() {
                eprintln!("    - {}", u.as_str().unwrap_or("?"));
            }
        }
    }
}

// ── Node.js management ────────────────────────────────────

fn cmd_node(args: &[String]) -> Result<(), String> {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    let home = PathBuf::from(resolve_instance_home(Some(id)));
    let data_dir = home.join("data");

    let sub = args
        .iter()
        .skip_while(|a| a.as_str() != "node")
        .nth(1)
        .map(String::as_str);

    match sub {
        Some("setup") => crate::node_manager::cmd_node_setup(&data_dir),
        Some("status") | None => {
            crate::node_manager::cmd_node_status(&data_dir);
            Ok(())
        }
        Some(other) => Err(format!("unknown node command: {other}. Use: setup, status")),
    }
}

// ── Browser management ────────────────────────────────────

fn cmd_browser(args: &[String]) -> Result<(), String> {
    let instance_id = parse_instance_id(args);
    let id = instance_id.as_deref().unwrap_or("default");
    let home = PathBuf::from(resolve_instance_home(Some(id)));
    let data_dir = home.join("data");

    let sub = args
        .iter()
        .skip_while(|a| a.as_str() != "browser")
        .nth(1)
        .map(String::as_str);

    match sub {
        Some("enable") => crate::node_manager::cmd_browser_enable(&home, &data_dir),
        Some("status") | None => {
            crate::node_manager::cmd_browser_status(&home);
            Ok(())
        }
        Some(other) => Err(format!(
            "unknown browser command: {other}. Use: enable, status"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_unit_file_contains_required_fields() {
        let unit = generate_unit_file("/usr/local/bin/cortex", "/home/user/.cortex", "default");
        assert!(unit.contains("ExecStart=/usr/local/bin/cortex --daemon --id default"));
        assert!(unit.contains("Environment=CORTEX_HOME=/home/user/.cortex"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("Type=simple"));
        assert!(unit.contains("After=network.target"));
        assert!(unit.contains("WantedBy=default.target"));
        assert!(!unit.contains("User="));
    }

    #[test]
    fn test_generate_system_unit_file_contains_required_fields() {
        let unit = generate_system_unit_file("/usr/local/bin/cortex", "/var/lib/cortex", "default");
        assert!(unit.contains("ExecStart=/usr/local/bin/cortex --daemon --id default"));
        assert!(unit.contains("Environment=CORTEX_HOME=/var/lib/cortex"));
        assert!(unit.contains("Restart=on-failure"));
        assert!(unit.contains("RestartSec=5"));
        assert!(unit.contains("Type=simple"));
        assert!(unit.contains("User=cortex"));
        assert!(unit.contains("After=network.target"));
        assert!(unit.contains("WantedBy=multi-user.target"));
    }

    #[test]
    fn test_generate_unit_file_custom_paths() {
        let unit = generate_unit_file("/opt/cortex/bin/cortex", "/var/lib/cortex", "work");
        assert!(unit.contains("ExecStart=/opt/cortex/bin/cortex --daemon --id work"));
        assert!(unit.contains("Environment=CORTEX_HOME=/var/lib/cortex"));
        let sys = generate_system_unit_file("/opt/cortex/bin/cortex", "/data/cortex", "prod");
        assert!(sys.contains("ExecStart=/opt/cortex/bin/cortex --daemon --id prod"));
        assert!(sys.contains("Environment=CORTEX_HOME=/data/cortex"));
    }

    #[test]
    fn test_parse_system_flag() {
        assert!(parse_system_flag(&["--system".to_string()]));
        assert!(parse_system_flag(&[
            "--purge".to_string(),
            "--system".to_string()
        ]));
        assert!(!parse_system_flag(&["--user".to_string()]));
        assert!(!parse_system_flag(&[]));
    }

    #[test]
    fn test_dispatch_recognizes_subcommands() {
        for cmd in &[
            "install",
            "uninstall",
            "deploy",
            "undeploy",
            "start",
            "stop",
            "status",
            "restart",
            "ps",
            "reset",
            "plugin",
            "channel",
        ] {
            assert!(dispatch(cmd, &[]).is_some(), "should recognize '{cmd}'");
        }
    }

    #[test]
    fn test_dispatch_ignores_non_subcommands() {
        assert!(dispatch("--acp", &[]).is_none());
        assert!(dispatch("foo", &[]).is_none());
    }

    #[test]
    fn test_undeploy_purge_flag_parsing() {
        let args = ["--purge".to_string()];
        assert!(args.iter().any(|a| a == "--purge"));
        let empty: [String; 0] = [];
        assert!(!empty.iter().any(|a| a == "--purge"));
    }

    #[test]
    fn test_system_unit_path() {
        assert_eq!(
            system_unit_path(),
            PathBuf::from("/etc/systemd/system/cortex.service")
        );
    }
}
