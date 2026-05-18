use std::collections::HashMap;
use std::fs;
use std::path::Path;

use cortex_types::config::{CortexConfig, ProviderRegistry};

mod mcp;
mod paths;
mod providers;
mod summary;

pub use paths::{
    ActorBindingsStore, ChannelFileSet, ConfigFileSet, CortexPaths, RuntimeStateStore,
    ensure_base_dirs, ensure_home_dirs, resolve_home,
};
pub use providers::{load_providers, load_providers_for_paths};
pub use summary::{format_config_section, format_config_summary};

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
    let paths = CortexPaths::from_instance_home(home);
    load_config_for_files(&paths.config_files(), home, resolved_provider, providers)
}

#[must_use]
pub fn load_config_for_paths(
    paths: &CortexPaths,
    resolved_provider: Option<&str>,
    providers: &ProviderRegistry,
) -> CortexConfig {
    load_config_for_files(
        &paths.config_files(),
        &paths.instance_home(),
        resolved_provider,
        providers,
    )
}

fn load_config_for_files(
    files: &ConfigFileSet,
    instance_home: &Path,
    resolved_provider: Option<&str>,
    providers: &ProviderRegistry,
) -> CortexConfig {
    if !files.config.exists() {
        generate_default_config(&files.config, resolved_provider, providers);
    }
    write_defaults_toml(&files.config);
    let mut config: CortexConfig = fs::read_to_string(&files.config)
        .ok()
        .and_then(|s| toml::from_str(&s).ok())
        .unwrap_or_default();
    config.api.apply_preset();
    // MCP servers loaded from separate `mcp.toml` (overrides config.toml `[mcp]`)
    config.mcp = mcp::load_mcp_config_for_file(&files.mcp);

    // Persist channel auth tokens from env vars to independent auth.json files
    persist_channel_auth(instance_home);

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
    if let (Ok(app_id), Ok(app_secret)) = (
        std::env::var("CORTEX_QQ_APP_ID"),
        std::env::var("CORTEX_QQ_APP_SECRET"),
    ) {
        let sandbox = std::env::var("CORTEX_QQ_SANDBOX")
            .ok()
            .is_none_or(|v| matches!(v.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"));
        let remove_at = std::env::var("CORTEX_QQ_REMOVE_AT")
            .ok()
            .is_none_or(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"));
        let markdown = std::env::var("CORTEX_QQ_MARKDOWN")
            .ok()
            .is_none_or(|v| !matches!(v.as_str(), "0" | "false" | "FALSE" | "no" | "NO"));
        let max_retry = std::env::var("CORTEX_QQ_MAX_RETRY")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(10);
        save_channel_auth_file(
            home,
            "qq",
            &serde_json::json!({
                "app_id": app_id,
                "app_secret": app_secret,
                "mode": "websocket",
                "sandbox": sandbox,
                "markdown": markdown,
                "remove_at": remove_at,
                "max_retry": max_retry,
            }),
        );
    }
}

fn save_channel_auth_file(home: &Path, platform: &str, auth: &serde_json::Value) {
    let paths = CortexPaths::from_instance_home(home);
    let files = paths.channel_files(platform);
    let _ = fs::create_dir_all(&files.dir);
    if let Ok(json) = serde_json::to_string_pretty(auth) {
        let _ = fs::write(&files.auth, json);
    }
    // Generate default policy.json if missing
    if !files.policy.exists() {
        let policy = serde_json::json!({
            "mode": "pairing",
            "whitelist": [],
            "blacklist": [],
            "pair_code_ttl_secs": 300,
            "max_pending": 10,
        });
        if let Ok(json) = serde_json::to_string_pretty(&policy) {
            let _ = fs::write(&files.policy, json);
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
    let embedding_api_key_display = key_line("api_key", &cfg.embedding.api_key);
    let brave_key_display = key_line("brave_api_key", &cfg.web.brave_api_key);
    let endpoints = format_endpoints_toml(&cfg.api.endpoints);
    let ep_groups = format_endpoint_groups_toml(&cfg.api.endpoint_groups);
    let llm_groups = format_llm_groups_toml(&cfg.llm_groups);

    let content = format!(
        "\
# Cortex configuration
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
{embedding_api_key}
model = {emb_model:?}

[web]
search_backend = {search_backend:?}
{brave_key}

[daemon]
addr = \"127.0.0.1:0\"

[turn]
max_tool_iterations = 1024
execution_timeout_secs = 0
tool_timeout_secs = 1800
strip_think_tags = {strip_think_tags}

[memory]
max_recall = 10
auto_extract = true
extract_min_turns = 5
consolidation_similarity_threshold = 0.85
semantic_upgrade_similarity_threshold = 0.90

[tools]
disabled = []

[acp]
request_timeout_secs = 120
clients = []

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
        embedding_api_key = embedding_api_key_display,
        emb_model = cfg.embedding.model,
        search_backend = cfg.web.search_backend,
        brave_key = brave_key_display,
        strip_think_tags = cfg.turn.strip_think_tags,
        prompt = cfg.ui.prompt_symbol,
        locale = cfg.ui.locale,
        endpoints = endpoints.trim_end(),
        ep_groups = ep_groups.trim_end(),
        llm_groups = llm_groups.trim_start(),
    );

    let _ = fs::write(path, content);
}

/// Write factory defaults reference to `config.defaults.toml`.
fn write_defaults_toml(config_path: &Path) {
    let Some(parent) = config_path.parent() else {
        return;
    };
    let paths = CortexPaths::from_instance_home(parent);
    let defaults_path = paths.config_defaults_path();
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
            defaults_path,
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
        let _ = writeln!(
            out,
            "max_tokens = {}  # 0 = infer provider/model cap",
            g.max_tokens
        );
        let _ = writeln!(
            out,
            "capabilities = {}  # empty = infer from provider/model/profile",
            format_capabilities_toml(&g.capabilities)
        );
        let _ = writeln!(out, "context_tokens = {}  # 0 = infer", g.context_tokens);
        let _ = writeln!(out, "output_tokens = {}  # 0 = infer", g.output_tokens);
        let _ = writeln!(out, "latency_ms = {}  # 0 = infer by tier", g.latency_ms);
        let _ = writeln!(
            out,
            "input_cost_per_million = {:.2}  # 0 = infer by tier",
            g.input_cost_per_million
        );
        let _ = writeln!(
            out,
            "output_cost_per_million = {:.2}  # 0 = infer by tier",
            g.output_cost_per_million
        );
        let _ = writeln!(
            out,
            "safety_score = {:.2}  # 0 = infer by tier/model",
            g.safety_score
        );
        let _ = writeln!(
            out,
            "reasoning_depth = {:.2}  # 0 = infer by tier/model",
            g.reasoning_depth
        );
        let _ = writeln!(
            out,
            "json_reliability = {:.2}  # 0 = infer by protocol",
            g.json_reliability
        );
    }
    out
}

fn format_capabilities_toml(capabilities: &[cortex_types::ModelCapability]) -> String {
    let values = capabilities
        .iter()
        .map(|capability| format!("{:?}", capability.label()))
        .collect::<Vec<_>>()
        .join(", ");
    format!("[{values}]")
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
    if let Ok(key) = std::env::var("CORTEX_EMBEDDING_API_KEY") {
        config.embedding.api_key = key;
    }
    if let Ok(bk) = std::env::var("CORTEX_BRAVE_KEY") {
        config.web.brave_api_key = bk;
    }
    apply_thinking_env_override(config);
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

fn apply_thinking_env_override(config: &mut CortexConfig) {
    match std::env::var("CORTEX_SHOW_THINKING") {
        Ok(value) if value.trim().is_empty() => {}
        Ok(value) => match parse_bool_like(&value) {
            Some(show) => {
                config.turn.strip_think_tags = !show;
                return;
            }
            None => eprintln!(
                "Ignoring invalid CORTEX_SHOW_THINKING={value:?}; use true/false, 1/0, yes/no, or on/off."
            ),
        },
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!("Ignoring CORTEX_SHOW_THINKING because it is not valid UTF-8.");
        }
    }

    match std::env::var("CORTEX_STRIP_THINK_TAGS") {
        Ok(value) if value.trim().is_empty() => {}
        Ok(value) => match parse_bool_like(&value) {
            Some(strip) => config.turn.strip_think_tags = strip,
            None => eprintln!(
                "Ignoring invalid CORTEX_STRIP_THINK_TAGS={value:?}; use true/false, 1/0, yes/no, or on/off."
            ),
        },
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            eprintln!("Ignoring CORTEX_STRIP_THINK_TAGS because it is not valid UTF-8.");
        }
    }
}

#[must_use]
pub fn parse_bool_like(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "t" | "yes" | "y" | "on" | "show" | "visible" | "enabled" | "enable" => {
            Some(true)
        }
        "0" | "false" | "f" | "no" | "n" | "off" | "hide" | "hidden" | "disabled" | "disable" => {
            Some(false)
        }
        _ => None,
    }
}

/// Update one scalar key in `config.toml` while preserving the rest of the file.
///
/// `value_literal` must be a valid TOML literal, for example `true` or
/// `"secret"`. The updated file is parsed before writing so malformed config
/// changes fail without mutating the existing file.
///
/// # Errors
/// Returns an error when the file cannot be read/written or when the resulting
/// TOML would be invalid.
pub fn update_config_toml_value(
    config_path: &Path,
    section: &str,
    key: &str,
    value_literal: &str,
) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|err| format!("cannot read {}: {err}", config_path.display()))?;
    let line = format!("{key} = {value_literal}");
    let section_header = format!("[{section}]");
    let mut lines = Vec::new();
    let mut in_section = false;
    let mut saw_section = false;
    let mut replaced = false;

    for original in content.lines() {
        let trimmed = original.trim();
        if trimmed == section_header {
            in_section = true;
            saw_section = true;
            lines.push(original.to_string());
            continue;
        }
        if in_section && trimmed.starts_with('[') {
            if !replaced {
                lines.push(line.clone());
                replaced = true;
            }
            in_section = false;
        }
        if in_section && is_toml_key_line(trimmed, key) {
            lines.push(line.clone());
            replaced = true;
            continue;
        }
        lines.push(original.to_string());
    }

    if saw_section && in_section && !replaced {
        lines.push(line.clone());
        replaced = true;
    }
    if !saw_section {
        if !lines.is_empty() {
            lines.push(String::new());
        }
        lines.push(section_header);
        lines.push(line);
    } else if !replaced {
        lines.push(line);
    }

    let updated = lines.join("\n");
    toml::from_str::<toml::Value>(&updated).map_err(|err| {
        format!(
            "updated {} would be invalid TOML: {err}",
            config_path.display()
        )
    })?;
    fs::write(config_path, updated)
        .map_err(|err| format!("cannot write {}: {err}", config_path.display()))
}

fn is_toml_key_line(trimmed: &str, key: &str) -> bool {
    let Some(rest) = trimmed.strip_prefix(key) else {
        return false;
    };
    rest.trim_start().starts_with('=')
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
