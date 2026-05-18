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
#   command = \"/absolute/path/to/mcp-server\"
#   args = []
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

/// Generate default `mcp.toml`.
fn generate_default_mcp_toml(path: &Path) {
    let body = format!("{DEFAULT_MCP_TOML_HEADER}\nservers = []\n");
    let _ = crate::atomic_write_text(path, body);
}
