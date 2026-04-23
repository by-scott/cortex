//! Node.js environment management for MCP servers and plugins.
//!
//! `cortex node setup` — detect or install Node.js + pnpm into ~/.cortex/default/data/node/
//! `cortex node status` — show Node.js status
//! `cortex browser enable` — configure chrome-devtools-mcp

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ── Detection ───────────────────────────────────────────────

struct NodeStatus {
    system_node: Option<String>,
    managed_node: Option<String>,
    managed_path: PathBuf,
    pnpm_available: bool,
    npx_path: Option<String>,
}

fn detect_node(data_dir: &Path) -> NodeStatus {
    let managed_path = data_dir.join("node");
    let managed_bin = managed_path.join("bin").join("node");

    let system_node = Command::new("node")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let managed_node = if managed_bin.exists() {
        Command::new(&managed_bin)
            .arg("--version")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    } else {
        None
    };

    let pnpm_available = Command::new("pnpm")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
        || managed_path.join("bin").join("pnpm").exists();

    let npx_path = if managed_path.join("bin").join("npx").exists() {
        Some(
            managed_path
                .join("bin")
                .join("npx")
                .to_string_lossy()
                .into_owned(),
        )
    } else {
        Command::new("which")
            .arg("npx")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };

    NodeStatus {
        system_node,
        managed_node,
        managed_path,
        pnpm_available,
        npx_path,
    }
}

// ── cortex node status ──────────────────────────────────────

pub fn cmd_node_status(data_dir: &Path) {
    let status = detect_node(data_dir);

    eprintln!("Node.js environment:");
    if let Some(ref v) = status.managed_node {
        eprintln!("  Managed: {v} ({})", status.managed_path.display());
    }
    if let Some(ref v) = status.system_node {
        eprintln!("  System:  {v}");
    }
    if status.managed_node.is_none() && status.system_node.is_none() {
        eprintln!("  Not installed. Run `cortex node setup` to install.");
    }
    eprintln!(
        "  pnpm:    {}",
        if status.pnpm_available {
            "available"
        } else {
            "not found"
        }
    );
    if let Some(ref p) = status.npx_path {
        eprintln!("  npx:     {p}");
    }
}

// ── cortex node setup ───────────────────────────────────────

/// # Errors
/// Returns error if node installation fails.
pub fn cmd_node_setup(data_dir: &Path) -> Result<(), String> {
    let status = detect_node(data_dir);

    if status.managed_node.is_some() {
        eprintln!(
            "Node.js already installed at {}",
            status.managed_path.display()
        );
        eprintln!(
            "To reinstall, delete {} and run again.",
            status.managed_path.display()
        );
        return Ok(());
    }

    if status.system_node.is_some() {
        eprintln!(
            "System Node.js detected: {}",
            status.system_node.as_deref().unwrap_or("?")
        );
        eprintln!("Cortex can use the system Node.js, or install its own copy.");
    } else {
        eprintln!("Node.js not found on this system.");
    }

    eprintln!();
    eprintln!("Options:");
    eprintln!(
        "  1) Install Node.js to {} (Cortex-managed, recommended)",
        status.managed_path.display()
    );
    eprintln!("  2) Install system-wide yourself:");
    eprintln!("     {}", suggest_node_install());
    eprintln!("     Then run `cortex node setup` again.");
    eprintln!();
    eprint!("Choose [1/2]: ");

    let mut input = String::new();
    if std::io::stdin().read_line(&mut input).is_err() {
        // Non-interactive (e.g. piped) — default to managed install
        input = "1".into();
    }

    match input.trim() {
        "1" | "" => {
            eprintln!("Installing managed Node.js...");
            install_node(&status.managed_path)?;
            install_pnpm(&status.managed_path)?;
            eprintln!("Node.js installed to {}", status.managed_path.display());
            eprintln!("pnpm installed.");
            eprintln!("MCP servers will use this environment automatically.");
            Ok(())
        }
        "2" => {
            eprintln!("Install Node.js manually, then run `cortex node setup` again.");
            Ok(())
        }
        other => Err(format!("invalid choice: {other}")),
    }
}

fn install_node(target: &Path) -> Result<(), String> {
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

    // Download latest LTS
    let url = format!("https://nodejs.org/dist/latest/node-latest-{os_name}-{arch_name}.tar.xz");

    eprintln!("Downloading Node.js from {url}...");

    let tmp = target.with_extension("tmp");
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).map_err(|e| format!("mkdir: {e}"))?;

    let tar_path = tmp.join("node.tar.xz");

    let status = Command::new("curl")
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
    let status = Command::new("tar")
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
    let _ = fs::remove_dir_all(target);
    fs::rename(&tmp, target).map_err(|e| format!("rename: {e}"))?;

    // Verify
    let node_bin = target.join("bin").join("node");
    if !node_bin.exists() {
        return Err("node binary not found after extraction".into());
    }

    let version = Command::new(&node_bin)
        .arg("--version")
        .output()
        .map_err(|e| format!("verify: {e}"))?;
    eprintln!(
        "Installed: {}",
        String::from_utf8_lossy(&version.stdout).trim()
    );
    Ok(())
}

