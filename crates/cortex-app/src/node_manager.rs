//! Node.js environment management for MCP servers and plugins.
//!
//! `cortex node setup` — install Node.js + pnpm into ~/.cortex/default/data/node/
//! `cortex node status` — show Node.js status
//! `cortex browser enable` — configure chrome-devtools-mcp

use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const NODE_VERSION: &str = "v24.15.0";
const PNPM_VERSION: &str = "11.1.3";
const CHROME_DEVTOOLS_MCP_VERSION: &str = "1.0.1";
const NODE_ENV_DIR: &str = "node";
const BIN_DIR: &str = "bin";

#[derive(Debug, Clone)]
struct ManagedNode {
    root: PathBuf,
    bin_dir: PathBuf,
    node: PathBuf,
    npm: PathBuf,
    npx: PathBuf,
    pnpm: PathBuf,
}

impl ManagedNode {
    fn from_data_dir(data_dir: &Path) -> Self {
        let root = data_dir.join(NODE_ENV_DIR);
        let bin_dir = root.join(BIN_DIR);
        Self {
            node: bin_dir.join("node"),
            npm: bin_dir.join("npm"),
            npx: bin_dir.join("npx"),
            pnpm: bin_dir.join("pnpm"),
            root,
            bin_dir,
        }
    }

    fn path_env(&self) -> String {
        path_with_prefix(&self.bin_dir)
    }
}

// ── Detection ───────────────────────────────────────────────

struct NodeStatus {
    managed: ManagedNode,
    system_node: Option<(PathBuf, String)>,
    managed_node: Option<String>,
    pnpm: Option<String>,
    npx_available: bool,
}

fn detect_node(data_dir: &Path) -> NodeStatus {
    let managed = ManagedNode::from_data_dir(data_dir);

    let system_node =
        find_on_path("node").and_then(|path| command_version(&path).map(|version| (path, version)));

    let managed_node = if managed.node.exists() {
        command_version(&managed.node)
    } else {
        None
    };

    let pnpm = if managed.pnpm.exists() {
        command_version_with_path(&managed.pnpm, &managed.path_env())
    } else {
        None
    };

    let npx_available = managed.npx.exists();

    NodeStatus {
        managed,
        system_node,
        managed_node,
        pnpm,
        npx_available,
    }
}

#[must_use]
pub(crate) fn managed_node_bin_dir(data_dir: &Path) -> PathBuf {
    data_dir.join(NODE_ENV_DIR).join(BIN_DIR)
}

#[must_use]
pub(crate) fn path_with_managed_node_bin(data_dir: &Path) -> String {
    path_with_prefix(&managed_node_bin_dir(data_dir))
}

fn command_version(command: &Path) -> Option<String> {
    Command::new(command)
        .arg("--version")
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn command_version_with_path(command: &Path, path_env: &str) -> Option<String> {
    Command::new(command)
        .arg("--version")
        .env("PATH", path_env)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn find_on_path(program: &str) -> Option<PathBuf> {
    let path_env = std::env::var_os("PATH")?;
    std::env::split_paths(&path_env)
        .map(|dir| dir.join(program))
        .find(|candidate| is_executable(candidate))
}

fn is_executable(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
}

fn path_with_prefix(prefix: &Path) -> String {
    let mut paths = vec![prefix.to_path_buf()];
    if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current).filter(|path| path != prefix));
    } else {
        paths.extend([
            PathBuf::from("/usr/local/sbin"),
            PathBuf::from("/usr/local/bin"),
            PathBuf::from("/usr/bin"),
            PathBuf::from("/bin"),
        ]);
    }
    std::env::join_paths(paths)
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn require_host_tool(program: &str) -> Result<PathBuf, String> {
    find_on_path(program).ok_or_else(|| {
        format!("required tool not found: {program}. Install {program} and run `cortex node setup` again.")
    })
}

// ── cortex node status ──────────────────────────────────────

