use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use cortex_types::config::{ResolvedEndpoint, resolved_model_token_limits};
use serde::{Deserialize, Serialize};

use crate::util::atomic_write;

const DEFAULT_TTL_HOURS: u64 = 168; // 7 days
const LEGACY_FALLBACK_CONTEXT: usize = 200_000;
const LEGACY_FALLBACK_MAX_OUTPUT: usize = 300_000;
const HTTP_TIMEOUT_SECS: u64 = 10;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInfo {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub fetched_at: DateTime<Utc>,
}

impl ModelInfo {
    #[must_use]
    pub fn is_expired(&self, ttl_hours: u64) -> bool {
        let age = Utc::now().signed_duration_since(self.fetched_at);
        age.num_hours() >= i64::try_from(ttl_hours).unwrap_or(i64::MAX)
    }
}

pub struct ModelInfoStore {
    path: PathBuf,
    cache: HashMap<String, ModelInfo>,
    ttl_hours: u64,
}

impl ModelInfoStore {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("model_info.json");
        let cache = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self {
            path,
            cache,
            ttl_hours: DEFAULT_TTL_HOURS,
        }
    }

    #[must_use]
    pub fn get(&self, key: &str, ttl_hours: u64) -> Option<&ModelInfo> {
        self.cache
            .get(key)
            .filter(|info| !info.is_expired(ttl_hours))
    }

    pub fn put(&mut self, key: String, info: ModelInfo) {
        self.cache.insert(key, info);
        self.persist();
    }

    /// Get cached model info or fetch from the provider API.
    pub async fn get_or_fetch(
        &mut self,
        endpoint: &ResolvedEndpoint,
        default_context_window: usize,
        default_max_output: usize,
    ) -> ModelInfo {
        let key = format!("{}:{}", endpoint.base_url, endpoint.model);
        let ttl_hours = if endpoint.capability_cache_ttl_hours == 0 {
            self.ttl_hours
        } else {
            endpoint.capability_cache_ttl_hours
        };
        if let Some(info) = self.get(&key, ttl_hours).cloned() {
            let normalized = normalize_cached_model_info(
                endpoint,
                info.clone(),
                default_context_window,
                default_max_output,
            );
            if normalized.context_window != info.context_window
                || normalized.max_output_tokens != info.max_output_tokens
            {
                self.put(key, normalized.clone());
            }
            return normalized;
        }
        let fallback = fallback_model_info(endpoint, default_context_window, default_max_output);
        let info = fetch_model_info(endpoint, default_context_window, default_max_output)
            .await
            .unwrap_or(fallback);
        self.put(key, info.clone());
        info
    }

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.cache) {
            let _ = atomic_write(&self.path, json.as_bytes());
        }
    }
}

async fn fetch_model_info(
    endpoint: &ResolvedEndpoint,
    default_context_window: usize,
    default_max_output: usize,
) -> Result<ModelInfo, String> {
    use cortex_types::config::ProviderProtocol;
    match endpoint.protocol {
        ProviderProtocol::Anthropic => {
            fetch_anthropic(endpoint, default_context_window, default_max_output).await
        }
        ProviderProtocol::OpenAI => {
            fetch_openai(endpoint, default_context_window, default_max_output).await
        }
        ProviderProtocol::Ollama => {
            fetch_ollama(endpoint, default_context_window, default_max_output).await
        }
    }
}

async fn fetch_anthropic(
    endpoint: &ResolvedEndpoint,
    default_context_window: usize,
    default_max_output: usize,
) -> Result<ModelInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/v1/models/{}", endpoint.base_url, endpoint.model);
    let resp = client
        .get(&url)
        .header("x-api-key", &endpoint.api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(model_info_from_json(
        endpoint,
        &json,
        default_context_window,
        default_max_output,
    ))
}

async fn fetch_openai(
    endpoint: &ResolvedEndpoint,
    default_context_window: usize,
    default_max_output: usize,
) -> Result<ModelInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/v1/models/{}", endpoint.base_url, endpoint.model);
    let resp = client
        .get(&url)
        .bearer_auth(&endpoint.api_key)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(model_info_from_json(
        endpoint,
        &json,
        default_context_window,
        default_max_output,
    ))
}

async fn fetch_ollama(
    endpoint: &ResolvedEndpoint,
    default_context_window: usize,
    default_max_output: usize,
) -> Result<ModelInfo, String> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(HTTP_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())?;
    let url = format!("{}/api/show", endpoint.base_url);
    let body = serde_json::json!({ "model": endpoint.model });
    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let fallback = fallback_model_info(endpoint, default_context_window, default_max_output);
    let context = json
        .pointer("/model_info/context_length")
        .or_else(|| json.pointer("/parameters/context_length"))
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(fallback.context_window);

    Ok(ModelInfo {
        context_window: context,
        max_output_tokens: fallback.max_output_tokens,
        fetched_at: Utc::now(),
    })
}

