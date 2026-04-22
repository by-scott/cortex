//! Video generation tool -- thin wrapper dispatching to the configured provider.

use cortex_types::config::MediaConfig;

use super::{Tool, ToolError, ToolResult, block_on_tool_future};

pub struct VideoGenTool {
    config: MediaConfig,
    api_key: String,
}

impl VideoGenTool {
    #[must_use]
    pub const fn new(config: MediaConfig, api_key: String) -> Self {
        Self { config, api_key }
    }
}

impl Tool for VideoGenTool {
    fn name(&self) -> &'static str {
        "video_gen"
    }

    fn description(&self) -> &'static str {
        "Generate a video from a text description.\n\n\
         Use when the user asks to create, generate, or produce a video. \
         Video generation takes 30-120 seconds. Returns the file path of \
         the generated video.\n\n\
         Not available if no video generation backend is configured."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Detailed description of the video to generate."
                },
                "size": {
                    "type": "string",
                    "description": "Video resolution: 1920x1080, 1280x720, 1024x1024",
                    "default": "1920x1080"
                },
                "quality": {
                    "type": "string",
                    "enum": ["quality", "speed"],
                    "default": "speed",
                    "description": "Optimize for quality or speed."
                }
            },
            "required": ["prompt"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        if self.config.video_gen.is_empty() {
            return Ok(ToolResult::error(
                "Video generation not configured. Set [media].video_gen in config.toml",
            ));
        }

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing prompt".into()))?;
        let size = input
            .get("size")
            .and_then(|v| v.as_str())
            .unwrap_or("1920x1080");
        let quality = input
            .get("quality")
            .and_then(|v| v.as_str())
            .unwrap_or("speed");

        block_on_tool_future(generate_video(
            &self.config,
            &self.api_key,
            prompt,
            size,
            quality,
        ))
    }
}

/// Async video generation: submit -> poll -> download.
async fn generate_video(
    config: &MediaConfig,
    api_key: &str,
    prompt: &str,
    size: &str,
    quality: &str,
) -> Result<ToolResult, ToolError> {
    let client = reqwest::Client::new();
    let provider_default = "https://open.bigmodel.cn/api/paas";
    let api_url = config
        .effective_api_url(provider_default)
        .trim_end_matches('/');
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
        .map_err(|e| ToolError::ExecutionFailed(format!("submit failed: {e}")))?;

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("parse response: {e}")))?;

    if let Some(err) = body.get("error") {
        return Ok(ToolResult::error(format!("API error: {err}")));
    }

    let task_id = body
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolError::ExecutionFailed("no task id in response".into()))?;

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
            .map_err(|e| ToolError::ExecutionFailed(format!("poll failed: {e}")))?;

        let poll_body: serde_json::Value = poll_resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("poll parse: {e}")))?;

        let status = poll_body
            .get("task_status")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        match status {
            "SUCCESS" => {
                // 3. Download video -- ZAI returns video_result or result with url
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
                        .map_err(|e| ToolError::ExecutionFailed(format!("download: {e}")))?
                        .bytes()
                        .await
                        .map_err(|e| ToolError::ExecutionFailed(format!("read bytes: {e}")))?;

                    let path = format!("/tmp/cortex-video-{}.mp4", uuid::Uuid::now_v7());
                    std::fs::write(&path, &video_bytes)
                        .map_err(|e| ToolError::ExecutionFailed(format!("save: {e}")))?;

                    return Ok(
                        ToolResult::success(format!("Generated video: {path}")).with_media(
                            crate::tools::attachment_from_path("video", "video/mp4", &path),
                        ),
                    );
                }
                return Ok(ToolResult::error("Video generated but no URL in result"));
            }
            "FAIL" => {
                let msg = poll_body
                    .get("error")
                    .map_or_else(|| "generation failed".into(), ToString::to_string);
                return Ok(ToolResult::error(format!("Video generation failed: {msg}")));
            }
            _ => {} // PROCESSING -- keep polling
        }
    }

    Ok(ToolResult::error("Video generation timed out (3 minutes)"))
}
