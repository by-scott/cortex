//! Text-to-speech synthesis providers.
//!
//! Providers: `"edge"` (edge-tts CLI), `"openai"`, `"zai"`.

use cortex_types::config::MediaConfig;

/// Default ZAI API base URL.
const ZAI_DEFAULT_URL: &str = "https://open.bigmodel.cn/api/paas";

/// Synthesize speech from text, returning the output file path.
///
/// # Errors
///
/// Returns an error string if synthesis fails.
pub async fn synthesize(
    config: &MediaConfig,
    api_key: &str,
    text: &str,
    voice: Option<&str>,
    client: &reqwest::Client,
) -> Result<String, String> {
    let voice = voice.unwrap_or(&config.tts_voice);
    match config.tts.as_str() {
        "openai" => synthesize_openai(config, api_key, text, voice, client).await,
        "zai" => synthesize_zai(config, api_key, text, voice, client).await,
        _ => synthesize_edge(text, voice).await,
    }
}

/// Edge-TTS CLI synthesis.
async fn synthesize_edge(text: &str, voice: &str) -> Result<String, String> {
    let output_path = format!("/tmp/cortex-tts-{}.mp3", uuid::Uuid::now_v7());
    let result = tokio::process::Command::new("edge-tts")
        .args([
            "--voice",
            voice,
            "--text",
            text,
            "--write-media",
            &output_path,
        ])
        .output()
        .await
        .map_err(|e| format!("edge-tts not found: {e}"))?;

    if result.status.success() {
        Ok(output_path)
    } else {
        Err(String::from_utf8_lossy(&result.stderr).to_string())
    }
}

/// OpenAI-compatible TTS API.
async fn synthesize_openai(
    config: &MediaConfig,
    api_key: &str,
    text: &str,
    voice: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let url = openai_compat_endpoint(config.tts_url("https://api.openai.com"), "audio/speech");

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": voice,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let output_path = format!("/tmp/cortex-tts-{}.mp3", uuid::Uuid::now_v7());
    tokio::fs::write(&output_path, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(output_path)
}

/// ZAI TTS API (OpenAI-compatible format, different default URL).
async fn synthesize_zai(
    config: &MediaConfig,
    api_key: &str,
    text: &str,
    voice: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let url = openai_compat_endpoint(config.tts_url(ZAI_DEFAULT_URL), "audio/speech");

    let resp = client
        .post(&url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": "tts-1",
            "input": text,
            "voice": voice,
        }))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let bytes = resp.bytes().await.map_err(|e| e.to_string())?;
    let output_path = format!("/tmp/cortex-tts-{}.mp3", uuid::Uuid::now_v7());
    tokio::fs::write(&output_path, &bytes)
        .await
        .map_err(|e| e.to_string())?;
    Ok(output_path)
}

fn openai_compat_endpoint(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.ends_with("/v4") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}
