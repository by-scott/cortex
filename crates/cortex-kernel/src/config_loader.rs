use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cortex_types::config::{
    AuthType, CortexConfig, ProviderConfig, ProviderProtocol, ProviderRegistry,
};

const CORTEX_HOME_ENV: &str = "CORTEX_HOME";

const DEFAULT_PROVIDERS_TOML: &str = r#"[anthropic]
name = "Anthropic"
protocol = "anthropic"
base_url = "https://api.anthropic.com"
auth_type = "x-api-key"
models = ["claude-sonnet-4-20250514"]

[openrouter]
name = "OpenRouter"
protocol = "openai"
base_url = "https://openrouter.ai/api"
auth_type = "bearer"
models = []

[openai]
name = "OpenAI"
protocol = "openai"
base_url = "https://api.openai.com"
auth_type = "bearer"
models = ["gpt-4o"]
vision_model = "gpt-4o"

[zai]
name = "ZhipuAI International (Anthropic)"
protocol = "anthropic"
base_url = "https://api.z.ai/api/anthropic"
auth_type = "x-api-key"
models = ["glm-5.1", "glm-5", "glm-4.7", "glm-4-plus", "glm-4.5-air"]
vision_model = "GLM-4.6V"

[zai-openai]
name = "ZhipuAI International (OpenAI)"
protocol = "openai"
base_url = "https://api.z.ai/api/coding/paas/v4"
auth_type = "bearer"
models = ["glm-5.1", "glm-5", "glm-4.7", "glm-4-plus", "glm-4.5-air"]
vision_model = "GLM-4.6V"

[zai-cn]
name = "ZhipuAI China (Anthropic)"
protocol = "anthropic"
base_url = "https://open.bigmodel.cn/api/anthropic"
auth_type = "x-api-key"
models = ["glm-4-plus"]
vision_model = "GLM-4.6V"

[zai-cn-openai]
name = "ZhipuAI China (OpenAI)"
protocol = "openai"
base_url = "https://open.bigmodel.cn/api/paas/v4"
auth_type = "bearer"
models = ["glm-4-plus"]
vision_model = "GLM-4.6V"

[kimi]
name = "Kimi"
protocol = "openai"
base_url = "https://api.moonshot.cn"
auth_type = "bearer"
models = ["moonshot-v1-auto"]

[kimi-cn]
name = "Kimi China"
protocol = "openai"
base_url = "https://api.moonshot.cn"
auth_type = "bearer"
models = ["moonshot-v1-auto"]

[minimax]
name = "MiniMax"
protocol = "openai"
base_url = "https://api.minimax.chat"
auth_type = "bearer"
models = ["abab6.5s-chat"]

[ollama]
name = "Ollama"
protocol = "ollama"
base_url = "http://localhost:11434"
auth_type = "none"
models = []
"#;

/// Resolve the Cortex home directory.
/// Priority: CLI arg > `CORTEX_HOME` env > `$HOME/.cortex`
#[must_use]
pub fn resolve_home(cli_arg: Option<&str>) -> PathBuf {
    if let Some(arg) = cli_arg {
        return PathBuf::from(arg);
    }
    if let Ok(env) = std::env::var(CORTEX_HOME_ENV) {
        return PathBuf::from(env);
    }
    dirs_fallback()
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME").map_or_else(
        |_| PathBuf::from(".cortex"),
        |h| PathBuf::from(h).join(".cortex"),
    )
}

/// Create the standard directory structure under home.
///
/// # Errors
/// Returns `io::Error` if directories cannot be created.
/// Create the standard directory structure under an instance home.
pub fn ensure_home_dirs(home: &Path) -> io::Result<()> {
    for sub in [
        "prompts",
        "prompts/system",
        "prompts/.backup",
        "skills",
        "data",
        "memory",
        "sessions",
    ] {
        fs::create_dir_all(home.join(sub))?;
    }
    Ok(())
}

/// Ensure the base directory exists (holds providers.toml, shared across instances).
///
/// # Errors
/// Returns `io::Error` if the directory cannot be created.
pub fn ensure_base_dirs(base: &Path) -> io::Result<()> {
    fs::create_dir_all(base)
}