pub fn cmd_node_status(data_dir: &Path) {
    let status = detect_node(data_dir);

    eprintln!("Node.js environment:");
    if let Some(ref v) = status.managed_node {
        eprintln!("  Managed: {v} ({})", status.managed.root.display());
    } else {
        eprintln!(
            "  Managed: not installed ({})",
            status.managed.root.display()
        );
    }
    if let Some((ref path, ref version)) = status.system_node {
        eprintln!("  System:  {version} ({})", path.display());
    }
    if status.managed_node.is_none() && status.system_node.is_none() {
        eprintln!("  Not installed. Run `cortex node setup` to install.");
    }
    eprintln!(
        "  pnpm:    {}",
        status.pnpm.as_ref().map_or("not found", String::as_str)
    );
    eprintln!(
        "  npx:     {}",
        if status.npx_available {
            status.managed.npx.display().to_string()
        } else {
            "not found".to_string()
        }
    );
    eprintln!("  PATH+:   {}", status.managed.bin_dir.display());
}

// ── cortex node setup ───────────────────────────────────────

/// # Errors
/// Returns error if node installation fails.
pub fn cmd_node_setup(data_dir: &Path) -> Result<(), String> {
    let mut status = detect_node(data_dir);

    if let Some(ref version) = status.managed_node {
        eprintln!(
            "Managed Node.js already installed: {version} ({})",
            status.managed.root.display()
        );
    } else {
        eprintln!(
            "Installing Node.js {NODE_VERSION} to {}",
            status.managed.root.display()
        );
        install_node(&status.managed)?;
        status = detect_node(data_dir);
    }

    if let Some(ref version) = status.pnpm {
        eprintln!("Managed pnpm already installed: {version}");
    } else {
        install_pnpm(&status.managed)?;
    }

    let final_status = detect_node(data_dir);
    if final_status.managed_node.is_none()
        || final_status.pnpm.is_none()
        || !final_status.npx_available
    {
        return Err("managed Node.js environment is incomplete after setup".into());
    }

    eprintln!("Managed Node.js environment is ready.");
    eprintln!("  Root: {}", final_status.managed.root.display());
    eprintln!("  Bin:  {}", final_status.managed.bin_dir.display());
    eprintln!("Installed services will prepend this bin directory to PATH.");
    Ok(())
}

fn install_node(env: &ManagedNode) -> Result<(), String> {
    let curl = require_host_tool("curl")?;
    let tar = require_host_tool("tar")?;
    let arch = std::env::consts::ARCH;
    let os_name = match std::env::consts::OS {
        "linux" => "linux",
        "macos" => "darwin",
        _ => return Err("unsupported OS".into()),
    };
    let arch_name = match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        _ => return Err(format!("unsupported architecture: {arch}")),
    };

    let url = format!(
        "https://nodejs.org/dist/{NODE_VERSION}/node-{NODE_VERSION}-{os_name}-{arch_name}.tar.xz"
    );

    eprintln!("Downloading Node.js from {url}...");

    let tmp = env.root.with_extension("tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("mkdir: {e}"))?;

    let tar_path = tmp.join("node.tar.xz");

    let status = Command::new(curl)
        .args(["-fsSL", "-o"])
        .arg(&tar_path)
        .arg(&url)
        .status()
        .map_err(|e| format!("curl: {e}"))?;

    if !status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err("download failed".into());
    }

    eprintln!("Extracting...");
    let status = Command::new(tar)
        .args(["xf"])
        .arg(&tar_path)
        .arg("-C")
        .arg(&tmp)
        .arg("--strip-components=1")
        .status()
        .map_err(|e| format!("tar: {e}"))?;

    if !status.success() {
        let _ = fs::remove_dir_all(&tmp);
        return Err("extraction failed".into());
    }

    let _ = fs::remove_file(&tar_path);

    // Move to final location
    let _ = fs::remove_dir_all(&env.root);
    fs::rename(&tmp, &env.root).map_err(|e| format!("rename: {e}"))?;

    // Verify
    if !env.node.exists() {
        return Err("node binary not found after extraction".into());
    }

    let version = Command::new(&env.node)
        .arg("--version")
        .output()
        .map_err(|e| format!("verify: {e}"))?;
    eprintln!(
        "Installed: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );
    Ok(())
}

