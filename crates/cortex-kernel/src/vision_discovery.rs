use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Utc;
use cortex_types::config::{
    ApiConfig, ProviderConfig, ProviderRegistry, ResolvedEndpoint, VisionCapability,
};

use crate::util::atomic_write;

const DEFAULT_TTL_HOURS: u64 = 168; // 7 days

/// 1x1 transparent PNG for vision probing.
const PROBE_IMAGE_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAAC0lEQVQI12NgAAIABQABNjN9GQAAAAlwSFlzAAAWJQAAFiUBSVIk8AAAAA0lEQVQI12P4z8BQDwAEgAF/QualzQAAAABJRU5ErkJggg==";

pub struct VisionCapStore {
    path: PathBuf,
    cache: HashMap<String, VisionCapability>,
    ttl_hours: u64,
}

impl VisionCapStore {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("vision_caps.json");
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
    pub fn get(&self, key: &str) -> Option<&VisionCapability> {
        self.cache.get(key).filter(|cap| !self.is_expired(cap))
    }

    pub fn put(&mut self, key: String, cap: VisionCapability) {
        self.cache.insert(key, cap);
        self.persist();
    }

    #[must_use]
    pub fn is_expired(&self, cap: &VisionCapability) -> bool {
        let age = Utc::now().signed_duration_since(cap.probed_at);
        age.num_hours() >= i64::try_from(self.ttl_hours).unwrap_or(i64::MAX)
    }

    #[must_use]
    pub fn cache_key(base_url: &str, model: &str) -> String {
        format!("{base_url}:{model}")
    }

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.cache) {
            let _ = atomic_write(&self.path, json.as_bytes());
        }
    }
}

/// Check if a model name suggests vision support by naming convention.
#[must_use]
pub fn is_vision_model_name(model: &str) -> bool {
    let lower = model.to_lowercase();
    if lower.contains("vision") {
        return true;
    }
    // Pattern: ends with V preceded by digit or dot (e.g., GLM-4.5V)
    let bytes = model.as_bytes();
    if bytes.len() >= 2 {
        let last = bytes[bytes.len() - 1];
        let prev = bytes[bytes.len() - 2];
        if (last == b'V' || last == b'v') && (prev.is_ascii_digit() || prev == b'.') {
            return true;
        }
    }
    false
}

/// Discover vision models by name matching within a provider's model list.
#[must_use]
pub fn discover_by_name_matching(provider: &ProviderConfig) -> Vec<String> {
    provider
        .models
        .iter()
        .filter(|m| is_vision_model_name(m))
        .cloned()
        .collect()
}

/// Discover vision-capable models from an Ollama instance.
///
/// GET `/api/tags` → for each model, POST `/api/show` → check `.vision.` keys.
pub async fn discover_ollama_vision(base_url: &str) -> Vec<String> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    else {
        return vec![];
    };
    let Ok(resp) = client.get(format!("{base_url}/api/tags")).send().await else {
        return vec![];
    };
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };
    let Some(models) = body.get("models").and_then(|m| m.as_array()) else {
        return vec![];
    };
    let mut vision = Vec::new();
    for model in models {
        let Some(name) = model.get("name").and_then(|n| n.as_str()) else {
            continue;
        };
        let Ok(show) = client
            .post(format!("{base_url}/api/show"))
            .json(&serde_json::json!({"name": name}))
            .send()
            .await
        else {
            continue;
        };
        if let Ok(info) = show.json::<serde_json::Value>().await
            && let Some(model_info) = info.get("model_info").and_then(|m| m.as_object())
            && model_info.keys().any(|k| k.contains(".vision."))
        {
            vision.push(name.to_string());
        }
    }
    vision
}

/// Discover vision-capable models from `OpenRouter`.
///
/// GET `/v1/models` → filter by `architecture.modality` containing "image".
pub async fn discover_openrouter_vision(base_url: &str, api_key: &str) -> Vec<String> {
    let Ok(client) = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    else {
        return vec![];
    };
    let Ok(resp) = client
        .get(format!("{base_url}/v1/models"))
        .header("x-api-key", api_key)
        .send()
        .await
    else {
        return vec![];
    };
    let Ok(body) = resp.json::<serde_json::Value>().await else {
        return vec![];
    };
    let Some(data) = body.get("data").and_then(|d| d.as_array()) else {
        return vec![];
    };
    data.iter()
        .filter_map(|m| {
            let id = m.get("id")?.as_str()?;
            let modality = m.pointer("/architecture/modality")?.as_str()?;
            if modality.contains("image") {
                Some(id.to_string())
            } else {
                None
            }
        })
        .collect()
}

