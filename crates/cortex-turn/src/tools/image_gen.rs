//! Image generation tool -- thin wrapper dispatching to the configured provider.

use base64::Engine;
use cortex_types::config::MediaConfig;

use super::{Tool, ToolError, ToolResult, block_on_tool_future};

pub struct ImageGenTool {
    config: MediaConfig,
    api_key: String,
}

impl ImageGenTool {
    #[must_use]
    pub const fn new(config: MediaConfig, api_key: String) -> Self {
        Self { config, api_key }
    }
}

impl Tool for ImageGenTool {
    fn name(&self) -> &'static str {
        "image_gen"
    }

    fn description(&self) -> &'static str {
        "Generate an image from a text description.\n\n\
         Use when the user asks to create, draw, or generate an image. \
         Returns the file path of the generated image.\n\n\
         Not available if no image generation backend is configured."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "Detailed description of the image to generate."
                },
                "size": {
                    "type": "string",
                    "description": "Image size: 1024x1024, 1792x1024, or 1024x1792",
                    "default": "1024x1024"
                }
            },
            "required": ["prompt"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        if self.config.image_gen.is_empty() {
            return Ok(ToolResult::error(
                "Image generation not configured. Set [media].image_gen in config.toml",
            ));
        }
        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing prompt".into()))?;
        let size = input
            .get("size")
            .and_then(|v| v.as_str())
            .unwrap_or("1024x1024");

        let provider_default_url = match self.config.image_gen.as_str() {
            "openai" => "https://api.openai.com",
            _ => "https://open.bigmodel.cn/api/paas",
        };
        let default_model = match self.config.image_gen.as_str() {
            "openai" => "dall-e-3",
            _ => "cogview-3-plus",
        };

        block_on_tool_future(async {
            let client = reqwest::Client::new();
            let base = self.config.effective_api_url(provider_default_url);
            let url = format!("{}/v1/images/generations", base.trim_end_matches('/'));
            let model = if self.config.image_gen_model.is_empty() {
                default_model
            } else {
                &self.config.image_gen_model
            };

            let resp = client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&serde_json::json!({
                    "model": model,
                    "prompt": prompt,
                    "size": size,
                    "n": 1,
                    "response_format": "b64_json",
                }))
                .send()
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            if let Some(b64) = json
                .get("data")
                .and_then(|d| d.get(0))
                .and_then(|item| item.get("b64_json"))
                .and_then(|v| v.as_str())
            {
                let bytes = base64::engine::general_purpose::STANDARD
                    .decode(b64)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                let path = format!("/tmp/cortex-img-{}.png", uuid::Uuid::now_v7());
                std::fs::write(&path, &bytes)
                    .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
                Ok(ToolResult::success(format!("[image:{path}]")))
            } else {
                let err = json
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("No image data in response");
                Ok(ToolResult::error(err))
            }
        })
    }
}
