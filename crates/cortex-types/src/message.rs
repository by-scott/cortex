use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Role {
    User,
    Assistant,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
    #[serde(rename = "image")]
    Image { media_type: String, data: String },
}

/// A multimedia attachment associated with a message.
///
/// Attachments represent external media (images, audio, video, files) that
/// accompany user or assistant messages. They can reference a local file
/// path or a remote URL.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// High-level type: `"image"`, `"audio"`, `"video"`, `"file"`.
    pub media_type: String,
    /// MIME type (e.g. `"image/jpeg"`, `"audio/ogg"`, `"video/mp4"`).
    pub mime_type: String,
    /// Local file path or remote URL.
    pub url: String,
    /// Optional caption or description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub caption: Option<String>,
    /// File size in bytes (if known).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TextFormat {
    #[default]
    Plain,
    Markdown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponsePart {
    Text { text: String, format: TextFormat },
    Media { attachment: Attachment },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssistantResponse {
    pub text: String,
    #[serde(default)]
    pub format: TextFormat,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<ResponsePart>,
}

impl AssistantResponse {
    #[must_use]
    pub fn plain_text(&self) -> String {
        if self.parts.is_empty() {
            return self.text.clone();
        }

        self.parts.iter().fold(String::new(), |mut acc, part| {
            match part {
                ResponsePart::Text { text, .. } => acc.push_str(text),
                ResponsePart::Media { attachment } => acc.push_str(media_placeholder(attachment)),
            }
            acc
        })
    }
}

fn media_placeholder(attachment: &Attachment) -> &'static str {
    match attachment.media_type.as_str() {
        "image" => "[image]",
        "audio" => "[audio]",
        "video" => "[video]",
        "file" => "[file]",
        _ => "[media]",
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: Vec<ContentBlock>,
    /// Multimedia attachments (images, audio, video, files).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
}

impl Message {
    #[must_use]
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            attachments: Vec::new(),
        }
    }

    #[must_use]
    pub fn assistant(text: impl Into<String>) -> Self {
        Self {
            role: Role::Assistant,
            content: vec![ContentBlock::Text { text: text.into() }],
            attachments: Vec::new(),
        }
    }

    #[must_use]
    pub fn user_with_images(text: impl Into<String>, images: Vec<(String, String)>) -> Self {
        let mut content = vec![ContentBlock::Text { text: text.into() }];
        for (media_type, data) in images {
            content.push(ContentBlock::Image { media_type, data });
        }
        Self {
            role: Role::User,
            content,
            attachments: Vec::new(),
        }
    }

    /// Create a user message with multimedia attachments.
    #[must_use]
    pub fn user_with_attachments(text: impl Into<String>, attachments: Vec<Attachment>) -> Self {
        Self {
            role: Role::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            attachments,
        }
    }

    #[must_use]
    pub fn has_images(&self) -> bool {
        self.content
            .iter()
            .any(|b| matches!(b, ContentBlock::Image { .. }))
    }

    /// Check if the message has any multimedia attachments.
    #[must_use]
    pub const fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    /// Get image attachments (`media_type == "image"`).
    #[must_use]
    pub fn image_attachments(&self) -> Vec<&Attachment> {
        self.attachments
            .iter()
            .filter(|a| a.media_type == "image")
            .collect()
    }

    #[must_use]
    pub fn has_tool_blocks(&self) -> bool {
        self.content.iter().any(|b| {
            matches!(
                b,
                ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
            )
        })
    }

    #[must_use]
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
