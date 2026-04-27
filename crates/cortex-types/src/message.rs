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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
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
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub media_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub sha256: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_actor: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub source_uri: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub taint: MediaTaint,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_vision_confidence: Option<u8>,
    #[serde(default)]
    pub external_recipient_policy: MediaExternalPolicy,
    #[serde(default)]
    pub memory_policy: MediaMemoryPolicy,
    #[serde(default)]
    pub publish_policy: MediaPublishPolicy,
}

impl Attachment {
    #[must_use]
    pub fn new(
        media_type: impl Into<String>,
        mime_type: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        let url = url.into();
        Self {
            media_type: media_type.into(),
            mime_type: mime_type.into(),
            source_uri: url.clone(),
            url,
            ..Self::default()
        }
    }

    #[must_use]
    pub fn with_caption(mut self, caption: impl Into<String>) -> Self {
        self.caption = Some(caption.into());
        self
    }

    #[must_use]
    pub const fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    #[must_use]
    pub fn with_media_id(mut self, media_id: impl Into<String>) -> Self {
        self.media_id = media_id.into();
        self
    }

    #[must_use]
    pub fn with_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = sha256.into();
        self
    }

    #[must_use]
    pub fn with_source_actor(mut self, actor: impl Into<String>) -> Self {
        self.source_actor = actor.into();
        self
    }

    #[must_use]
    pub fn with_source_uri(mut self, uri: impl Into<String>) -> Self {
        self.source_uri = uri.into();
        self
    }

    #[must_use]
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    #[must_use]
    pub const fn with_taint(mut self, taint: MediaTaint) -> Self {
        self.taint = taint;
        self
    }

    #[must_use]
    pub const fn with_vision_confidence(mut self, confidence: u8) -> Self {
        self.derived_vision_confidence = Some(if confidence > 100 { 100 } else { confidence });
        self
    }

    #[must_use]
    pub const fn with_external_policy(mut self, policy: MediaExternalPolicy) -> Self {
        self.external_recipient_policy = policy;
        self
    }

    #[must_use]
    pub const fn with_memory_policy(mut self, policy: MediaMemoryPolicy) -> Self {
        self.memory_policy = policy;
        self
    }

    #[must_use]
    pub const fn with_publish_policy(mut self, policy: MediaPublishPolicy) -> Self {
        self.publish_policy = policy;
        self
    }

    #[must_use]
    pub const fn allows_external_recipient(&self, same_actor: bool) -> bool {
        match self.external_recipient_policy {
            MediaExternalPolicy::Blocked => false,
            MediaExternalPolicy::SameActorOnly => same_actor,
            MediaExternalPolicy::Allowed => true,
        }
    }

    #[must_use]
    pub const fn may_enter_durable_memory(&self) -> bool {
        matches!(self.memory_policy, MediaMemoryPolicy::Allowed)
    }

    #[must_use]
    pub const fn may_publish(&self) -> bool {
        matches!(self.publish_policy, MediaPublishPolicy::Allowed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaTaint {
    Trusted,
    UserProvided,
    Generated,
    External,
    Hostile,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaExternalPolicy {
    Blocked,
    #[default]
    SameActorOnly,
    Allowed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaMemoryPolicy {
    #[default]
    RequiresExplicitConsent,
    Allowed,
    Blocked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum MediaPublishPolicy {
    #[default]
    RequiresExplicitApproval,
    Allowed,
    Blocked,
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
    Media { attachment: Box<Attachment> },
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
