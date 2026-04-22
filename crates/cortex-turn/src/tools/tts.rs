//! Text-to-speech tool -- thin wrapper dispatching to the configured TTS provider.

use cortex_types::config::MediaConfig;

use super::{Tool, ToolError, ToolResult, block_on_tool_future};

pub struct TtsTool {
    config: MediaConfig,
    api_key: String,
}

impl TtsTool {
    #[must_use]
    pub const fn new(config: MediaConfig, api_key: String) -> Self {
        Self { config, api_key }
    }
}

impl Tool for TtsTool {
    fn name(&self) -> &'static str {
        "tts"
    }

    fn description(&self) -> &'static str {
        "Convert text to speech audio.\n\n\
         Use when the user asks to hear something spoken, wants an audio response, \
         or requests voice output. Generates an audio file from text.\n\n\
         The audio file path is returned for delivery to the user's platform."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to convert to speech."
                },
                "voice": {
                    "type": "string",
                    "description": "Voice name (optional, uses config default)."
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input
            .get("text")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        let voice = input
            .get("voice")
            .and_then(|v| v.as_str())
            .unwrap_or(&self.config.tts_voice);

        match self.config.tts.as_str() {
            "" => Err(ToolError::ExecutionFailed(
                "TTS is not configured. Set [media].tts to \"edge\", \"openai\", or \"zai\"."
                    .into(),
            )),
            "openai" => execute_api_tts(
                text,
                voice,
                &self.api_key,
                &self.config,
                "https://api.openai.com",
            ),
            "zai" => execute_api_tts(
                text,
                voice,
                &self.api_key,
                &self.config,
                "https://open.bigmodel.cn/api/paas",
            ),
            _ => execute_edge_tts(text, voice),
        }
    }
}

fn execute_edge_tts(text: &str, voice: &str) -> Result<ToolResult, ToolError> {
    let output_path = format!("/tmp/cortex-tts-{}.mp3", uuid::Uuid::now_v7());
    let result = std::process::Command::new("edge-tts")
        .args([
            "--voice",
            voice,
            "--text",
            text,
            "--write-media",
            &output_path,
        ])
        .output()
        .map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "edge-tts is not installed or not on PATH: {e}. Install edge-tts or set [media].tts to an API provider."
            ))
        })?;

    if result.status.success() {
        Ok(
            ToolResult::success(format!("Generated audio: {output_path}")).with_media(
                crate::tools::attachment_from_path("audio", "audio/mpeg", &output_path),
            ),
        )
    } else {
        Ok(ToolResult::error(
            String::from_utf8_lossy(&result.stderr).to_string(),
        ))
    }
}

fn execute_api_tts(
    text: &str,
    voice: &str,
    api_key: &str,
    config: &MediaConfig,
    provider_default_url: &str,
) -> Result<ToolResult, ToolError> {
    block_on_tool_future(async {
        let url = openai_compat_endpoint(config.tts_url(provider_default_url), "audio/speech");

        let client = reqwest::Client::new();
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
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let bytes = resp
            .bytes()
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let output_path = format!("/tmp/cortex-tts-{}.mp3", uuid::Uuid::now_v7());
        std::fs::write(&output_path, &bytes)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        Ok(
            ToolResult::success(format!("Generated audio: {output_path}")).with_media(
                crate::tools::attachment_from_path("audio", "audio/mpeg", &output_path),
            ),
        )
    })
}

fn openai_compat_endpoint(base_url: &str, path: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.ends_with("/v4") {
        format!("{base}/{path}")
    } else {
        format!("{base}/v1/{path}")
    }
}