fn install_pnpm(env: &ManagedNode) -> Result<(), String> {
    if !env.npm.exists() {
        return Err("npm not found in node installation".into());
    }

    eprintln!("Installing pnpm {PNPM_VERSION}...");
    let package = format!("pnpm@{PNPM_VERSION}");
    let status = Command::new(&env.npm)
        .args(["install", "-g"])
        .arg(package)
        .env("PATH", env.path_env())
        .status()
        .map_err(|e| format!("npm install pnpm: {e}"))?;

    if !status.success() {
        return Err("pnpm installation failed".into());
    }
    Ok(())
}

// ── cortex browser enable ───────────────────────────────────

/// # Errors
/// Returns error if browser setup fails.
pub fn cmd_browser_enable(
    args: &[String],
    instance_home: &Path,
    data_dir: &Path,
) -> Result<(), String> {
    // 1. Check chromium/chrome
    let chrome = detect_chrome();
    if chrome.is_none() {
        eprintln!("Chrome/Chromium not found.");
        eprintln!();
        eprintln!("Install with:");
        eprintln!("{}", suggest_chrome_install());
        eprintln!();
        eprintln!("Then run `cortex browser enable` again.");
        return Err("chromium not installed".into());
    }
    eprintln!("Chrome found: {}", chrome.as_deref().unwrap_or("?"));

    // 2. Check managed npx
    let status = detect_node(data_dir);
    if status.managed_node.is_none() {
        return Err("managed Node.js is not installed. Run `cortex node setup` first.".to_string());
    }
    if !status.npx_available {
        return Err("managed npx is missing. Run `cortex node setup` first.".to_string());
    }
    let npx = status.managed.npx.to_string_lossy().into_owned();
    eprintln!("npx: {npx}");

    // 3. Write mcp.toml entry
    let mcp_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(instance_home),
    )
    .mcp;
    let chrome_path = chrome.unwrap_or_default();
    let entry = chrome_devtools_mcp_entry(&status.managed, &chrome_path)?;

    let mut content = fs::read_to_string(&mcp_path).unwrap_or_default();
    // Remove empty `servers = []` that conflicts with [[servers]] entries
    content = content.replace("servers = []", "");
    content = upsert_server_block(&content, "chrome-devtools", &entry);
    cortex_kernel::atomic_write_text(&mcp_path, &content)
        .map_err(|e| format!("write mcp.toml: {e}"))?;
    crate::deploy::reload_running_daemon_config(args);

    eprintln!("Browser MCP configured.");
    eprintln!("If the daemon is running, MCP tools will hot-reload shortly.");
    eprintln!();
    eprintln!("Tools will appear as: mcp_chrome-devtools_*");
    Ok(())
}

fn chrome_devtools_mcp_entry(env: &ManagedNode, chrome_path: &str) -> Result<String, String> {
    let mut server_env = HashMap::new();
    server_env.insert(
        "CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS".to_string(),
        "1".to_string(),
    );
    server_env.insert("PATH".to_string(), env.path_env());
    let server = cortex_types::config::McpServerConfig {
        name: "chrome-devtools".to_string(),
        transport: cortex_types::config::McpTransportType::Stdio,
        command: env.npx.to_string_lossy().into_owned(),
        args: vec![
            "-y".to_string(),
            format!("chrome-devtools-mcp@{CHROME_DEVTOOLS_MCP_VERSION}"),
            "--executablePath".to_string(),
            chrome_path.to_string(),
            "--headless".to_string(),
            "--isolated".to_string(),
            "--chromeArg=--no-sandbox".to_string(),
            "--chromeArg=--disable-setuid-sandbox".to_string(),
            "--no-usage-statistics".to_string(),
            "--no-performance-crux".to_string(),
        ],
        env: server_env,
        url: String::new(),
        headers: HashMap::new(),
    };
    toml::to_string_pretty(&cortex_types::config::McpConfig {
        servers: vec![server],
    })
    .map_err(|e| format!("serialize browser MCP config: {e}"))
}

