use cortex_types::{
    DeliveryItem, DeliveryPlan, DeliveryTextMode, MediaKind, TransportCapabilities,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportKind {
    Cli,
    Qq,
    Telegram,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportPacket {
    pub text: Option<String>,
    pub media_label: Option<String>,
    pub markdown: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportAdapter {
    kind: TransportKind,
    capabilities: TransportCapabilities,
}

impl TransportAdapter {
    #[must_use]
    pub fn telegram() -> Self {
        Self {
            kind: TransportKind::Telegram,
            capabilities: TransportCapabilities {
                text_mode: DeliveryTextMode::Markdown,
                max_chars: 4_096,
                media: vec![MediaKind::Image, MediaKind::Audio, MediaKind::File],
            },
        }
    }

    #[must_use]
    pub fn qq() -> Self {
        Self {
            kind: TransportKind::Qq,
            capabilities: TransportCapabilities::plain(2_000)
                .with_media(MediaKind::Image)
                .with_media(MediaKind::File),
        }
    }

    #[must_use]
    pub const fn cli() -> Self {
        Self {
            kind: TransportKind::Cli,
            capabilities: TransportCapabilities {
                text_mode: DeliveryTextMode::Markdown,
                max_chars: 8_000,
                media: Vec::new(),
            },
        }
    }

    #[must_use]
    pub const fn capabilities(&self) -> &TransportCapabilities {
        &self.capabilities
    }

    #[must_use]
    pub const fn kind(&self) -> TransportKind {
        self.kind
    }

    #[must_use]
    pub fn render(&self, plan: &DeliveryPlan) -> Vec<TransportPacket> {
        plan.items
            .iter()
            .map(|item| self.render_item(item))
            .collect()
    }

    fn render_item(&self, item: &DeliveryItem) -> TransportPacket {
        match item {
            DeliveryItem::Text { text, markdown, .. } => match self.capabilities.text_mode {
                DeliveryTextMode::Plain => TransportPacket {
                    text: Some(markdown_to_plain(text)),
                    media_label: None,
                    markdown: false,
                },
                DeliveryTextMode::Markdown => TransportPacket {
                    text: Some(text.clone()),
                    media_label: None,
                    markdown: *markdown,
                },
                DeliveryTextMode::Html => TransportPacket {
                    text: Some(markdown_to_html(text, *markdown)),
                    media_label: None,
                    markdown: false,
                },
            },
            DeliveryItem::Media { kind, label, .. } => {
                if self.capabilities.media.contains(kind) {
                    TransportPacket {
                        text: None,
                        media_label: Some(label.clone()),
                        markdown: false,
                    }
                } else {
                    TransportPacket {
                        text: Some(format!("[{}] {label}", media_label(kind))),
                        media_label: None,
                        markdown: false,
                    }
                }
            }
        }
    }
}

fn markdown_to_plain(text: &str) -> String {
    let mut plain = String::new();
    let mut chars = text.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '[' => {
                let label = take_until(&mut chars, ']');
                if chars.next_if_eq(&'(').is_some() {
                    let url = take_until(&mut chars, ')');
                    plain.push_str(&label);
                    if !url.is_empty() {
                        plain.push_str(" (");
                        plain.push_str(&url);
                        plain.push(')');
                    }
                } else {
                    plain.push('[');
                    plain.push_str(&label);
                }
            }
            '*' | '_' | '`' => {}
            '#' if plain.ends_with('\n') || plain.is_empty() => {
                while chars.next_if_eq(&'#').is_some() {}
                let _ = chars.next_if_eq(&' ');
            }
            other => plain.push(other),
        }
    }
    plain
}

fn markdown_to_html(text: &str, markdown: bool) -> String {
    if markdown {
        escape_html(&markdown_to_plain(text))
    } else {
        escape_html(text)
    }
}

fn take_until(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, terminator: char) -> String {
    let mut output = String::new();
    for character in chars.by_ref() {
        if character == terminator {
            break;
        }
        output.push(character);
    }
    output
}

fn escape_html(text: &str) -> String {
    let mut escaped = String::new();
    for character in text.chars() {
        match character {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            other => escaped.push(other),
        }
    }
    escaped
}

const fn media_label(kind: &MediaKind) -> &'static str {
    match kind {
        MediaKind::Image => "image",
        MediaKind::Audio => "audio",
        MediaKind::Video => "video",
        MediaKind::File => "file",
    }
}