/// Aggregated vision model discovery using protocol-specific strategies.
///
/// - Ollama: `/api/tags` + `/api/show` to detect `.vision.` keys
/// - `OpenRouter`: `/v1/models` modality field
/// - Others: name matching (`vision` keyword, `V` suffix)
pub async fn discover_vision_models(
    endpoint: &ResolvedEndpoint,
    provider: &ProviderConfig,
    store: &mut VisionCapStore,
) -> Vec<String> {
    use cortex_types::config::ProviderProtocol;
    match endpoint.protocol {
        ProviderProtocol::Ollama => {
            let models = discover_ollama_vision(&endpoint.base_url).await;
            for model in &models {
                let key = VisionCapStore::cache_key(&endpoint.base_url, model);
                if store.get(&key).is_none() {
                    store.put(
                        key,
                        VisionCapability {
                            supported: true,
                            model_id: model.clone(),
                            probed_at: Utc::now(),
                        },
                    );
                }
            }
            models
        }
        ProviderProtocol::Anthropic if endpoint.base_url.contains("openrouter.ai") => {
            discover_openrouter_vision(&endpoint.base_url, &endpoint.api_key).await
        }
        _ => discover_by_name_matching(provider),
    }
}

/// Probe a specific endpoint for vision support by sending a tiny image.
///
/// # Errors
/// Returns `None` on any failure (timeout, parse error, etc.)
pub async fn probe_vision_support(endpoint: &ResolvedEndpoint) -> Option<bool> {
    use cortex_types::config::ProviderProtocol;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .ok()?;

    match endpoint.protocol {
        ProviderProtocol::Anthropic => probe_anthropic(&client, endpoint).await,
        ProviderProtocol::OpenAI => probe_openai(&client, endpoint).await,
        ProviderProtocol::Ollama => probe_ollama(&client, endpoint).await,
    }
}

async fn probe_anthropic(client: &reqwest::Client, endpoint: &ResolvedEndpoint) -> Option<bool> {
    let body = serde_json::json!({
        "model": endpoint.model,
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "image",
                "source": { "type": "base64", "media_type": "image/png", "data": PROBE_IMAGE_BASE64 }
            }, {
                "type": "text", "text": "ok"
            }]
        }]
    });
    let resp = client
        .post(format!("{}/v1/messages", endpoint.base_url))
        .header("x-api-key", &endpoint.api_key)
        .header("anthropic-version", "2023-06-01")
        .json(&body)
        .send()
        .await
        .ok()?;
    Some(resp.status().is_success())
}

async fn probe_openai(client: &reqwest::Client, endpoint: &ResolvedEndpoint) -> Option<bool> {
    let body = serde_json::json!({
        "model": endpoint.model,
        "max_tokens": 1,
        "messages": [{
            "role": "user",
            "content": [{
                "type": "image_url",
                "image_url": { "url": format!("data:image/png;base64,{PROBE_IMAGE_BASE64}") }
            }, {
                "type": "text", "text": "ok"
            }]
        }]
    });
    let mut req = client
        .post(format!("{}/v1/chat/completions", endpoint.base_url))
        .json(&body);
    if !endpoint.api_key.is_empty() {
        req = req.bearer_auth(&endpoint.api_key);
    }
    let resp = req.send().await.ok()?;
    Some(resp.status().is_success())
}

async fn probe_ollama(client: &reqwest::Client, endpoint: &ResolvedEndpoint) -> Option<bool> {
    let body = serde_json::json!({
        "model": endpoint.model,
        "prompt": "describe",
        "images": [PROBE_IMAGE_BASE64],
        "options": { "num_predict": 1 }
    });
    let resp = client
        .post(format!("{}/api/generate", endpoint.base_url))
        .json(&body)
        .send()
        .await
        .ok()?;
    Some(resp.status().is_success())
}