fn model_info_from_json(
    endpoint: &ResolvedEndpoint,
    json: &serde_json::Value,
    default_context_window: usize,
    default_max_output: usize,
) -> ModelInfo {
    let fallback = fallback_model_info(endpoint, default_context_window, default_max_output);
    ModelInfo {
        context_window: json_usize(json, "context_window").unwrap_or(fallback.context_window),
        max_output_tokens: json_usize(json, "max_output_tokens")
            .unwrap_or(fallback.max_output_tokens),
        fetched_at: Utc::now(),
    }
}

fn json_usize(json: &serde_json::Value, key: &str) -> Option<usize> {
    json.get(key)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
}

fn fallback_model_info(
    endpoint: &ResolvedEndpoint,
    default_context_window: usize,
    default_max_output: usize,
) -> ModelInfo {
    let limits = resolved_model_token_limits(
        &endpoint.provider,
        &endpoint.protocol,
        &endpoint.model,
        default_context_window,
        default_max_output,
    );
    ModelInfo {
        context_window: limits.context_tokens,
        max_output_tokens: limits.output_tokens,
        fetched_at: Utc::now(),
    }
}

fn normalize_cached_model_info(
    endpoint: &ResolvedEndpoint,
    mut info: ModelInfo,
    default_context_window: usize,
    default_max_output: usize,
) -> ModelInfo {
    let fallback = fallback_model_info(endpoint, default_context_window, default_max_output);
    if info.context_window == LEGACY_FALLBACK_CONTEXT
        && fallback.context_window != LEGACY_FALLBACK_CONTEXT
    {
        info.context_window = fallback.context_window;
    }
    if info.max_output_tokens == LEGACY_FALLBACK_MAX_OUTPUT
        && fallback.max_output_tokens != LEGACY_FALLBACK_MAX_OUTPUT
    {
        info.max_output_tokens = fallback.max_output_tokens;
    }
    info
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::config::{AuthType, OpenAiImageInputMode, ProviderProtocol};

    fn endpoint(provider: &str, protocol: ProviderProtocol, model: &str) -> ResolvedEndpoint {
        ResolvedEndpoint {
            provider: provider.to_string(),
            base_url: "https://example.invalid".to_string(),
            protocol,
            auth_type: AuthType::Bearer,
            api_key: String::new(),
            model: model.to_string(),
            max_tokens: 0,
            vision_max_output_tokens: 0,
            image_input_mode: OpenAiImageInputMode::DataUrl,
            files_base_url: String::new(),
            openai_stream_options: false,
            openai_thinking_parameter: cortex_types::config::OpenAiThinkingParameter::None,
            capability_cache_path: String::new(),
            capability_cache_ttl_hours: 0,
        }
    }

    #[test]
    fn fallback_limits_are_model_specific_when_provider_metadata_is_missing() {
        let openai = endpoint("openai", ProviderProtocol::OpenAI, "gpt-4o");
        let claude = endpoint(
            "anthropic",
            ProviderProtocol::Anthropic,
            "claude-sonnet-4-20250514",
        );
        let local = endpoint("ollama", ProviderProtocol::Ollama, "llama3");

        let openai_info = fallback_model_info(&openai, 0, 0);
        let claude_info = fallback_model_info(&claude, 0, 0);
        let local_info = fallback_model_info(&local, 0, 0);

        assert_eq!(openai_info.context_window, 128_000);
        assert_eq!(openai_info.max_output_tokens, 16_384);
        assert_eq!(claude_info.context_window, 200_000);
        assert_eq!(claude_info.max_output_tokens, 8_192);
        assert_eq!(local_info.context_window, 8_192);
        assert_eq!(local_info.max_output_tokens, 4_096);
    }

    #[test]
    fn explicit_limits_override_model_specific_fallbacks() {
        let endpoint = endpoint("openai", ProviderProtocol::OpenAI, "gpt-4o");
        let info = fallback_model_info(&endpoint, 64_000, 2_048);

        assert_eq!(info.context_window, 64_000);
        assert_eq!(info.max_output_tokens, 2_048);
    }

    #[test]
    fn legacy_200k_300k_cache_entries_are_normalized_by_model() {
        let endpoint = endpoint("openai", ProviderProtocol::OpenAI, "gpt-4o");
        let stale = ModelInfo {
            context_window: LEGACY_FALLBACK_CONTEXT,
            max_output_tokens: LEGACY_FALLBACK_MAX_OUTPUT,
            fetched_at: Utc::now(),
        };

        let normalized = normalize_cached_model_info(&endpoint, stale, 0, 0);

        assert_eq!(normalized.context_window, 128_000);
        assert_eq!(normalized.max_output_tokens, 16_384);
    }
}