/// Load `CortexConfig` from `config.toml`. Returns default on missing/invalid file.
///
/// If the file does not exist, a default config.toml is written first.
#[must_use]
/// `resolved_provider`: provider name resolved by `load_providers` (URL match or auto-create).
/// `providers`: loaded registry, used to pick default model for the resolved provider.
pub fn load_config(
    home: &Path,
    resolved_provider: Option<&str>,
    providers: &ProviderRegistry,
) -> CortexConfig {
    let path = home.join("config.toml");
    if !path.exists() {
        generate_default_config(&path, resolved_provider, providers);
    }
    let mut config: CortexConfig = fs::read_to_string(&path)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    config.api.apply_preset();
    // MCP servers loaded from separate `mcp.toml` (overrides config.toml `[mcp]`)
    config.mcp = load_mcp_config(home);

    // Persist channel auth tokens from env vars to independent auth.json files
    persist_channel_auth(home);

    config
}

/// Save channel auth tokens from environment variables to
/// `channels/<platform>/auth.json` so the daemon can load them
/// independently of `config.toml`.
fn persist_channel_auth(home: &Path) {
    // Telegram — all fields with defaults
    if let Ok(token) = std::env::var("CORTEX_TELEGRAM_TOKEN") {
        save_channel_auth_file(
            home,
            "telegram",
            &serde_json::json!({
                "bot_token": token,
                "mode": "polling",
                "webhook_addr": "",
                "webhook_url": "",
            }),
        );
    }
    // WhatsApp — all fields with defaults
    if let Ok(token) = std::env::var("CORTEX_WHATSAPP_TOKEN") {
        let phone_id = std::env::var("CORTEX_WHATSAPP_PHONE_ID").unwrap_or_default();
        let verify = std::env::var("CORTEX_WHATSAPP_VERIFY_TOKEN").unwrap_or_default();
        save_channel_auth_file(
            home,
            "whatsapp",
            &serde_json::json!({
                "access_token": token,
                "phone_number_id": phone_id,
                "verify_token": verify,
                "mode": "webhook",
                "webhook_addr": "",
            }),
        );
    }
}

fn save_channel_auth_file(home: &Path, platform: &str, auth: &serde_json::Value) {
    let dir = home.join("channels").join(platform);
    let _ = fs::create_dir_all(&dir);
    if let Ok(json) = serde_json::to_string_pretty(auth) {
        let _ = fs::write(dir.join("auth.json"), json);
    }
    // Generate default policy.json if missing
    let policy_path = dir.join("policy.json");
    if !policy_path.exists() {
        let policy = serde_json::json!({
            "mode": "pairing",
            "whitelist": [],
            "blacklist": [],
            "pair_code_ttl_secs": 300,
            "max_pending": 10,
        });
        if let Ok(json) = serde_json::to_string_pretty(&policy) {
            let _ = fs::write(policy_path, json);
        }
    }
}

/// Generate `config.toml` from environment variables and provider defaults.
fn generate_default_config(
    path: &Path,
    resolved_provider: Option<&str>,
    providers: &ProviderRegistry,
) {
    let mut cfg = CortexConfig::default();
    apply_env_overrides(&mut cfg, resolved_provider, providers);
    populate_llm_groups(&mut cfg, providers);
    populate_endpoint_groups(&mut cfg);
    write_config_toml(path, &cfg);
    write_defaults_toml(path);
}

/// Write the user-facing `config.toml` with commonly-edited sections.
fn key_line(key: &str, value: &str) -> String {
    if value.is_empty() {
        String::new()
    } else {
        format!("{key} = {value:?}")
    }
}

