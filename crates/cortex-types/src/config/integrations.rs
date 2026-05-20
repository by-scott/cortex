use std::collections::HashMap;

use serde::{Deserialize, Serialize};

// ── MCP Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportType,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportType {
    Stdio,
    Sse,
}

// ── ACP Config ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpConfig {
    pub clients: Vec<AcpClientConfig>,
    pub request_timeout_secs: u64,
}

impl Default for AcpConfig {
    fn default() -> Self {
        Self {
            clients: Vec::new(),
            request_timeout_secs: 120,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AcpClientConfig {
    pub id: String,
    pub ssh_host: String,
    pub initialize_format: String,
    pub protocol_version: String,
    pub client_name: String,
    pub client_version: String,
    pub command: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub env: HashMap<String, String>,
}

impl Default for AcpClientConfig {
    fn default() -> Self {
        Self {
            id: String::new(),
            ssh_host: String::new(),
            initialize_format: "standard".to_string(),
            protocol_version: "1".to_string(),
            client_name: "cortex".to_string(),
            client_version: String::new(),
            command: String::new(),
            args: Vec::new(),
            cwd: ".".to_string(),
            env: HashMap::new(),
        }
    }
}