fn install_pnpm(node_dir: &Path) -> Result<(), String> {
    let npm = node_dir.join("bin").join("npm");
    if !npm.exists() {
        return Err("npm not found in node installation".into());
    }

    eprintln!("Installing pnpm...");
    let status = Command::new(&npm)
        .args(["install", "-g", "pnpm"])
        .env(
            "PATH",
            format!(
                "{}:{}",
                node_dir.join("bin").display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
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
pub fn cmd_browser_enable(instance_home: &Path, data_dir: &Path) -> Result<(), String> {
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

    // 2. Check npx
    let status = detect_node(data_dir);
    let npx = status
        .npx_path
        .ok_or_else(|| "npx not found. Run `cortex node setup` first.".to_string())?;
    eprintln!("npx: {npx}");

    // 3. Write mcp.toml entry
    let mcp_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(instance_home),
    )
    .mcp;
    let chrome_path = chrome.unwrap_or_default();
    let entry = format!(
        "[[servers]]\nname = \"chrome-devtools\"\ntransport = \"stdio\"\ncommand = \"{npx}\"\n\
         args = [\"-y\", \"chrome-devtools-mcp@latest\", \"--executablePath\", \"{chrome_path}\", \"--headless\", \"--isolated\", \"--chromeArg=--no-sandbox\", \"--chromeArg=--disable-setuid-sandbox\"]\n\
         env = {{ CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS = \"1\" }}\n"
    );

    let mut content = fs::read_to_string(&mcp_path).unwrap_or_default();
    // Remove empty `servers = []` that conflicts with [[servers]] entries
    content = content.replace("servers = []", "");
    content = upsert_server_block(&content, "chrome-devtools", &entry);
    fs::write(&mcp_path, &content).map_err(|e| format!("write mcp.toml: {e}"))?;

    eprintln!("Browser MCP configured. Restart daemon to activate:");
    eprintln!("  cortex restart");
    eprintln!();
    eprintln!("Tools will appear as: mcp_chrome-devtools_*");
    Ok(())
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

fn suggest_node_install() -> String {
    let mut lines = Vec::new();
    lines.push("curl -fsSL https://fnm.vercel.app/install | bash && fnm install --latest".into());
    let pkg_cmd = if Path::new("/etc/debian_version").exists() {
        Some("sudo apt install nodejs npm")
    } else if Path::new("/etc/fedora-release").exists() {
        Some("sudo dnf install nodejs npm")
    } else if Path::new("/etc/arch-release").exists() {
        Some("sudo pacman -S nodejs npm")
    } else if Command::new("apk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        Some("sudo apk add nodejs npm")
    } else if Command::new("brew")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        Some("brew install node")
    } else {
        None
    };
    if let Some(cmd) = pkg_cmd {
        lines.push(format!("{cmd}  (version may be outdated)"));
    }
    lines.join("\n     Or: ")
}

fn detect_chrome() -> Option<String> {
    for cmd in [
        "chromium",
        "chromium-browser",
        "google-chrome",
        "google-chrome-stable",
    ] {
        if let Ok(output) = Command::new("which").arg(cmd).output()
            && output.status.success()
        {
            return Some(String::from_utf8_lossy(&output.stdout).trim().to_string());
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
    if Command::new("apk")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return "  sudo apk add chromium".into();
    }
    if Command::new("brew")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
    {
        return "  brew install --cask chromium".into();
    }
    "  Install Chromium from your package manager or https://www.chromium.org/".into()
}

// ── cortex browser status ───────────────────────────────────

/// # Errors
/// Returns error if browser teardown fails.
pub fn cmd_browser_disable(instance_home: &Path) -> Result<(), String> {
    let mcp_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(instance_home),
    )
    .mcp;
    let content = fs::read_to_string(&mcp_path).unwrap_or_default();
    let updated = remove_server_block(&content, "chrome-devtools");
    fs::write(&mcp_path, updated).map_err(|e| format!("write mcp.toml: {e}"))?;

    eprintln!("Browser MCP removed. Restart daemon to apply:");
    eprintln!("  cortex restart");
    Ok(())
}

pub fn cmd_browser_status(instance_home: &Path) {
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