fn write_config_toml(path: &Path, cfg: &CortexConfig) {
    let api_key_display = key_line("api_key", &cfg.api.api_key);
    let brave_key_display = key_line("brave_api_key", &cfg.web.brave_api_key);
    let endpoints = format_endpoints_toml(&cfg.api.endpoints);
    let ep_groups = format_endpoint_groups_toml(&cfg.api.endpoint_groups);
    let llm_groups = format_llm_groups_toml(&cfg.llm_groups);

    let content = format!(
        "\
# Cortex configuration — see docs/config.md for details
#
# Only commonly-edited settings are listed here.
# All other options use sensible defaults.
# Run `/config get <section>` to see all options.

[api]
provider = {provider:?}
{api_key}
model = {model:?}
preset = {preset:?}

[embedding]
provider = {emb_provider:?}
model = {emb_model:?}

[web]
search_backend = {search_backend:?}
{brave_key}

[daemon]
addr = \"127.0.0.1:0\"

[turn]
max_tool_iterations = 1024
tool_timeout_secs = 300

[turn.trace]
level = \"summary\"
# phase = \"minimal\"
# llm = \"full\"
# tool = \"summary\"

[memory]
max_recall = 10
auto_extract = true

[tools]
disabled = []

[rate_limit]
per_session_rpm = 10
global_rpm = 60

[plugins]
enabled = []

[ui]
prompt_symbol = {prompt:?}
locale = {locale:?}

# -- Sub-endpoint toggles (which background tasks use a separate LLM)
{endpoints}

# -- Sub-endpoint → LLM group mapping
{ep_groups}

# -- LLM groups (heavy = main conversations, medium = analysis, light = extraction)
{llm_groups}",
        provider = cfg.api.provider,
        api_key = api_key_display,
        model = cfg.api.model,
        preset = format!("{:?}", cfg.api.preset).to_lowercase(),
        emb_provider = cfg.embedding.provider,
        emb_model = cfg.embedding.model,
        search_backend = cfg.web.search_backend,
        brave_key = brave_key_display,
        prompt = cfg.ui.prompt_symbol,
        locale = cfg.ui.locale,
        endpoints = endpoints.trim_end(),
        ep_groups = ep_groups.trim_end(),
        llm_groups = llm_groups.trim_start(),
    );

    let _ = fs::write(path, content);
}

/// Write factory defaults reference to `data/defaults.toml`.
fn write_defaults_toml(config_path: &Path) {
    let Some(parent) = config_path.parent() else {
        return;
    };
    let data_dir = parent.join("data");
    let _ = fs::create_dir_all(&data_dir);
    let mut factory = CortexConfig::default();
    factory.api.apply_preset();
    // Populate endpoints/groups with defaults so they appear in the reference
    for ep in &[
        "memory_extract",
        "entity_extract",
        "compress",
        "summary",
        "self_update",
        "causal_analyze",
        "autonomous",
    ] {
        factory.api.endpoints.entry((*ep).into()).or_insert(true);
    }
    for (ep, group) in &[
        ("memory_extract", "light"),
        ("entity_extract", "light"),
        ("compress", "light"),
        ("summary", "light"),
        ("self_update", "medium"),
        ("causal_analyze", "medium"),
    ] {
        factory
            .api
            .endpoint_groups
            .entry((*ep).into())
            .or_insert_with(|| (*group).into());
    }
    for (name, model) in &[("heavy", ""), ("medium", ""), ("light", "")] {
        factory.llm_groups.entry((*name).into()).or_insert_with(|| {
            cortex_types::config::LlmGroupConfig {
                model: (*model).into(),
                ..Default::default()
            }
        });
    }
    if let Ok(full) = toml::to_string_pretty(&factory) {
        let _ = fs::write(
            data_dir.join("defaults.toml"),
            format!(
                "# Factory default configuration reference (read-only)\n\
                 # Add any section to config.toml to override.\n\n{full}"
            ),
        );
    }
}

/// Apply environment variable overrides to a config.
/// Format `[llm_groups.*]` entries with all fields and inline comments.
fn format_endpoints_toml(endpoints: &HashMap<String, bool>) -> String {
    use std::fmt::Write;
    let mut out = String::from("[api.endpoints]\n");
    for (name, enabled) in endpoints {
        let _ = writeln!(out, "{name} = {enabled}");
    }
    out
}

fn format_endpoint_groups_toml(groups: &HashMap<String, String>) -> String {
    use std::fmt::Write;
    let mut out = String::from("[api.endpoint_groups]\n");
    for (name, group) in groups {
        let _ = writeln!(out, "{name} = {group:?}");
    }
    out
}

fn format_llm_groups_toml(
    groups: &HashMap<String, cortex_types::config::LlmGroupConfig>,
) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    for (name, g) in groups {
        let _ = writeln!(out, "\n[llm_groups.{name}]");
        let _ = writeln!(out, "provider = {:?}", g.provider);
        let _ = writeln!(out, "model = {:?}", g.model);
        let _ = writeln!(
            out,
            "api_key = {:?}  # empty = inherit from [api]",
            g.api_key
        );
        let _ = writeln!(out, "max_tokens = {}  # 0 = provider default", g.max_tokens);
    }
    out
}

