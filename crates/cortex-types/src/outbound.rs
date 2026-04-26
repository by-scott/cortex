use serde::{Deserialize, Serialize};

use crate::{AuthContext, ClientId, DeliveryId, OwnedScope, SessionId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryPhase {
    Draft,
    Final,
    Correction,
    Notification,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryTextMode {
    Plain,
    Markdown,
    Html,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryStatus {
    Planned,
    Sent,
    Failed,
    Acknowledged,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Image,
    Audio,
    Video,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportCapabilities {
    pub text_mode: DeliveryTextMode,
    pub max_chars: usize,
    pub media: Vec<MediaKind>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutboundBlock {
    Text {
        text: String,
        markdown: bool,
    },
    Code {
        language: Option<String>,
        source: String,
    },
    Media {
        kind: MediaKind,
        label: String,
    },
    Diagnostic {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundMessage {
    pub id: DeliveryId,
    pub scope: OwnedScope,
    pub phase: DeliveryPhase,
    pub blocks: Vec<OutboundBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub correction_reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DeliveryItem {
    Text {
        text: String,
        markdown: bool,
        phase: DeliveryPhase,
    },
    Media {
        kind: MediaKind,
        label: String,
        phase: DeliveryPhase,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct DeliveryPlan {
    pub items: Vec<DeliveryItem>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboundDeliveryRecord {
    pub delivery_id: DeliveryId,
    pub session_id: SessionId,
    pub scope: OwnedScope,
    pub recipient_client_id: ClientId,
    pub plan: DeliveryPlan,
    pub status: DeliveryStatus,
    pub attempts: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

impl TransportCapabilities {
    #[must_use]
    pub const fn plain(max_chars: usize) -> Self {
        Self {
            text_mode: DeliveryTextMode::Plain,
            max_chars,
            media: Vec::new(),
        }
    }

    #[must_use]
    pub fn with_media(mut self, kind: MediaKind) -> Self {
        self.media.push(kind);
        self.media.sort();
        self.media.dedup();
        self
    }
}

impl OutboundMessage {
    #[must_use]
    pub fn new(scope: OwnedScope, phase: DeliveryPhase) -> Self {
        Self {
            id: DeliveryId::new(),
            scope,
            phase,
            blocks: Vec::new(),
            correction_reason: None,
        }
    }

    pub fn push(&mut self, block: OutboundBlock) {
        self.blocks.push(block);
    }

    #[must_use]
    pub fn source_text(&self) -> String {
        self.blocks
            .iter()
            .map(block_text)
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[must_use]
    pub fn plan(&self, capabilities: &TransportCapabilities) -> DeliveryPlan {
        let mut plan = DeliveryPlan::default();
        let mut text = String::new();
        let mut markdown = false;
        for block in &self.blocks {
            match block {
                OutboundBlock::Media { kind, label } if capabilities.media.contains(kind) => {
                    flush_text(&mut plan, &mut text, markdown, capabilities, self.phase);
                    markdown = false;
                    plan.items.push(DeliveryItem::Media {
                        kind: kind.clone(),
                        label: label.clone(),
                        phase: self.phase,
                    });
                }
                OutboundBlock::Media { kind, label } => {
                    append_line(
                        &mut text,
                        &format!("[{} unavailable] {label}", kind_label(kind)),
                    );
                }
                other => {
                    markdown |= block_markdown(other);
                    append_line(&mut text, &block_text(other));
                }
            }
        }
        flush_text(&mut plan, &mut text, markdown, capabilities, self.phase);
        plan
    }

    #[must_use]
    pub fn permits_final_after(&self, draft: &Self) -> bool {
        self.correction_reason.is_some()
            || self.source_text().chars().count() >= draft.source_text().chars().count()
    }
}

impl DeliveryPlan {
    #[must_use]
    pub fn combined_text(&self) -> String {
        self.items
            .iter()
            .filter_map(|item| match item {
                DeliveryItem::Text { text, .. } => Some(text.as_str()),
                DeliveryItem::Media { .. } => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}

impl OutboundDeliveryRecord {
    #[must_use]
    pub fn planned(
        delivery_id: DeliveryId,
        session_id: SessionId,
        recipient: &AuthContext,
        plan: DeliveryPlan,
    ) -> Self {
        Self {
            delivery_id,
            session_id,
            scope: OwnedScope::private_for(recipient),
            recipient_client_id: recipient.client_id.clone(),
            plan,
            status: DeliveryStatus::Planned,
            attempts: 0,
            last_error: None,
        }
    }

    pub fn mark_sent(&mut self) {
        self.status = DeliveryStatus::Sent;
        self.attempts = self.attempts.saturating_add(1);
        self.last_error = None;
    }

    pub fn mark_failed(&mut self, error: impl Into<String>) {
        self.status = DeliveryStatus::Failed;
        self.attempts = self.attempts.saturating_add(1);
        self.last_error = Some(error.into());
    }

    pub const fn acknowledge(&mut self) {
        self.status = DeliveryStatus::Acknowledged;
    }
}

fn flush_text(
    plan: &mut DeliveryPlan,
    text: &mut String,
    markdown: bool,
    capabilities: &TransportCapabilities,
    phase: DeliveryPhase,
) {
    let source = std::mem::take(text);
    for chunk in split_chars(&source, capabilities.max_chars.max(1)) {
        plan.items.push(DeliveryItem::Text {
            text: chunk,
            markdown: markdown && capabilities.text_mode != DeliveryTextMode::Plain,
            phase,
        });
    }
}

fn split_chars(text: &str, max_chars: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut chunks = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if current.chars().count() == max_chars {
            chunks.push(std::mem::take(&mut current));
        }
        current.push(character);
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

fn append_line(target: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !target.is_empty() {
        target.push('\n');
    }
    target.push_str(text);
}

fn block_text(block: &OutboundBlock) -> String {
    match block {
        OutboundBlock::Text { text, .. } => text.clone(),
        OutboundBlock::Code { language, source } => {
            let mut rendered = String::from("```");
            if let Some(language) = language {
                rendered.push_str(language);
            }
            rendered.push('\n');
            rendered.push_str(source);
            rendered.push_str("\n```");
            rendered
        }
        OutboundBlock::Media { kind, label } => format!("[{}] {label}", kind_label(kind)),
        OutboundBlock::Diagnostic { message } => message.clone(),
    }
}

const fn block_markdown(block: &OutboundBlock) -> bool {
    matches!(
        block,
        OutboundBlock::Text { markdown: true, .. } | OutboundBlock::Code { .. }
    )
}

const fn kind_label(kind: &MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Audio => "audio",
        MediaKind::Video => "video",
        MediaKind::File => "file",
    }
}
