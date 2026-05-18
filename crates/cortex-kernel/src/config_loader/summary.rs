use cortex_types::config::{CortexConfig, ProviderRegistry};

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
        "  Thinking: request={}, output={}",
        if config.turn.strip_think_tags {
            "disabled"
        } else {
            "enabled"
        },
        if config.turn.strip_think_tags {
            "hidden"
        } else {
            "shown"
        }
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
        "acp" => Ok(format_section_acp(config)),
        "providers" => Ok(format_section_providers(providers)),
        "daemon" => Ok(format_section_daemon(config)),
        "web" => Ok(format_section_web(config)),
        "skills" => Ok(format_section_skills(config)),
        "auth" => Ok(format_section_auth(config)),
        "risk" => Ok(format_section_risk(config)),
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
    let _ = writeln!(
        out,
        "  extract_min_turns = {}",
        config.memory.extract_min_turns
    );
    let _ = writeln!(
        out,
        "  consolidate_interval_hours = {}",
        config.memory.consolidate_interval_hours
    );
    let _ = writeln!(
        out,
        "  consolidation_similarity_threshold = {}",
        config.memory.consolidation_similarity_threshold
    );
    let _ = writeln!(
        out,
        "  semantic_upgrade_similarity_threshold = {}",
        config.memory.semantic_upgrade_similarity_threshold
    );
    out
}

fn format_section_embedding(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[embedding]");
    let _ = writeln!(out, "  provider = {}", config.embedding.provider);
    let _ = writeln!(out, "  model = {}", config.embedding.model);
    let api_key_display = if config.embedding.api_key.is_empty() {
        "(not set)"
    } else {
        "(set)"
    };
    let _ = writeln!(out, "  api_key = {api_key_display}");
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
        "  execution_timeout_secs = {}",
        config.turn.execution_timeout_secs
    );
    let _ = writeln!(
        out,
        "  tool_timeout_secs = {}",
        config.turn.tool_timeout_secs
    );
    let _ = writeln!(out, "  strip_think_tags = {}", config.turn.strip_think_tags);
    let _ = writeln!(
        out,
        "  provider_thinking_request = {}",
        !config.turn.strip_think_tags
    );
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

fn format_section_acp(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[acp]");
    let _ = writeln!(
        out,
        "  request_timeout_secs = {}",
        config.acp.request_timeout_secs
    );
    let _ = writeln!(out, "  clients = {} configured", config.acp.clients.len());
    for client in &config.acp.clients {
        let _ = writeln!(out, "  - {}: {}", client.id, client.command);
    }
    out
}

fn format_section_providers(providers: &ProviderRegistry) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[providers] ({} loaded)", providers.len());
    for (key, p) in providers.iter() {
        let _ = writeln!(
            out,
            "  {key}: {} ({}) thinking={:?}",
            p.name, p.base_url, p.openai_thinking_parameter
        );
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

fn format_section_risk(config: &CortexConfig) -> String {
    use std::fmt::Write;
    let mut out = String::new();
    let _ = writeln!(out, "[risk]");
    let _ = writeln!(out, "  tool_policies = {}", config.risk.tools.len());
    for (name, policy) in &config.risk.tools {
        let _ = writeln!(
            out,
            "    - {name}: require_confirmation={}, block={}, allow_background={}",
            policy.require_confirmation, policy.block, policy.allow_background
        );
    }
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
        let _ = writeln!(
            out,
            "  {name}: provider={} model={} capabilities={}",
            g.provider,
            g.model,
            super::format_capabilities_toml(&g.capabilities)
        );
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
