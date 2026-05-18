use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use cortex_types::config::{
    AuthType, OpenAiImageInputMode, OpenAiThinkingParameter, ProviderConfig, ProviderProtocol,
    ProviderRegistry,
};

use super::CortexPaths;

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
image_input_mode = "data-url"
openai_stream_options = true
vision_max_output_tokens = 8192

[zai]
name = "ZhipuAI International (Anthropic)"
protocol = "anthropic"
base_url = "https://api.z.ai/api/anthropic"
auth_type = "x-api-key"
models = ["glm-5.1", "glm-5", "glm-4.7", "glm-4-plus", "glm-4.5-air"]
vision_provider = "zai-openai"
vision_model = "GLM-4.6V"
vision_max_output_tokens = 8192

[zai-openai]
name = "ZhipuAI International (OpenAI)"
protocol = "openai"
base_url = "https://api.z.ai/api/coding/paas/v4"
auth_type = "bearer"
models = ["glm-5.1", "glm-5", "glm-4.7", "glm-4-plus", "glm-4.5-air"]
vision_model = "GLM-4.6V"
image_input_mode = "data-url"
files_base_url = "https://api.z.ai/api/paas/v4"
vision_max_output_tokens = 8192

[zai-cn]
name = "ZhipuAI China (Anthropic)"
protocol = "anthropic"
base_url = "https://open.bigmodel.cn/api/anthropic"
auth_type = "x-api-key"
models = ["glm-4-plus"]
vision_provider = "zai-cn-openai"
vision_model = "GLM-4.6V"
vision_max_output_tokens = 8192

[zai-cn-openai]
name = "ZhipuAI China (OpenAI)"
protocol = "openai"
base_url = "https://open.bigmodel.cn/api/paas/v4"
auth_type = "bearer"
models = ["glm-4-plus"]
vision_model = "GLM-4.6V"
image_input_mode = "data-url"
files_base_url = "https://open.bigmodel.cn/api/paas/v4"
vision_max_output_tokens = 8192

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

/// Load `ProviderRegistry` from `providers.toml`.
///
/// On first run, if `CORTEX_PROVIDER` names a provider not in the default
/// registry and `CORTEX_BASE_URL` is set, the provider is auto-created
/// with protocol detection (try anthropic -> openai -> ollama).
///
/// # Errors
/// Returns `io::Error` if the default providers file cannot be written.
/// Returns `(registry, resolved_provider_name)`. The resolved name is `Some`
/// when `CORTEX_BASE_URL` was used to match or create a provider.
pub fn load_providers(home: &Path) -> io::Result<(ProviderRegistry, Option<String>)> {
    let paths = CortexPaths::new(home, "default");
    load_providers_for_file(&paths.config_files().providers)
}

/// Load `ProviderRegistry` using the shared base path layout.
///
/// # Errors
/// Returns `io::Error` if the providers registry cannot be initialized.
pub fn load_providers_for_paths(
    paths: &CortexPaths,
) -> io::Result<(ProviderRegistry, Option<String>)> {
    load_providers_for_file(&paths.config_files().providers)
}

fn load_providers_for_file(path: &Path) -> io::Result<(ProviderRegistry, Option<String>)> {
    ensure_default_providers(path)?;
    let content = fs::read_to_string(path).unwrap_or_default();
    let mut registry = parse_providers(&content);
    let mut dirty = apply_builtin_provider_defaults(&mut registry);
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
                    vision_provider: String::new(),
                    vision_model: String::new(),
                    image_input_mode: OpenAiImageInputMode::default(),
                    files_base_url: String::new(),
                    openai_stream_options: false,
                    openai_thinking_parameter: inferred_openai_thinking_parameter(&name),
                    vision_max_output_tokens: 0,
                    capability_cache_ttl_hours: 0,
                },
            );
            resolved_name = Some(name);
            dirty = true;
        }
    }

    // Deploy-time: CORTEX_EMBEDDING_BASE_URL overrides the embedding provider's base_url.
    if let Ok(embed_url) = std::env::var("CORTEX_EMBEDDING_BASE_URL") {
        let embed_provider = std::env::var("CORTEX_EMBEDDING_PROVIDER").unwrap_or_default();
        if !embed_provider.is_empty() {
            if let Some(pcfg) = registry.get_mut(&embed_provider) {
                pcfg.base_url = embed_url;
            }
            dirty = true;
        }
    }

    if dirty && let Ok(updated) = toml::to_string_pretty(&registry) {
        let _ = crate::atomic_write_text(path, updated);
    }

    Ok((registry, resolved_name))
}

