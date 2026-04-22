//! Video generation providers.
//!
//! Providers: `"zai"` (`CogVideoX`).

use cortex_types::config::MediaConfig;

/// Default ZAI API base URL.
const ZAI_DEFAULT_URL: &str = "https://open.bigmodel.cn/api/paas";

/// Generate a video from a text prompt, returning the output file path.
///
/// This is an async operation: submit -> poll -> download.
///
/// # Errors
///
/// Returns an error string if the provider is empty or generation fails.
pub async fn generate(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    quality: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    if config.video_gen.is_empty() {
        return Err("Video generation not configured. Set [media].video_gen in config.toml".into());
    }
    // Currently only ZAI provider is supported
    generate_zai(config, api_key, prompt, size, quality, client).await
}

/// ZAI `CogVideoX` video generation: submit -> poll -> download.
async fn generate_zai(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    quality: &str,
    client: &reqwest::Client,
) -> Result<String, String> {
    let base = config.effective_api_url(ZAI_DEFAULT_URL);
    let api_url = base.trim_end_matches('/');
    let model = if config.video_gen_model.is_empty() {
        "cogvideox-3"
    } else {
        &config.video_gen_model
    };

    // 1. Submit generation request
    let submit_url = format!("{api_url}/v4/videos/generations");
    let resp = client
        .post(&submit_url)
        .header("Authorization", format!("Bearer {api_key}"))
        .json(&serde_json::json!({
            "model": model,
            "prompt": prompt,
            "quality": quality,
            "size": size,
        }))
        .send()
        .await
        .map_err(|e| format!("submit failed: {e}"))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("parse response: {e}"))?;

    if let Some(err) = body.get("error") {
        return Err(format!("API error: {err}"));
    }

    let task_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "no task id in response".to_string())?;

    // 2. Poll for completion (max 3 minutes: 36 * 5s)
    let poll_url = format!("{api_url}/v4/async-result/{task_id}");
    let max_polls = 36;
    for _ in 0..max_polls {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        let poll_resp = client
            .get(&poll_url)
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
            .await
            .map_err(|e| format!("poll failed: {e}"))?;

        let poll_body: serde_json::Value = poll_resp
            .json()
            .await
            .map_err(|e| format!("poll parse: {e}"))?;

        let status = poll_body
            .get("task_status")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match status {
            "SUCCESS" => {
                // 3. Download video
                let video_url = poll_body
                    .get("video_result")
                    .or_else(|| poll_body.get("result"))
                    .and_then(|r| {
                        r.as_array()
                            .and_then(|a| a.first())
                            .and_then(|v| v.get("url").and_then(|u| u.as_str()))
                            .or_else(|| r.get("url").and_then(|u| u.as_str()))
                    });

                if let Some(url) = video_url {
                    let video_bytes = client
                        .get(url)
                        .send()
                        .await
                        .map_err(|e| format!("download: {e}"))?
                        .bytes()
                        .await
                        .map_err(|e| format!("read bytes: {e}"))?;

                    let path = format!("/tmp/cortex-video-{}.mp4", uuid::Uuid::now_v7());
                    tokio::fs::write(&path, &video_bytes)
                        .await
                        .map_err(|e| format!("save: {e}"))?;
                    return Ok(path);
                }
                return Err("Video generated but no URL in result".into());
            }
            "FAIL" => {
                let msg = poll_body
                    .get("error")
                    .map_or_else(|| "generation failed".into(), ToString::to_string);
                return Err(format!("Video generation failed: {msg}"));
            }
            _ => {} // PROCESSING -- keep polling
        }
    }

    Err("Video generation timed out (3 minutes)".into())
}