fn apply_env_overrides(
    config: &mut CortexConfig,
    resolved_provider: Option<&str>,
    providers: &ProviderRegistry,
) {
    if let Ok(key) = std::env::var("CORTEX_API_KEY") {
        config.api.api_key = key;
    }
    let provider_name = resolved_provider
        .map(String::from)
        .or_else(|| std::env::var("CORTEX_PROVIDER").ok())
        .unwrap_or_else(|| config.api.provider.clone());
    config.api.provider.clone_from(&provider_name);
    if let Ok(model) = std::env::var("CORTEX_MODEL") {
        config.api.model = model;
    } else if let Some(pcfg) = providers.get(&provider_name)
        && let Some(first) = pcfg.models.first()
    {
        config.api.model.clone_from(first);
    }
    if let Ok(ep) = std::env::var("CORTEX_EMBEDDING_PROVIDER") {
        config.embedding.provider = ep;
    }
    if let Ok(em) = std::env::var("CORTEX_EMBEDDING_MODEL") {
        config.embedding.model = em;
    }
    if let Ok(bk) = std::env::var("CORTEX_BRAVE_KEY") {
        config.web.brave_api_key = bk;
    }
    if let Ok(preset) = std::env::var("CORTEX_LLM_PRESET") {
        config.api.preset = match preset.to_lowercase().as_str() {
            "full" => cortex_types::config::LlmPreset::Full,
            "cognitive" => cortex_types::config::LlmPreset::Cognitive,
            "standard" => cortex_types::config::LlmPreset::Standard,
            _ => cortex_types::config::LlmPreset::Minimal,
        };
    }
    config.api.apply_preset();
}

/// Populate default LLM groups (heavy/medium/light) from provider model list.
fn populate_llm_groups(config: &mut CortexConfig, providers: &ProviderRegistry) {
    if !config.llm_groups.is_empty() {
        return;
    }
    let prov = &config.api.provider;
    let main_model = &config.api.model;
    let models: Vec<String> = providers
        .get(prov)
        .map(|p| p.models.clone())
        .unwrap_or_default();
    let medium_model = models
        .iter()
        .find(|m| {
            let l = m.to_lowercase();
            (l.contains("4.7") || l.contains("4-plus")) && m.as_str() != main_model
        })
        .cloned()
        .unwrap_or_else(|| main_model.clone());
    let light_model = models
        .iter()
        .find(|m| {
            let l = m.to_lowercase();
            (l.contains("air") || l.contains("lite") || l.contains("mini"))
                && m.as_str() != main_model
        })
        .cloned()
        .unwrap_or_else(|| medium_model.clone());
    let mk = |model: String| cortex_types::config::LlmGroupConfig {
        provider: prov.clone(),
        model,
        ..Default::default()
    };
    config
        .llm_groups
        .insert("heavy".into(), mk(main_model.clone()));
    config.llm_groups.insert("medium".into(), mk(medium_model));
    config.llm_groups.insert("light".into(), mk(light_model));
}

/// Populate default endpoint groups (light/medium tier mapping).
fn populate_endpoint_groups(config: &mut CortexConfig) {
    if !config.api.endpoint_groups.is_empty() {
        return;
    }
    for ep in &["memory_extract", "compress", "entity_extract", "summary"] {
        config
            .api
            .endpoint_groups
            .insert((*ep).to_string(), "light".into());
    }
    for ep in &["self_update", "causal_analyze"] {
        config
            .api
            .endpoint_groups
            .insert((*ep).to_string(), "medium".into());
    }
}

// ── MCP configuration (mcp.toml) ────────────────────────────

const DEFAULT_MCP_TOML_HEADER: &str = "\
# MCP server configuration — see docs/config.md#mcp for details
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

