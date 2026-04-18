//! Image generation providers.
//!
//! Providers: `"zai"`, `"openai"`.

use base64::Engine;
use cortex_types::config::MediaConfig;

/// Default ZAI API base URL.
const ZAI_DEFAULT_URL: &str = "https://open.bigmodel.cn/api/paas";

/// Generate an image from a text prompt, returning the output file path.
///
/// # Errors
///
/// Returns an error string if the provider is empty or generation fails.
pub async fn generate(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    if config.image_gen.is_empty() {
        return Err("Image generation not configured. Set [media].image_gen in config.toml".into());
    }
    match config.image_gen.as_str() {
        "openai" => generate_openai(config, api_key, prompt, size, client).await,
        _ => generate_zai(config, api_key, prompt, size, client).await,
    }
}

/// ZAI / OpenAI-compatible image generation (`b64_json` response).
async fn generate_zai(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url(ZAI_DEFAULT_URL);
    let url = format!("{}/v1/images/generations", base.trim_end_matches('/'));
    let model = if config.image_gen_model.is_empty() {
        "cogview-3-plus"
    } else {
        &config.image_gen_model
    };

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "size": size,
            "n": 1,
            "response_format": "b64_json",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    decode_b64_image_response(resp).await
}

/// `OpenAI` DALL-E image generation.
async fn generate_openai(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url("https://api.openai.com");
    let url = format!("{}/v1/images/generations", base.trim_end_matches('/'));
    let model = if config.image_gen_model.is_empty() {
        "dall-e-3"
    } else {
        &config.image_gen_model
    };

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "size": size,
            "n": 1,
            "response_format": "b64_json",
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    decode_b64_image_response(resp).await
}

/// Shared helper: decode a `b64_json` image response and save to file.
async fn decode_b64_image_response(resp: reqwest::Response) -> Result<String, String> {
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if let Some(b64) = json
        .get("data")
        .and_then(|d| d.get(0))
        .and_then(|item| item.get("b64_json"))
        .and_then(|v| v.as_str())
    {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| e.to_string())?;
        let path = format!("/tmp/cortex-img-{}.png", uuid::Uuid::now_v7());
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| e.to_string())?;
        Ok(path)
    } else {
        let err = json
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("No image data in response");
        Err(err.to_string())
    }
}
