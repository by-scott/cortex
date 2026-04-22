//! Video understanding providers.
//!
//! Providers: `"zai"` (GLM-4V-Plus), `"gemini"`.

use base64::Engine;
use cortex_types::config::MediaConfig;

/// Default ZAI API base URL.
const ZAI_DEFAULT_URL: &str = "https://open.bigmodel.cn/api/paas";

/// Maximum video size for inline base64 encoding (10 MB).
const MAX_INLINE_BYTES: usize = 10 * 1024 * 1024;

/// Analyze a video file and return a textual description.
///
/// # Errors
///
/// Returns an error string if the provider is empty or analysis fails.
pub async fn understand(
    config: &MediaConfig,
    api_key: &str,
    video_path: &str,
    prompt: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    if config.video_understand.is_empty() {
        return Err(
            "Video understanding not configured. Set [media].video_understand in config.toml"
                .into(),
        );
    }
    match config.video_understand.as_str() {
        "gemini" => understand_gemini(config, api_key, video_path, prompt, client).await,
        _ => understand_zai(config, api_key, video_path, prompt, client).await,
    }
}

/// ZAI GLM-4V-Plus video understanding via chat completions.
///
/// For videos <= 10 MB, encodes inline as base64 data URL.
/// For larger videos, uploads via the ZAI file API first.
async fn understand_zai(
    config: &MediaConfig,
    api_key: &str,
    video_path: &str,
    prompt: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url(ZAI_DEFAULT_URL);
    let api_url = base.trim_end_matches('/');
    let model = if config.video_understand_model.is_empty() {
        "glm-4v-plus"
    } else {
        &config.video_understand_model
    };

    let video_bytes = tokio::fs::read(video_path)
        .await
        .map_err(|e| format!("read video: {e}"))?;

    // Build video content block
    let video_url_value = if video_bytes.len() <= MAX_INLINE_BYTES {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&video_bytes);
        format!("data:video/mp4;base64,{b64}")
    } else {
        // Upload to ZAI file API for large videos
        upload_to_zai_file_api(api_url, api_key, video_path, &video_bytes, client).await?
    };

    let url = format!("{api_url}/v4/chat/completions");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "messages": [{
                "role": "user",
                "content": [
                    {
                        "type": "video_url",
                        "video_url": { "url": video_url_value }
                    },
                    {
                        "type": "text",
                        "text": prompt
                    }
                ]
            }]
        }))
        .send()
        .await
        .map_err(|e| format!("chat request: {e}"))?;

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("API error: {err}"));
    }

    Ok(json["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// Upload a large video to ZAI file API and return the file URL.
async fn upload_to_zai_file_api(
    api_url: &str,
    api_key: &str,
    video_path: &str,
    video_bytes: &[u8],
    client: &reqwest::Client,
) -> Result<String, String> {
    let file_name = std::path::Path::new(video_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("video.mp4")
        .to_string();

    let part = reqwest::multipart::Part::bytes(video_bytes.to_vec())
        .file_name(file_name)
        .mime_str("video/mp4")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("purpose", "file-extract")
        .part("file", part);

    let url = format!("{api_url}/v4/files");
    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("file upload: {e}"))?;

    let json: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse upload: {e}"))?;

    json.get("id")
        .and_then(|v| v.as_str())
        .map(|id| format!("{api_url}/v4/files/{id}/content"))
        .ok_or_else(|| "no file id in upload response".to_string())
}

/// Gemini video understanding via the Gemini API.
async fn understand_gemini(
    config: &MediaConfig,
    api_key: &str,
    video_path: &str,
    prompt: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let model = if config.video_understand_model.is_empty() {
        "gemini-2.0-flash"
    } else {
        &config.video_understand_model
    };
    let base = config.effective_api_url("https://generativelanguage.googleapis.com");
    let api_url = base.trim_end_matches('/');

    let video_bytes = tokio::fs::read(video_path)
        .await
        .map_err(|e| format!("read video: {e}"))?;
    let b64 = base64::engine::general_purpose::STANDARD.encode(&video_bytes);

    let url = format!("{api_url}/v1beta/models/{model}:generateContent?key={api_key}");
    let resp = client
        .post(&url)
        .json(&serde_json::json!({
            "contents": [{
                "parts": [
                    {
                        "inline_data": {
                            "mime_type": "video/mp4",
                            "data": b64
                        }
                    },
                    {
                        "text": prompt
                    }
                ]
            }]
        }))
        .send()
        .await
        .map_err(|e| format!("gemini request: {e}"))?;

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("parse: {e}"))?;

    if let Some(err) = json.get("error") {
        return Err(format!("Gemini error: {err}"));
    }

    Ok(json["candidates"][0]["content"]["parts"][0]["text"]
        .as_str()
        .unwrap_or("")
        .to_string())
}
