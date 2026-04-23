use super::{Tool, ToolError, ToolResult};

pub struct SendMediaTool;

impl Tool for SendMediaTool {
    fn name(&self) -> &'static str {
        "send_media"
    }

    fn description(&self) -> &'static str {
        "Send an existing media file or document to the user. Use this when the \
         user asks to send, share, upload, or deliver an existing image, audio, \
         video, or file. This tool only declares structured media; Cortex \
         runtime delivers it through the active client channel."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Local filesystem path or remote URL of the media to send."
                },
                "media_type": {
                    "type": "string",
                    "enum": ["image", "audio", "video", "file"],
                    "description": "Optional explicit media type. If omitted, Cortex infers it from MIME type or extension."
                },
                "mime_type": {
                    "type": "string",
                    "description": "Optional MIME type such as image/png, audio/mpeg, video/mp4, or application/pdf."
                },
                "caption": {
                    "type": "string",
                    "description": "Optional short caption for the media."
                }
            },
            "required": ["path"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let path = input
            .get("path")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| ToolError::InvalidInput("path required".into()))?;

        if !is_remote_url(path) && !std::path::Path::new(path).is_file() {
            return Ok(ToolResult::error(format!("media file not found: {path}")));
        }

        let file_name = std::path::Path::new(path)
            .file_name()
            .and_then(|name| name.to_str());
        let explicit_mime = input
            .get("mime_type")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty());
        let media_type = input
            .get("media_type")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| {
                super::infer_media_type(explicit_mime.unwrap_or_default(), file_name)
            });
        let mime_type =
            explicit_mime.unwrap_or_else(|| super::infer_mime_type(media_type, file_name));
        let caption = input
            .get("caption")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let mut attachment = super::attachment_from_path(media_type, mime_type, path);
        attachment.caption = caption;

        Ok(ToolResult::success(format!("Prepared {media_type}: {path}")).with_media(attachment))
    }
}

fn is_remote_url(value: &str) -> bool {
    value.starts_with("http://") || value.starts_with("https://")
}
