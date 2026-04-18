//! Image understanding via dedicated vision provider.
//!
//! When `[media].image_understand` is empty (default), images are handled
//! natively by the main LLM's vision capability.  This module is only
//! called when a dedicated provider is explicitly configured.

use base64::Engine;
use cortex_types::config::MediaConfig;

/// Analyze an image file and return a textual description.
///
/// # Errors
///
/// Returns an error string if the provider is unknown or the API call fails.
pub async fn understand(
    config: &MediaConfig,
    api_key: &str,
    image_path: &str,
    prompt: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    match config.image_understand.as_str() {
        "zai" => {
            let base = config.image_understand_url("https://open.bigmodel.cn/api/paas");
            let model = if config.image_understand_model.is_empty() {
                "glm-4v-plus"
            } else {
                &config.image_understand_model
            };
            call_vision_api(api_key, base, model, image_path, prompt, client).await
        }
        "openai" => {
            let base = config.image_understand_url("https://api.openai.com");
            let model = if config.image_understand_model.is_empty() {
                "gpt-4o"
            } else {
                &config.image_understand_model
            };
            call_vision_api(api_key, base, model, image_path, prompt, client).await
        }
        other => Err(format!("Unknown image_understand provider: {other}")),
    }
}

/// Send an image to an OpenAI-compatible vision chat completions endpoint.
async fn call_vision_api(
    api_key: &str,
    base_url: &str,
    model: &str,
    image_path: &str,
    prompt: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let image_bytes = tokio::fs::read(image_path)
        .await
        .map_err(|e| format!("read image: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&image_bytes);
    let mime = if image_path
        .rsplit('.')
        .next()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
    {
        "image/png"
    } else {
        "image/jpeg"
    };

    let url = format!("{}/v1/chat/completions", base_url.trim_end_matches('/'));
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {"type": "image_url", "image_url": {"url": format!("data:{mime};base64,{b64}")}},
                    {"type": "text", "text": prompt}
                ]
            }],
            "max_tokens": 1024,
        }))
        .send()
        .await
        .map_err(|e| format!("vision request: {e}"))?;

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("API error: {err}"));
    }

    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}
