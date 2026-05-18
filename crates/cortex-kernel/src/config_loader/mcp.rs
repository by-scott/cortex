use std::collections::HashMap;
use std::fs;
use std::path::Path;

const DEFAULT_MCP_TOML_HEADER: &str = "\
# MCP server configuration
#
# Each [[servers]] entry connects to an external MCP server at daemon startup.
# Tools are bridged into the Cortex registry as mcp_{name}_{tool}.
#
# Example:
#   [[servers]]
#   name = \"github\"
#   transport = \"stdio\"
#   command = \"npx\"
#   args = [\"-y\", \"@modelcontextprotocol/server-github\"]
#   env = { GITHUB_TOKEN = \"ghp_...\" }
";

pub(super) fn load_mcp_config_for_file(path: &Path) -> cortex_types::config::McpConfig {
    if !path.exists() {
        generate_default_mcp_toml(path);
    }
    fs::read_to_string(path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default()
}

/// Generate default `mcp.toml` with optional `chrome-devtools` entry.
fn generate_default_mcp_toml(path: &Path) {
    let mut mcp = cortex_types::config::McpConfig::default();
    if std::env::var("CORTEX_CHROME_DEVTOOLS").is_ok_and(|v| v == "1" || v == "true") {
        inject_chrome_devtools_mcp(&mut mcp);
        eprintln!("[info] Chrome DevTools MCP enabled. Prerequisites:");
        eprintln!("  1. Node.js + npm/pnpm:");
        eprintln!("       npm install -g chrome-devtools-mcp");
        eprintln!("       or: pnpm add -g chrome-devtools-mcp");
        eprintln!("  2. Chrome or Chromium browser:");
        eprintln!("       Debian/Ubuntu: sudo apt install chromium");
        eprintln!("       macOS: brew install --cask chromium");
        eprintln!("       or: https://www.google.com/chrome/");
    }
    let body = if mcp.servers.is_empty() {
        format!("{DEFAULT_MCP_TOML_HEADER}\nservers = []\n")
    } else {
        let serialized = toml::to_string_pretty(&mcp).unwrap_or_default();
        format!("{DEFAULT_MCP_TOML_HEADER}\n{serialized}")
    };
    let _ = crate::atomic_write_text(path, body);
}

/// Inject `chrome-devtools` MCP server configuration if not already present.
fn inject_chrome_devtools_mcp(mcp: &mut cortex_types::config::McpConfig) {
    if mcp.servers.iter().any(|s| s.name == "chrome-devtools") {
        return;
    }
    let mut env = HashMap::new();
    env.insert("CHROME_DEVTOOLS_MCP_NO_USAGE_STATISTICS".into(), "1".into());
    mcp.servers.push(cortex_types::config::McpServerConfig {
        name: "chrome-devtools".into(),
        transport: cortex_types::config::McpTransportType::Stdio,
        command: "npx".into(),
        args: vec!["-y".into(), "chrome-devtools-mcp@latest".into()],
        env,
        url: String::new(),
        headers: HashMap::new(),
    });
}