/// 3-level vision resolution:
/// 1. Explicit config (`api.vision`)
/// 2. Runtime discovery (`Ollama` API, `OpenRouter` modality, name matching)
/// 3. Probe the main model
pub async fn resolve_vision(
    api_config: &ApiConfig,
    providers: &ProviderRegistry,
    store: &mut VisionCapStore,
) -> Option<ResolvedEndpoint> {
    // Level 1: explicit config
    if !api_config.vision.provider.is_empty() && !api_config.vision.model.is_empty() {
        return ResolvedEndpoint::resolve_vision(api_config, providers)
            .ok()
            .flatten();
    }

    // Level 2: runtime discovery
    let primary = ResolvedEndpoint::resolve_primary(api_config, providers).ok()?;
    let provider = providers.get(&api_config.provider)?;
    let candidates = discover_vision_models(&primary, provider, store).await;
    if let Some(model) = candidates.first() {
        let key = VisionCapStore::cache_key(&primary.base_url, model);
        if let Some(cap) = store.get(&key)
            && cap.supported
        {
            let mut ep = primary.clone();
            ep.model.clone_from(model);
            return Some(ep);
        }
        // Verify via probe
        let mut probe_ep = primary.clone();
        probe_ep.model.clone_from(model);
        if let Some(supported) = probe_vision_support(&probe_ep).await {
            store.put(
                key,
                VisionCapability {
                    supported,
                    model_id: model.clone(),
                    probed_at: Utc::now(),
                },
            );
            if supported {
                return Some(probe_ep);
            }
        }
    }

    // Level 3: probe the main model (if no discovery candidates found)
    {
        let key = VisionCapStore::cache_key(&primary.base_url, &primary.model);
        if let Some(cap) = store.get(&key) {
            return if cap.supported { Some(primary) } else { None };
        }
        let supported = probe_vision_support(&primary).await.unwrap_or(false);
        store.put(
            key,
            VisionCapability {
                supported,
                model_id: primary.model.clone(),
                probed_at: Utc::now(),
            },
        );
        if supported { Some(primary) } else { None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vision_cache_crud() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = VisionCapStore::open(dir.path());
        let cap = VisionCapability {
            supported: true,
            model_id: "test-model".into(),
            probed_at: Utc::now(),
        };
        store.put("key1".into(), cap);
        assert!(store.get("key1").is_some());
        assert!(store.get("key1").unwrap().supported);
    }

    #[test]
    fn vision_cache_ttl() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = VisionCapStore::open(dir.path());
        let cap = VisionCapability {
            supported: true,
            model_id: "old".into(),
            probed_at: Utc::now() - chrono::Duration::hours(169),
        };
        store.put("expired".into(), cap);
        assert!(store.get("expired").is_none());
    }

    #[test]
    fn cache_key_format() {
        assert_eq!(
            VisionCapStore::cache_key("http://localhost", "model1"),
            "http://localhost:model1"
        );
    }

    #[test]
    fn name_matching_vision() {
        assert!(is_vision_model_name("GLM-4.5V"));
        assert!(is_vision_model_name("gpt-4-vision-preview"));
        assert!(!is_vision_model_name("gpt-4o"));
        assert!(!is_vision_model_name("claude-sonnet-4-20250514"));
    }

    #[test]
    fn discover_by_name() {
        let provider = ProviderConfig {
            name: "test".into(),
            protocol: cortex_types::config::ProviderProtocol::OpenAI,
            base_url: String::new(),
            auth_type: cortex_types::config::AuthType::None,
            models: vec!["gpt-4o".into(), "gpt-4-vision".into(), "GLM-4.5V".into()],
            vision_model: String::new(),
        };
        let vision = discover_by_name_matching(&provider);
        assert_eq!(vision.len(), 2);
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut store = VisionCapStore::open(dir.path());
            store.put(
                "k".into(),
                VisionCapability {
                    supported: true,
                    model_id: "m".into(),
                    probed_at: Utc::now(),
                },
            );
        }
        let store2 = VisionCapStore::open(dir.path());
        assert!(store2.get("k").is_some());
    }
}