fn rewrite_server_block(
    content: &str,
    server_name: &str,
    replacement_block: Option<&str>,
) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut out = Vec::new();
    let mut i = 0;
    let mut replaced = false;

    while i < lines.len() {
        if lines[i].trim() == "[[servers]]" {
            let start = i;
            let mut end = i + 1;
            let mut is_target = false;
            while end < lines.len() && lines[end].trim() != "[[servers]]" {
                if lines[end].trim() == format!("name = \"{server_name}\"") {
                    is_target = true;
                }
                end += 1;
            }

            if is_target {
                if !replaced && let Some(block) = replacement_block {
                    out.push(block.trim().to_string());
                    replaced = true;
                }
            } else {
                out.extend(lines[start..end].iter().map(|line| (*line).to_string()));
            }
            i = end;
            continue;
        }

        if !lines[i].trim().is_empty() {
            out.push(lines[i].to_string());
        }
        i += 1;
    }

    if !replaced && let Some(block) = replacement_block {
        if !out.is_empty() {
            out.push(String::new());
        }
        out.push(block.trim().to_string());
    }

    out.join("\n") + "\n"
}

fn upsert_server_block(content: &str, server_name: &str, replacement_block: &str) -> String {
    rewrite_server_block(content, server_name, Some(replacement_block))
}

pub(crate) fn remove_server_block(content: &str, server_name: &str) -> String {
    rewrite_server_block(content, server_name, None)
}

fn detect_chrome() -> Option<String> {
    for cmd in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if let Some(path) = find_on_path(cmd) {
            return Some(path.to_string_lossy().into_owned());
        }
    }
    None
}

fn suggest_chrome_install() -> String {
    // Detect package manager
    if Path::new("/etc/debian_version").exists() {
        return "  sudo apt install chromium".into();
    }
    if Path::new("/etc/fedora-release").exists() {
        return "  sudo dnf install chromium".into();
    }
    if Path::new("/etc/arch-release").exists() {
        return "  sudo pacman -S chromium".into();
    }
    if find_on_path("apk").is_some() {
        return "  sudo apk add chromium".into();
    }
    if find_on_path("brew").is_some() {
        return "  brew install --cask chromium".into();
    }
    "  Install Chromium from your package manager or https://www.chromium.org/".into()
}

// ── cortex browser status ───────────────────────────────────

/// # Errors
/// Returns error if browser teardown fails.
pub fn cmd_browser_disable(args: &[String], instance_home: &Path) -> Result<(), String> {
    let mcp_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(instance_home),
    )
    .mcp;
    let content = fs::read_to_string(&mcp_path).unwrap_or_default();
    let updated = remove_server_block(&content, "chrome-devtools");
    cortex_kernel::atomic_write_text(&mcp_path, updated)
        .map_err(|e| format!("write mcp.toml: {e}"))?;
    crate::deploy::reload_running_daemon_config(args);

    eprintln!("Browser MCP removed.");
    eprintln!("If the daemon is running, MCP tools will hot-reload shortly.");
    Ok(())
}

pub fn cmd_browser_status(instance_home: &Path, data_dir: &Path) {
    let node = detect_node(data_dir);
    let chrome = detect_chrome();
    let mcp_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(instance_home),
    )
    .mcp;
    let configured = fs::read_to_string(&mcp_path).is_ok_and(|c| c.contains("chrome-devtools"));

    eprintln!("Browser status:");
    if let Some(ref path) = chrome {
        eprintln!("  Chrome:     {path}");
    } else {
        eprintln!("  Chrome:     not found");
    }
    eprintln!(
        "  Node:       {}",
        node.managed_node
            .as_deref()
            .unwrap_or("managed Node.js not installed")
    );
    eprintln!(
        "  npx:        {}",
        if node.npx_available {
            node.managed.npx.display().to_string()
        } else {
            "not found".to_string()
        }
    );
    eprintln!(
        "  MCP config: {}",
        if configured {
            "enabled"
        } else {
            "not configured"
        }
    );
    if configured {
        eprintln!("  Run `cortex browser disable` to remove it.");
    } else {
        eprintln!("  Run `cortex browser enable` to set up.");
    }
}