fn apply_builtin_provider_defaults(registry: &mut ProviderRegistry) -> bool {
    let mut dirty = false;
    if let Some(provider) = registry.get_mut("zai") {
        if provider.vision_provider.is_empty() {
            provider.vision_provider = "zai-openai".into();
            dirty = true;
        }
        if provider.vision_max_output_tokens == 0 {
            provider.vision_max_output_tokens = 8192;
            dirty = true;
        }
    }
    if let Some(provider) = registry.get_mut("zai-cn") {
        if provider.vision_provider.is_empty() {
            provider.vision_provider = "zai-cn-openai".into();
            dirty = true;
        }
        if provider.vision_max_output_tokens == 0 {
            provider.vision_max_output_tokens = 8192;
            dirty = true;
        }
    }
    for name in ["zai-openai", "zai-cn-openai"] {
        if let Some(provider) = registry.get_mut(name) {
            if !matches!(provider.image_input_mode, OpenAiImageInputMode::DataUrl) {
                provider.image_input_mode = OpenAiImageInputMode::DataUrl;
                dirty = true;
            }
            if provider.vision_max_output_tokens == 0 {
                provider.vision_max_output_tokens = 8192;
                dirty = true;
            }
        }
    }
    if let Some(provider) = registry.get_mut("vllm")
        && matches!(
            provider.openai_thinking_parameter,
            OpenAiThinkingParameter::None
        )
    {
        provider.openai_thinking_parameter = OpenAiThinkingParameter::ChatTemplateThinking;
        dirty = true;
    }
    dirty
}

fn inferred_openai_thinking_parameter(provider_name: &str) -> OpenAiThinkingParameter {
    if provider_name.to_ascii_lowercase().contains("vllm") {
        OpenAiThinkingParameter::ChatTemplateThinking
    } else {
        OpenAiThinkingParameter::None
    }
}

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

fn probe_provider_protocol(base_url: &str) -> (ProviderProtocol, AuthType) {
    let url = base_url.trim_end_matches('/');
    if url.contains("anthropic") || url.contains("/anthropic") {
        (ProviderProtocol::Anthropic, AuthType::XApiKey)
    } else if url.contains("ollama") || url.contains(":11434") {
        (ProviderProtocol::Ollama, AuthType::None)
    } else {
        (ProviderProtocol::OpenAI, AuthType::Bearer)
    }
}

fn ensure_default_providers(path: &Path) -> io::Result<()> {
    if !path.exists() {
        crate::atomic_write_text(path, DEFAULT_PROVIDERS_TOML)?;
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
        registry.insert(key.clone(), parse_provider(key, t));
    }
    registry
}

fn parse_provider(key: &str, t: &toml::map::Map<String, toml::Value>) -> ProviderConfig {
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
    let vision_provider = t
        .get("vision_provider")
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    let image_input_mode = match t
        .get("image_input_mode")
        .and_then(toml::Value::as_str)
        .unwrap_or("data-url")
    {
        "upload-then-url" => OpenAiImageInputMode::UploadThenUrl,
        "remote-url-only" => OpenAiImageInputMode::RemoteUrlOnly,
        _ => OpenAiImageInputMode::DataUrl,
    };
    let files_base_url = t
        .get("files_base_url")
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_string();
    let openai_stream_options = t
        .get("openai_stream_options")
        .and_then(toml::Value::as_bool)
        .unwrap_or(false);
    let openai_thinking_parameter = t
        .get("openai_thinking_parameter")
        .and_then(toml::Value::as_str)
        .map_or_else(
            || inferred_openai_thinking_parameter(key),
            parse_openai_thinking_parameter,
        );
    let vision_max_output_tokens = t
        .get("vision_max_output_tokens")
        .and_then(toml::Value::as_integer)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(0);
    let capability_cache_ttl_hours = t
        .get("capability_cache_ttl_hours")
        .and_then(toml::Value::as_integer)
        .and_then(|value| u64::try_from(value).ok())
        .unwrap_or(0);

    ProviderConfig {
        name,
        protocol,
        base_url,
        auth_type,
        models,
        vision_provider,
        vision_model,
        image_input_mode,
        files_base_url,
        openai_stream_options,
        openai_thinking_parameter,
        vision_max_output_tokens,
        capability_cache_ttl_hours,
    }
}

fn parse_openai_thinking_parameter(value: &str) -> OpenAiThinkingParameter {
    match value {
        "top-level-thinking" | "thinking" => OpenAiThinkingParameter::TopLevelThinking,
        "chat-template-thinking" | "chat_template_thinking" => {
            OpenAiThinkingParameter::ChatTemplateThinking
        }
        "chat-template-enable-thinking" | "chat_template_enable_thinking" | "enable_thinking" => {
            OpenAiThinkingParameter::ChatTemplateEnableThinking
        }
        _ => OpenAiThinkingParameter::None,
    }
}
