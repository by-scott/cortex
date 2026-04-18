//! Speech-to-text transcription providers.
//!
//! Providers: `"local"` (whisper CLI), `"openai"`, `"zai"`.

use cortex_types::config::MediaConfig;

/// Default ZAI API base URL.
const ZAI_DEFAULT_URL: &str = "https://open.bigmodel.cn/api/paas";

/// Transcribe audio using the configured STT provider.
///
/// # Errors
///
/// Returns an error string if transcription fails (CLI not found, API error, etc.).
pub async fn transcribe(
    config: &MediaConfig,
    api_key: &str,
    audio_path: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    match config.stt.as_str() {
        "openai" => transcribe_openai(config, api_key, audio_path, client).await,
        "zai" => transcribe_zai(config, api_key, audio_path, client).await,
        _ => transcribe_local(config, audio_path).await,
    }
}

/// Local STT via whisper CLI.
async fn transcribe_local(config: &MediaConfig, audio_path: &str) -> Result<String, String> {
    let output = tokio::process::Command::new("whisper")
        .args([
            "--model",
            &config.whisper_model,
            "--output_format",
            "txt",
            audio_path,
        ])
        .output()
        .await
        .map_err(|e| format!("whisper not found: {e}"))?;

    if output.status.success() {
        let txt_path = format!("{audio_path}.txt");
        tokio::fs::read_to_string(&txt_path)
            .await
            .map(|s| s.trim().to_string())
            .map_err(|e| e.to_string())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

/// OpenAI-compatible STT (Whisper API).
async fn transcribe_openai(
    config: &MediaConfig,
    api_key: &str,
    audio_path: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url("https://api.openai.com");
    let url = format!("{}/v1/audio/transcriptions", base.trim_end_matches('/'));

    let file_bytes = tokio::fs::read(audio_path)
        .await
        .map_err(|e| e.to_string())?;
    let file_name = std::path::Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.ogg")
        .to_string();

    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("audio/ogg")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("model", config.whisper_model.clone())
        .part("file", part);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(json
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}

/// ZAI STT (same OpenAI-compatible format, different default URL).
async fn transcribe_zai(
    config: &MediaConfig,
    api_key: &str,
    audio_path: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url(ZAI_DEFAULT_URL);
    let url = format!("{}/v1/audio/transcriptions", base.trim_end_matches('/'));

    let file_bytes = tokio::fs::read(audio_path)
        .await
        .map_err(|e| e.to_string())?;
    let file_name = std::path::Path::new(audio_path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("audio.ogg")
        .to_string();

    let part = reqwest::multipart::Part::bytes(file_bytes)
        .file_name(file_name)
        .mime_str("audio/ogg")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("model", config.whisper_model.clone())
        .part("file", part);

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    Ok(json
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}