/// Load MCP server configuration from `mcp.toml`.
///
/// On first run, generates a default `mcp.toml` with commented examples.
/// When `CORTEX_CHROME_DEVTOOLS=1` is set during install, injects a
/// `chrome-devtools` server entry.
fn load_mcp_config(home: &Path) -> cortex_types::config::McpConfig {
    let path = home.join("mcp.toml");
    if !path.exists() {
        generate_default_mcp_toml(&path);
    }
    fs::read_to_string(&path)
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
    let _ = fs::write(path, body);
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

/// Load `ProviderRegistry` from `providers.toml`.
///
/// On first run, if `CORTEX_PROVIDER` names a provider not in the default
/// registry and `CORTEX_BASE_URL` is set, the provider is auto-created
/// with protocol detection (try anthropic → openai → ollama).
///
/// # Errors
/// Returns `io::Error` if the default providers file cannot be written.
/// Returns `(registry, resolved_provider_name)`. The resolved name is `Some`
/// when `CORTEX_BASE_URL` was used to match or create a provider.
pub fn load_providers(home: &Path) -> io::Result<(ProviderRegistry, Option<String>)> {
    ensure_default_providers(home)?;
    let path = home.join("providers.toml");
    let content = fs::read_to_string(&path).unwrap_or_default();
    let mut registry = parse_providers(&content);
    let mut resolved_name: Option<String> = None;

    // Deploy-time: CORTEX_BASE_URL triggers provider resolution.
    if let Ok(base_url) = std::env::var("CORTEX_BASE_URL") {
        let env_provider = std::env::var("CORTEX_PROVIDER").unwrap_or_default();

        if let Some((existing_name, _)) = registry.find_by_url(&base_url) {
            if !env_provider.is_empty() && env_provider != existing_name {
                eprintln!(
                    "Note: URL '{base_url}' matches existing provider '{existing_name}'. \
                     Using '{existing_name}'."
                );
            }
            resolved_name = Some(existing_name);
        } else {
            let name = if env_provider.is_empty() {
                derive_provider_name(&base_url)
            } else {
                env_provider
            };
            let (protocol, auth_type) = probe_provider_protocol(&base_url);
            let model = std::env::var("CORTEX_MODEL").unwrap_or_default();
            let models = if model.is_empty() {
                Vec::new()
            } else {
                vec![model]
            };
            eprintln!("Creating provider '{name}' for {base_url} (protocol: {protocol:?})");
            registry.insert(
                name.clone(),
                ProviderConfig {
                    name: name.clone(),
                    protocol,
                    base_url,
                    auth_type,
                    models,
                    vision_model: String::new(),
                },
            );
            resolved_name = Some(name);
            if let Ok(updated) = toml::to_string_pretty(&registry) {
                let _ = fs::write(&path, updated);
            }
        }
    }

    // Deploy-time: CORTEX_EMBEDDING_BASE_URL overrides the embedding provider's base_url.
    if let Ok(embed_url) = std::env::var("CORTEX_EMBEDDING_BASE_URL") {
        let embed_provider = std::env::var("CORTEX_EMBEDDING_PROVIDER").unwrap_or_default();
        if !embed_provider.is_empty() {
            if let Some(pcfg) = registry.get_mut(&embed_provider) {
                pcfg.base_url = embed_url;
            }
            if let Ok(updated) = toml::to_string_pretty(&registry) {
                let _ = fs::write(&path, updated);
            }
        }
    }

    Ok((registry, resolved_name))
}

/// Derive a provider name from a URL (e.g. `https://api.example.com` → `example`).
fn derive_provider_name(url: &str) -> String {
    url.trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("custom")
        .split(':')
        .next()
        .unwrap_or("custom")
        .split('.')
        .rev()
        .nth(1)
        .unwrap_or("custom")
        .to_string()
}

/// Probe a URL to determine protocol and auth type.
/// Priority: anthropic → openai → ollama.
fn probe_provider_protocol(base_url: &str) -> (ProviderProtocol, AuthType) {
    let url = base_url.trim_end_matches('/');
    // Heuristic: URL path or domain hints
    if url.contains("anthropic") || url.contains("/anthropic") {
        (ProviderProtocol::Anthropic, AuthType::XApiKey)
    } else if url.contains("ollama") || url.contains(":11434") {
        (ProviderProtocol::Ollama, AuthType::None)
    } else {
        // Default to OpenAI-compatible
        (ProviderProtocol::OpenAI, AuthType::Bearer)
    }
}

fn ensure_default_providers(home: &Path) -> io::Result<()> {
    let path = home.join("providers.toml");
    if !path.exists() {
        fs::write(path, DEFAULT_PROVIDERS_TOML)?;
    }
    Ok(())
}

fn parse_providers(toml_str: &str) -> ProviderRegistry {
    let table: HashMap<String, toml::Value> = toml::from_str(toml_str).unwrap_or_default();
    let mut registry = ProviderRegistry::new();
    for (key, value) in &table {
        let Some(t) = value.as_table() else {
            continue;
        };
        let name = t
            .get("name")
            .and_then(toml::Value::as_str)
            .unwrap_or(key)
            .to_string();
        let protocol_str = t
            .get("protocol")
            .and_then(toml::Value::as_str)
            .unwrap_or("openai");
        let protocol = match protocol_str {
            "anthropic" => ProviderProtocol::Anthropic,
            "ollama" => ProviderProtocol::Ollama,
            _ => ProviderProtocol::OpenAI,
        };
        let base_url = t
            .get("base_url")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .to_string();
        let auth_str = t
            .get("auth_type")
            .and_then(toml::Value::as_str)
            .unwrap_or("bearer");
        let auth_type = match auth_str {
            "x-api-key" => AuthType::XApiKey,
            "none" => AuthType::None,
            _ => AuthType::Bearer,
        };
        let models = t
            .get("models")
            .and_then(toml::Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(toml::Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        let vision_model = t
            .get("vision_model")
            .and_then(toml::Value::as_str)
            .unwrap_or("")
            .to_string();

        registry.insert(
            key.clone(),
            ProviderConfig {
                name,
                protocol,
                base_url,
                auth_type,
                models,
                vision_model,
            },
        );
    }
    registry
}

/// Format a human-readable config summary.
#[must_use]
pub fn format_config_summary(config: &CortexConfig, providers: &ProviderRegistry) -> String {
    use std::fmt::Write;
    let mut out = String::from("[Config Summary]\n");
    let _ = writeln!(
        out,
        "  Provider: {} | Model: {}",
        config.api.provider, config.api.model
    );
    let _ = writeln!(out, "  Providers loaded: {}", providers.len());
    let _ = writeln!(
        out,
        "  Memory: max_recall={}, decay_rate={}",
        config.memory.max_recall, config.memory.decay_rate
    );
    let _ = writeln!(
        out,
        "  Metacognition: doom_threshold={}, fatigue={}",
        config.metacognition.doom_loop_threshold, config.metacognition.fatigue_threshold
    );
    out
}

/// Format a specific config section.
///
/// # Errors
/// Returns an error string if the section name is unknown.
pub fn format_config_section(
    config: &CortexConfig,
    providers: &ProviderRegistry,
    section: &str,
) -> Result<String, String> {
    match section {
        "api" => Ok(format_section_api(config)),
        "context" => Ok(format_section_context(config)),
        "memory" => Ok(format_section_memory(config)),
        "embedding" => Ok(format_section_embedding(config)),
        "metacognition" => Ok(format_section_metacognition(config)),
        "turn" => Ok(format_section_turn(config)),
        "autonomous" => Ok(format_section_autonomous(config)),
        "tools" => Ok(format_section_tools(config)),
        "providers" => Ok(format_section_providers(providers)),
        "daemon" => Ok(format_section_daemon(config)),
        "web" => Ok(format_section_web(config)),
        "skills" => Ok(format_section_skills(config)),
        "auth" => Ok(format_section_auth(config)),
        "rate_limit" => Ok(format_section_rate_limit(config)),
        "health" => Ok(format_section_health(config)),
        "evolution" => Ok(format_section_evolution(config)),
        "ui" => Ok(format_section_ui(config)),
        "tls" => Ok(format_section_tls(config)),
        "plugins" => Ok(format_section_plugins(config)),
        "mcp" => Ok(format_section_mcp(config)),
        "llm_groups" => Ok(format_section_llm_groups(config)),
        "memory_share" => Ok(format_section_memory_share(config)),
        _ => Err(format!("unknown section: {section}")),
    }
}

fn format_section_api(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[api]");
    let _ = writeln!(out, "  provider = {}", config.api.provider);
    let _ = writeln!(out, "  model = {}", config.api.model);
    let api_key_display = if config.api.api_key.is_empty() {
        "(not set)"
    } else {
        "(set)"
    };
    let _ = writeln!(out, "  api_key = {api_key_display}");
    out
}

fn format_section_context(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[context]");
    let _ = writeln!(out, "  max_tokens = {}", config.context.max_tokens);
    let _ = writeln!(
        out,
        "  pressure_thresholds = {:?}",
        config.context.pressure_thresholds
    );
    out
}

fn format_section_memory(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[memory]");
    let _ = writeln!(out, "  max_recall = {}", config.memory.max_recall);
    let _ = writeln!(out, "  decay_rate = {}", config.memory.decay_rate);
    let _ = writeln!(out, "  auto_extract = {}", config.memory.auto_extract);
    out
}

fn format_section_embedding(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[embedding]");
    let _ = writeln!(out, "  provider = {}", config.embedding.provider);
    let _ = writeln!(out, "  model = {}", config.embedding.model);
    out
}

fn format_section_metacognition(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[metacognition]");
    let _ = writeln!(
        out,
        "  doom_loop_threshold = {}",
        config.metacognition.doom_loop_threshold
    );
    let _ = writeln!(
        out,
        "  fatigue_threshold = {}",
        config.metacognition.fatigue_threshold
    );
    out
}

fn format_section_turn(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[turn]");
    let _ = writeln!(
        out,
        "  max_tool_iterations = {}",
        config.turn.max_tool_iterations
    );
    let _ = writeln!(
        out,
        "  tool_timeout_secs = {}",
        config.turn.tool_timeout_secs
    );
    let _ = writeln!(out, "  strip_think_tags = {}", config.turn.strip_think_tags);
    out
}

fn format_section_autonomous(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[autonomous]");
    let _ = writeln!(out, "  enabled = {}", config.autonomous.enabled);
    let _ = writeln!(
        out,
        "  heartbeat_interval_secs = {}",
        config.autonomous.heartbeat_interval_secs
    );
    let _ = writeln!(out, "[autonomous.limits]");
    let _ = writeln!(
        out,
        "  max_llm_calls_per_hour = {}",
        config.autonomous.limits.max_llm_calls_per_hour
    );
    let _ = writeln!(
        out,
        "  cooldown_after_llm_secs = {}",
        config.autonomous.limits.cooldown_after_llm_secs
    );
    out
}

fn format_section_tools(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[tools]");
    let _ = writeln!(out, "  disabled = {:?}", config.tools.disabled);
    out
}

fn format_section_providers(providers: &ProviderRegistry) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[providers] ({} loaded)", providers.len());
    for (key, p) in providers.iter() {
        let _ = writeln!(out, "  {key}: {} ({})", p.name, p.base_url);
    }
    out
}

fn format_section_daemon(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[daemon]");
    let _ = writeln!(out, "  addr = {}", config.daemon.addr);
    let _ = writeln!(
        out,
        "  maintenance_interval_secs = {}",
        config.daemon.maintenance_interval_secs
    );
    out
}

fn format_section_web(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[web]");
    let _ = writeln!(out, "  search_backend = {}", config.web.search_backend);
    let brave_display = if config.web.brave_api_key.is_empty() {
        "(not set)"
    } else {
        "(set)"
    };
    let _ = writeln!(out, "  brave_api_key = {brave_display}");
    let _ = writeln!(
        out,
        "  brave_max_results = {}",
        config.web.brave_max_results
    );
    let _ = writeln!(out, "  fetch_max_chars = {}", config.web.fetch_max_chars);
    out
}

fn format_section_skills(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[skills]");
    let _ = writeln!(
        out,
        "  max_active_summaries = {}",
        config.skills.max_active_summaries
    );
    let _ = writeln!(
        out,
        "  default_timeout_secs = {}",
        config.skills.default_timeout_secs
    );
    let _ = writeln!(
        out,
        "  inject_summaries = {}",
        config.skills.inject_summaries
    );
    out
}

fn format_section_auth(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[auth]");
    let _ = writeln!(out, "  enabled = {}", config.auth.enabled);
    let _ = writeln!(
        out,
        "  token_expiry_hours = {}",
        config.auth.token_expiry_hours
    );
    out
}

fn format_section_rate_limit(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[rate_limit]");
    let _ = writeln!(
        out,
        "  per_session_rpm = {}",
        config.rate_limit.per_session_rpm
    );
    let _ = writeln!(out, "  global_rpm = {}", config.rate_limit.global_rpm);
    out
}

fn format_section_health(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[health]");
    let _ = writeln!(
        out,
        "  check_interval_turns = {}",
        config.health.check_interval_turns
    );
    let _ = writeln!(
        out,
        "  degraded_threshold = {}",
        config.health.degraded_threshold
    );
    out
}

fn format_section_evolution(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[evolution]");
    let _ = writeln!(
        out,
        "  source_modify_enabled = {}",
        config.evolution.source_modify_enabled
    );
    let _ = writeln!(
        out,
        "  correction_weight = {}",
        config.evolution.correction_weight
    );
    out
}

fn format_section_ui(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[ui]");
    let _ = writeln!(out, "  prompt_symbol = {}", config.ui.prompt_symbol);
    let _ = writeln!(out, "  locale = {}", config.ui.locale);
    out
}

fn format_section_tls(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[tls]");
    let _ = writeln!(out, "  enabled = {}", config.tls.enabled);
    let _ = writeln!(
        out,
        "  cert_path = {}",
        config.tls.cert_path.as_deref().unwrap_or("(not set)")
    );
    let _ = writeln!(
        out,
        "  key_path = {}",
        config.tls.key_path.as_deref().unwrap_or("(not set)")
    );
    out
}

fn format_section_plugins(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[plugins]");
    let _ = writeln!(out, "  dir = {}", config.plugins.dir);
    let _ = writeln!(out, "  enabled = {:?}", config.plugins.enabled);
    out
}

fn format_section_mcp(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[mcp]");
    let _ = writeln!(out, "  servers = {} configured", config.mcp.servers.len());
    for s in &config.mcp.servers {
        let _ = writeln!(out, "    - {} ({:?})", s.name, s.transport);
    }
    out
}

fn format_section_llm_groups(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[llm_groups] ({} defined)", config.llm_groups.len());
    for (name, g) in &config.llm_groups {
        let _ = writeln!(out, "  {name}: provider={} model={}", g.provider, g.model);
    }
    out
}

fn format_section_memory_share(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[memory_share]");
    let _ = writeln!(out, "  mode = {:?}", config.memory_share.mode);
    let _ = writeln!(out, "  instance_id = {}", config.memory_share.instance_id);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_home_cli_overrides() {
        let home = resolve_home(Some("/tmp/test-cortex"));
        assert_eq!(home, PathBuf::from("/tmp/test-cortex"));
    }

    #[test]
    fn ensure_dirs_creates_structure() {
        let dir = tempfile::tempdir().unwrap();
        ensure_home_dirs(dir.path()).unwrap();
        assert!(dir.path().join("prompts").is_dir());
        assert!(dir.path().join("data").is_dir());
        assert!(dir.path().join("memory").is_dir());
    }

    #[test]
    fn load_config_default_on_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config = load_config(dir.path(), None, &ProviderRegistry::new());
        assert_eq!(config.api.provider, "anthropic");
    }

    #[test]
    fn generated_config_has_header() {
        let dir = tempfile::tempdir().unwrap();
        let _ = load_config(dir.path(), None, &ProviderRegistry::new());
        let content = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        assert!(content.starts_with("# Cortex"));
    }

    #[test]
    fn load_config_parses_toml() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.toml"),
            "[api]\nprovider = \"ollama\"\nmodel = \"llama3\"\n",
        )
        .unwrap();
        let config = load_config(dir.path(), None, &ProviderRegistry::new());
        assert_eq!(config.api.provider, "ollama");
        assert_eq!(config.api.model, "llama3");
    }

    #[test]
    fn load_providers_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        let (registry, _) = load_providers(dir.path()).unwrap();
        assert!(!registry.is_empty());
        assert!(registry.get("anthropic").is_some());
        assert!(registry.get("ollama").is_some());
    }

    #[test]
    fn parse_providers_handles_all_protocols() {
        let registry = parse_providers(DEFAULT_PROVIDERS_TOML);
        let anthropic = registry.get("anthropic").unwrap();
        assert_eq!(anthropic.protocol, ProviderProtocol::Anthropic);
        let ollama = registry.get("ollama").unwrap();
        assert_eq!(ollama.protocol, ProviderProtocol::Ollama);
        let openai = registry.get("openai").unwrap();
        assert_eq!(openai.protocol, ProviderProtocol::OpenAI);
    }

    #[test]
    fn format_summary() {
        let config = CortexConfig::default();
        let providers = parse_providers(DEFAULT_PROVIDERS_TOML);
        let summary = format_config_summary(&config, &providers);
        assert!(summary.contains("Provider:"));
        assert!(summary.contains("Providers loaded:"));
    }

    #[test]
    fn format_section_unknown() {
        let config = CortexConfig::default();
        let providers = ProviderRegistry::new();
        assert!(format_config_section(&config, &providers, "nonexistent").is_err());
    }

    #[test]
    fn format_section_api() {
        let config = CortexConfig::default();
        let providers = ProviderRegistry::new();
        let out = format_config_section(&config, &providers, "api").unwrap();
        assert!(out.contains("[api]"));
        assert!(out.contains("provider"));
    }
}
