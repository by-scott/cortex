use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use cortex_types::config::ResolvedEndpoint;
use serde::{Deserialize, Serialize};

use crate::util::atomic_write;

const DEFAULT_TTL_HOURS: u64 = 168; // 7 days
/// Fallback context window size when online fetch fails.
/// Actual values are fetched from the model provider at runtime.
const FALLBACK_CONTEXT: usize = 200_000;
/// Fallback output token limit (used when online fetch fails).
const FALLBACK_MAX_OUTPUT: usize = 300_000;
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
        if let Some(info) = self.get(&key, ttl_hours) {
            return info.clone();
        }
        let info = fetch_model_info(endpoint)
            .await
            .unwrap_or_else(|_| ModelInfo {
                context_window: default_context_window,
                max_output_tokens: default_max_output,
                fetched_at: Utc::now(),
            });
        self.put(key, info.clone());
        info
    }

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.cache) {
            let _ = atomic_write(&self.path, json.as_bytes());
        }
    }
}

async fn fetch_model_info(endpoint: &ResolvedEndpoint) -> Result<ModelInfo, String> {
    use cortex_types::config::ProviderProtocol;
    match endpoint.protocol {
        ProviderProtocol::Anthropic => fetch_anthropic(endpoint).await,
        ProviderProtocol::OpenAI => fetch_openai(endpoint).await,
        ProviderProtocol::Ollama => fetch_ollama(endpoint).await,
    }
}

async fn fetch_anthropic(endpoint: &ResolvedEndpoint) -> Result<ModelInfo, String> {
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
    Ok(ModelInfo {
        context_window: json
            .get("context_window")
            .and_then(serde_json::Value::as_u64)
            .map_or(FALLBACK_CONTEXT, |v| {
                usize::try_from(v).unwrap_or(FALLBACK_CONTEXT)
            }),
        max_output_tokens: json
            .get("max_output_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(FALLBACK_MAX_OUTPUT, |v| {
                usize::try_from(v).unwrap_or(FALLBACK_MAX_OUTPUT)
            }),
        fetched_at: Utc::now(),
    })
}

async fn fetch_openai(endpoint: &ResolvedEndpoint) -> Result<ModelInfo, String> {
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
    Ok(ModelInfo {
        context_window: json
            .get("context_window")
            .and_then(serde_json::Value::as_u64)
            .map_or(FALLBACK_CONTEXT, |v| {
                usize::try_from(v).unwrap_or(FALLBACK_CONTEXT)
            }),
        max_output_tokens: json
            .get("max_output_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(FALLBACK_MAX_OUTPUT, |v| {
                usize::try_from(v).unwrap_or(FALLBACK_MAX_OUTPUT)
            }),
        fetched_at: Utc::now(),
    })
}

async fn fetch_ollama(endpoint: &ResolvedEndpoint) -> Result<ModelInfo, String> {
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

    let context = json
        .pointer("/model_info/context_length")
        .or_else(|| json.pointer("/parameters/context_length"))
        .and_then(serde_json::Value::as_u64)
        .map_or(FALLBACK_CONTEXT, |v| {
            usize::try_from(v).unwrap_or(FALLBACK_CONTEXT)
        });

    Ok(ModelInfo {
        context_window: context,
        max_output_tokens: FALLBACK_MAX_OUTPUT,
        fetched_at: Utc::now(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ttl_not_expired() {
        let info = ModelInfo {
            context_window: 100_000,
            max_output_tokens: FALLBACK_MAX_OUTPUT,
            fetched_at: Utc::now(),
        };
        assert!(!info.is_expired(24));
    }

    #[test]
    fn ttl_expired() {
        let info = ModelInfo {
            context_window: 100_000,
            max_output_tokens: FALLBACK_MAX_OUTPUT,
            fetched_at: Utc::now() - chrono::Duration::hours(169),
        };
        assert!(info.is_expired(24));
    }

    #[test]
    fn persist_and_load() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = ModelInfoStore::open(dir.path());
            store.put(
                "test:model".into(),
                ModelInfo {
                    context_window: 50_000,
                    max_output_tokens: 2048,
                    fetched_at: Utc::now(),
                },
            );
        }
        let store2 = ModelInfoStore::open(dir.path());
        let info = store2.get("test:model", DEFAULT_TTL_HOURS).unwrap();
        assert_eq!(info.context_window, 50_000);
    }

    #[test]
    fn expired_not_returned() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ModelInfoStore::open(dir.path());
        store.put(
            "old:model".into(),
            ModelInfo {
                context_window: 10_000,
                max_output_tokens: FALLBACK_MAX_OUTPUT,
                fetched_at: Utc::now() - chrono::Duration::hours(169),
            },
        );
        assert!(store.get("old:model", DEFAULT_TTL_HOURS).is_none());
    }
}
