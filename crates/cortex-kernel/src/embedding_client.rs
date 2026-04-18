use std::fmt;

use cortex_types::config::{AuthType, ProviderConfig, ProviderProtocol};
use reqwest::Client;

const ZERO_NORM_THRESHOLD: f64 = 1e-10;
const CONSTANT_VARIANCE_THRESHOLD: f64 = 1e-10;

#[derive(Debug)]
pub enum EmbeddingError {
    RequestFailed(String),
    ParseError(String),
    UnsupportedProtocol(String),
    DegradedVector(String),
}

impl fmt::Display for EmbeddingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RequestFailed(e) => write!(f, "embedding request failed: {e}"),
            Self::ParseError(e) => write!(f, "embedding parse error: {e}"),
            Self::UnsupportedProtocol(p) => write!(f, "unsupported embedding protocol: {p}"),
            Self::DegradedVector(r) => write!(f, "degraded embedding vector: {r}"),
        }
    }
}

impl std::error::Error for EmbeddingError {}

pub struct EmbeddingClient {
    http: Client,
    base_url: String,
    protocol: ProviderProtocol,
    auth_type: AuthType,
    api_key: String,
    model: String,
}

impl EmbeddingClient {
    #[must_use]
    pub fn new(provider: &ProviderConfig, api_key: &str, model: &str) -> Self {
        Self {
            http: Client::new(),
            base_url: provider.base_url.clone(),
            protocol: provider.protocol.clone(),
            auth_type: provider.auth_type.clone(),
            api_key: api_key.to_string(),
            model: model.to_string(),
        }
    }

    /// Return the configured model name.
    #[must_use]
    pub fn model_name(&self) -> &str {
        &self.model
    }

    /// Generate an embedding vector for the given text.
    ///
    /// # Errors
    /// Returns `EmbeddingError` if the request fails, the response cannot be parsed,
    /// or the protocol is unsupported.
    pub async fn embed(&self, text: &str) -> Result<Vec<f64>, EmbeddingError> {
        let vec = match self.protocol {
            ProviderProtocol::Ollama => self.embed_ollama(text).await?,
            ProviderProtocol::OpenAI => self.embed_openai(text).await?,
            ProviderProtocol::Anthropic => {
                return Err(EmbeddingError::UnsupportedProtocol(
                    "Anthropic does not provide embeddings".into(),
                ));
            }
        };
        validate_embedding(&vec)?;
        Ok(vec)
    }

    async fn embed_ollama(&self, text: &str) -> Result<Vec<f64>, EmbeddingError> {
        let url = format!("{}/api/embed", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": text
        });
        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| EmbeddingError::RequestFailed(e.to_string()))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::ParseError(e.to_string()))?;

        // Try new format: {"embeddings": [[...]]}
        if let Some(embeddings) = json.get("embeddings").and_then(serde_json::Value::as_array)
            && let Some(first) = embeddings.first().and_then(serde_json::Value::as_array)
        {
            return first
                .iter()
                .map(|v| {
                    v.as_f64()
                        .ok_or_else(|| EmbeddingError::ParseError("non-numeric value".into()))
                })
                .collect();
        }
        // Fall back to old format: {"embedding": [...]}
        if let Some(embedding) = json.get("embedding").and_then(serde_json::Value::as_array) {
            return embedding
                .iter()
                .map(|v| {
                    v.as_f64()
                        .ok_or_else(|| EmbeddingError::ParseError("non-numeric value".into()))
                })
                .collect();
        }

        Err(EmbeddingError::ParseError(
            "no embedding field in response".into(),
        ))
    }

    async fn embed_openai(&self, text: &str) -> Result<Vec<f64>, EmbeddingError> {
        let url = format!("{}/v1/embeddings", self.base_url);
        let body = serde_json::json!({
            "model": self.model,
            "input": text
        });
        let mut req = self.http.post(&url).json(&body);
        match self.auth_type {
            AuthType::Bearer => {
                req = req.bearer_auth(&self.api_key);
            }
            AuthType::XApiKey => {
                req = req.header("x-api-key", &self.api_key);
            }
            AuthType::None => {}
        }
        let resp = req
            .send()
            .await
            .map_err(|e| EmbeddingError::RequestFailed(e.to_string()))?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| EmbeddingError::ParseError(e.to_string()))?;
        let data = json
            .get("data")
            .and_then(serde_json::Value::as_array)
            .and_then(|arr| arr.first())
            .and_then(|obj| obj.get("embedding"))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| EmbeddingError::ParseError("missing data[0].embedding".into()))?;

        data.iter()
            .map(|v| {
                v.as_f64()
                    .ok_or_else(|| EmbeddingError::ParseError("non-numeric value".into()))
            })
            .collect()
    }
}

/// Validate that an embedding vector is usable.
///
/// # Errors
/// Returns `EmbeddingError::DegradedVector` if the vector is empty, zero, or constant.
pub fn validate_embedding(vec: &[f64]) -> Result<(), EmbeddingError> {
    if vec.is_empty() {
        return Err(EmbeddingError::DegradedVector("empty vector".into()));
    }
    let norm: f64 = vec.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm < ZERO_NORM_THRESHOLD {
        return Err(EmbeddingError::DegradedVector("zero vector".into()));
    }
    let len_f64 = f64::from(u32::try_from(vec.len()).unwrap_or(u32::MAX));
    let mean = vec.iter().sum::<f64>() / len_f64;
    let variance = vec.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / len_f64;
    if variance < CONSTANT_VARIANCE_THRESHOLD {
        return Err(EmbeddingError::DegradedVector("constant vector".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validate_empty() {
        assert!(validate_embedding(&[]).is_err());
    }

    #[test]
    fn validate_zero() {
        assert!(validate_embedding(&[0.0, 0.0, 0.0]).is_err());
    }

    #[test]
    fn validate_constant() {
        assert!(validate_embedding(&[1.0, 1.0, 1.0]).is_err());
    }

    #[test]
    fn validate_normal() {
        assert!(validate_embedding(&[0.1, 0.5, -0.3, 0.8]).is_ok());
    }

    #[test]
    fn validate_small_but_valid() {
        assert!(validate_embedding(&[0.001, -0.002, 0.003, -0.004]).is_ok());
    }

    #[test]
    fn anthropic_unsupported() {
        let provider = ProviderConfig {
            name: "Anthropic".into(),
            protocol: ProviderProtocol::Anthropic,
            base_url: "https://api.anthropic.com".into(),
            auth_type: AuthType::XApiKey,
            models: vec![],
            vision_model: String::new(),
        };
        let client = EmbeddingClient::new(&provider, "key", "model");
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(client.embed("test"));
        assert!(result.is_err());
    }
}
