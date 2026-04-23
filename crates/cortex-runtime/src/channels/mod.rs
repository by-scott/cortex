//! Message channel framework for external messaging platforms.
//!
//! Channels run inside the daemon process, sharing `DaemonState` directly.
//! The daemon starts/stops them alongside other transports (HTTP, socket, stdio).

pub mod pairing;
pub mod qq;
pub mod store;
pub mod telegram;
pub mod whatsapp;

use std::path::Path;
use std::sync::Arc;

use crate::command_registry::{CommandInvocation, DefaultCommandRegistry, SessionCommand};
use cortex_types::{Attachment, ResponsePart, TextFormat};
use pairing::PairingAction;
use store::ChannelStore;

use crate::daemon::DaemonState;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChannelTextCapability {
    Plain,
    Markdown,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ChannelCapabilities {
    pub text: ChannelTextCapability,
    media_mask: u8,
}

impl ChannelCapabilities {
    pub(crate) const IMAGE: u8 = 1 << 0;
    pub(crate) const AUDIO: u8 = 1 << 1;
    pub(crate) const VIDEO: u8 = 1 << 2;
    pub(crate) const FILE: u8 = 1 << 3;

    pub(crate) const fn text_only(text: ChannelTextCapability) -> Self {
        Self {
            text,
            media_mask: 0,
        }
    }

    pub(crate) const fn with_media(text: ChannelTextCapability, media_mask: u8) -> Self {
        Self { text, media_mask }
    }

    pub(crate) fn supports(self, attachment: &Attachment) -> bool {
        let flag = match attachment.media_type.as_str() {
            "image" => Self::IMAGE,
            "audio" => Self::AUDIO,
            "video" => Self::VIDEO,
            "file" => Self::FILE,
            _ => 0,
        };
        flag != 0 && (self.media_mask & flag) != 0
    }
}

#[derive(Debug, Clone)]
pub(crate) enum ChannelDeliveryItem {
    Text { text: String, markdown: bool },
    Media { attachment: Attachment },
}

pub enum ChannelSlashAction {
    Reply(String),
    RunPrompt { session_id: String, prompt: String },
}

fn reply_events(text: String) -> Vec<crate::daemon::BroadcastEvent> {
    if text.is_empty() {
        Vec::new()
    } else {
        vec![crate::daemon::BroadcastEvent::done(
            text.clone(),
            vec![ResponsePart::Text {
                text,
                format: TextFormat::Markdown,
            }],
        )]
    }
}

pub(crate) fn infer_attachment_media_type(mime_type: &str, file_name: Option<&str>) -> String {
    let mime = mime_type.to_ascii_lowercase();
    if mime.starts_with("image/") {
        return "image".into();
    }
    if mime == "voice" || mime.starts_with("audio/") {
        return "audio".into();
    }
    if mime.starts_with("video/") {
        return "video".into();
    }
    if let Some(name) = file_name {
        let lower = name.to_ascii_lowercase();
        if [".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "image".into();
        }
        if [".ogg", ".mp3", ".wav", ".m4a", ".aac", ".opus"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "audio".into();
        }
        if [".mp4", ".mov", ".mkv", ".webm", ".avi"]
            .iter()
            .any(|ext| lower.ends_with(ext))
        {
            return "video".into();
        }
    }
    "file".into()
}

pub(crate) fn default_prompt_for_attachments(attachments: &[Attachment]) -> String {
    let has = |kind: &str| attachments.iter().any(|a| a.media_type == kind);
    if has("image") {
        "The previous user message is an image attachment. Describe what you see in the image."
            .into()
    } else if has("video") {
        "The user sent a video. Describe the content.".into()
    } else if has("audio") {
        "The user sent an audio message. Transcribe or summarize it.".into()
    } else {
        "The user sent a file. Identify it and help with it.".into()
    }
}

pub(crate) fn resolve_effective_inbound_text(text: &str, attachments: &[Attachment]) -> String {
    let mut prefix = String::new();
    for attachment in attachments {
        let Some(caption) = attachment.caption.as_deref().map(str::trim) else {
            continue;
        };
        if caption.is_empty() {
            continue;
        }
        let label = match attachment.media_type.as_str() {
            "image" => "[Image analysis] ",
            "video" => "[Video analysis] ",
            "audio" => "[Audio transcript] ",
            "file" => "[File note] ",
            _ => "[Attachment] ",
        };
        prefix.push_str(label);
        prefix.push_str(caption);
        prefix.push('\n');
    }

    let trimmed = text.trim();
    if !trimmed.is_empty() {
        if prefix.is_empty() {
            trimmed.to_string()
        } else {
            format!("{prefix}{trimmed}")
        }
    } else if !prefix.is_empty() {
        prefix.trim_end().to_string()
    } else if attachments.is_empty() {
        String::new()
    } else {
        default_prompt_for_attachments(attachments)
    }
}

pub(crate) async fn enrich_inbound_attachment(
    state: &Arc<DaemonState>,
    client: &reqwest::Client,
    mut attachment: Attachment,
) -> Attachment {
    let (media_config, api_key) = {
        let cfg = state.config();
        let media_config = cfg.media.clone();
        let api_key = media_config.effective_api_key(&cfg.api.api_key).to_string();
        drop(cfg);
        (media_config, api_key)
    };

    match attachment.media_type.as_str() {
        "audio" => {
            let existing = attachment.caption.clone();
            let stt_key = media_config.stt_key(&api_key).to_string();
            if let Ok(transcript) =
                crate::media::stt::transcribe(&media_config, &stt_key, &attachment.url, client)
                    .await
            {
                if transcript.trim().is_empty() {
                    attachment.caption = existing;
                } else {
                    attachment.caption = Some(transcript);
                }
            }
        }
        "image"
            if attachment
                .caption
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
                && !state.supports_direct_image_input()
                && !media_config.image_understand.is_empty() =>
        {
            let image_key = media_config.image_understand_key(&api_key).to_string();
            if let Ok(summary) = crate::media::image_understand::understand(
                &media_config,
                &image_key,
                &attachment.url,
                "Describe the content of this image.",
                client,
            )
            .await
                && !summary.trim().is_empty()
            {
                attachment.caption = Some(summary);
            }
        }
        "video"
            if attachment
                .caption
                .as_deref()
                .unwrap_or_default()
                .trim()
                .is_empty()
                && !media_config.video_understand.is_empty() =>
        {
            let video_key = media_config.video_understand_key(&api_key).to_string();
            if let Ok(summary) = crate::media::video_understand::understand(
                &media_config,
                &video_key,
                &attachment.url,
                "Describe the content of this video.",
                client,
            )
            .await
                && !summary.trim().is_empty()
            {
                attachment.caption = Some(summary);
            }
        }
        _ => {}
    }

    attachment
}

pub fn resolve_channel_slash(
    state: &Arc<DaemonState>,
    actor: &str,
    text: &str,
) -> ChannelSlashAction {
    let session_id = state.resolve_actor_session(actor);
    let trimmed = text.trim();
    let registry = DefaultCommandRegistry::new();
    match registry.classify(trimmed) {
        CommandInvocation::Builtin(crate::command_registry::ParsedCommand {
            kind: crate::command_registry::CommandKind::Session(SessionCommand::New),
            ..
        }) => {
            let (sid_str, _) = state.create_session_for_actor(actor);
            return ChannelSlashAction::Reply(format!("New session: {sid_str}"));
        }
        CommandInvocation::Builtin(crate::command_registry::ParsedCommand {
            kind: crate::command_registry::CommandKind::Session(SessionCommand::Switch { target }),
            ..
        }) => {
            if !state.actor_can_access_session(actor, target) {
                return ChannelSlashAction::Reply("You can only access your own sessions.".into());
            }
            state.set_actor_session(actor, target);
            return ChannelSlashAction::Reply(format!("Switched to session: {target}"));
        }
        CommandInvocation::Builtin(crate::command_registry::ParsedCommand {
            kind: crate::command_registry::CommandKind::Lifecycle,
            ..
        }) => {
            state.clear_actor_session(actor);
            return ChannelSlashAction::Reply(
                "Session ended. Send a message to start fresh.".into(),
            );
        }
        _ => {}
    }

    match state.resolve_slash_command_for_session(Some(&session_id), text) {
        crate::daemon::SlashCommandAction::Output(text) => ChannelSlashAction::Reply(text),
        crate::daemon::SlashCommandAction::Prompt(prompt) => {
            match state.inject_message(&session_id, prompt.clone()) {
                crate::daemon::InjectMessageResult::Accepted => ChannelSlashAction::Reply(format!(
                    "Command injected into running turn: {trimmed}"
                )),
                crate::daemon::InjectMessageResult::InputClosed
                | crate::daemon::InjectMessageResult::NoActiveTurn => {
                    ChannelSlashAction::RunPrompt { session_id, prompt }
                }
            }
        }
        crate::daemon::SlashCommandAction::NotFound(msg) => ChannelSlashAction::Reply(msg),
    }
}

/// Process an inbound message through pairing + Cortex execution.
///
/// Uses streaming execution so trace events (tool, meta, etc.) are captured
/// and included in the response, matching the behavior of Web and CLI.
/// Session state is tracked per canonical actor in the daemon so multiple
/// linked channel identities can share one user's session set.
pub fn handle_message(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    user_id: &str,
    user_name: &str,
    text: &str,
    session_prefix: &str,
) -> String {
    render_channel_events(&handle_message_events(
        state,
        store,
        user_id,
        user_name,
        text,
        &[],
        session_prefix,
    ))
}

pub fn handle_message_events(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    user_id: &str,
    user_name: &str,
    text: &str,
    attachments: &[Attachment],
    session_prefix: &str,
) -> Vec<crate::daemon::BroadcastEvent> {
    // Check pairing
    match pairing::check_user(store, user_id, user_name, session_prefix) {
        PairingAction::Allowed => {}
        PairingAction::SendPairingPrompt(msg) => return reply_events(msg),
        PairingAction::Denied => return Vec::new(),
    }

    let actor = DaemonState::channel_actor(session_prefix, user_id);
    let session_id = state.resolve_actor_session(&actor);
    let effective_text = resolve_effective_inbound_text(text, attachments);

    if effective_text.starts_with('/') {
        match resolve_channel_slash(state, &actor, &effective_text) {
            ChannelSlashAction::Reply(text) => return reply_events(text),
            ChannelSlashAction::RunPrompt { session_id, prompt } => {
                return run_single_turn_events(
                    state,
                    &session_id,
                    &prompt,
                    attachments,
                    session_prefix,
                );
            }
        }
    }

    // If a turn is already running, inject the message mid-turn so the LLM
    // sees it in the next TPN iteration.
    match state.inject_message(&session_id, effective_text.clone()) {
        crate::daemon::InjectMessageResult::Accepted => {
            return reply_events(
                "Message received. It has been injected into the running turn and will be handled after the current execution step finishes.".into(),
            );
        }
        crate::daemon::InjectMessageResult::InputClosed => {
            return reply_events(
                "The current turn is already finalizing; a new turn will be started for this message.".into(),
            );
        }
        crate::daemon::InjectMessageResult::NoActiveTurn => {}
    }

    run_single_turn_events(
        state,
        &session_id,
        &effective_text,
        attachments,
        session_prefix,
    )
}

fn run_single_turn_events(
    state: &Arc<DaemonState>,
    session_id: &str,
    text: &str,
    attachments: &[Attachment],
    source: &str,
) -> Vec<crate::daemon::BroadcastEvent> {
    let turn_attachments: Vec<Attachment> = attachments
        .iter()
        .filter(|attachment| {
            attachment.media_type != "image" || state.supports_direct_image_input()
        })
        .cloned()
        .collect();
    let tracer = crate::daemon::TracingTurnTracer {
        config: state.config().turn.trace.clone(),
    };
    let turn_input = crate::turn_executor::TurnInput {
        text,
        attachments: &turn_attachments,
        inline_images: &[],
    };
    let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let events_for_turn = Arc::clone(&events);
    let (result, mut collected) = {
        let foreground = state.begin_foreground_execution();
        let result = state.execute_foreground_turn_streaming(
            &foreground,
            session_id,
            &turn_input,
            source,
            move |event| {
                if let Some(event) = crate::daemon::BroadcastEvent::from_turn_stream_event(event) {
                    match event {
                        crate::daemon::BroadcastEvent::Text(_) => {}
                        other => {
                            events_for_turn
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .push(other);
                        }
                    }
                }
            },
            &tracer,
        );
        drop(foreground);
        let collected = events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        (result, collected)
    };
    match result {
        Ok(output) => collected.push(crate::daemon::BroadcastEvent::done(
            output.response_text.clone().unwrap_or_default(),
            output.response_parts,
        )),
        Err(error) => collected.push(crate::daemon::BroadcastEvent::Error(error)),
    }
    collected
}

#[must_use]
pub fn render_channel_events(events: &[crate::daemon::BroadcastEvent]) -> String {
    let rendered: Vec<String> = events
        .iter()
        .flat_map(|event| {
            channel_delivery_items(
                event,
                ChannelCapabilities::text_only(ChannelTextCapability::Plain),
            )
        })
        .filter_map(|item| match item {
            ChannelDeliveryItem::Text { text, .. } if !text.trim().is_empty() => Some(text),
            _ => None,
        })
        .filter(|text| !text.trim().is_empty())
        .collect();
    rendered.join("\n")
}

pub(crate) fn channel_delivery_items(
    event: &crate::daemon::BroadcastEvent,
    capabilities: ChannelCapabilities,
) -> Vec<ChannelDeliveryItem> {
    match event {
        crate::daemon::BroadcastEvent::Done {
            response,
            response_parts,
        } => {
            if response_parts.is_empty() {
                return vec![ChannelDeliveryItem::Text {
                    text: response.clone(),
                    markdown: capabilities.text == ChannelTextCapability::Markdown,
                }];
            }

            response_parts
                .iter()
                .cloned()
                .filter_map(|part| channel_delivery_item(part, capabilities))
                .collect()
        }
        crate::daemon::BroadcastEvent::Text(text) => vec![ChannelDeliveryItem::Text {
            text: text.clone(),
            markdown: false,
        }],
        crate::daemon::BroadcastEvent::Boundary => Vec::new(),
        crate::daemon::BroadcastEvent::Observer { .. }
        | crate::daemon::BroadcastEvent::Trace { .. }
        | crate::daemon::BroadcastEvent::Error(_)
        | crate::daemon::BroadcastEvent::PermissionRequested(_) => {
            vec![ChannelDeliveryItem::Text {
                text: event.plain_text(),
                markdown: false,
            }]
        }
    }
}

fn channel_delivery_item(
    part: ResponsePart,
    capabilities: ChannelCapabilities,
) -> Option<ChannelDeliveryItem> {
    match part {
        ResponsePart::Text { text, format } if !text.trim().is_empty() => {
            Some(ChannelDeliveryItem::Text {
                text,
                markdown: capabilities.text == ChannelTextCapability::Markdown
                    && matches!(format, TextFormat::Markdown),
            })
        }
        ResponsePart::Text { .. } => None,
        ResponsePart::Media { attachment } => {
            if capabilities.supports(&attachment) {
                Some(ChannelDeliveryItem::Media { attachment })
            } else {
                Some(media_fallback_text(&attachment))
            }
        }
    }
}

fn media_fallback_text(attachment: &Attachment) -> ChannelDeliveryItem {
    let mut text = attachment_placeholder(attachment).to_string();
    if let Some(caption) = attachment.caption.as_deref().map(str::trim)
        && !caption.is_empty()
    {
        text.push('\n');
        text.push_str(caption);
    }
    ChannelDeliveryItem::Text {
        text,
        markdown: false,
    }
}

fn attachment_placeholder(attachment: &Attachment) -> &'static str {
    match attachment.media_type.as_str() {
        "image" => "[image]",
        "audio" => "[audio]",
        "video" => "[video]",
        "file" => "[file]",
        _ => "[media]",
    }
}

/// Split a message into chunks respecting a platform's character limit.
#[must_use]
pub fn split_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < text.len() {
        let mut end = (start + max_len).min(text.len());
        if end < text.len() {
            if let Some(nl) = text[start..end].rfind('\n') {
                end = start + nl + 1;
            } else {
                while end > start && !text.is_char_boundary(end) {
                    end -= 1;
                }
            }
        }
        if end == start {
            end = start + 1;
        }
        chunks.push(text[start..end].to_string());
        start = end;
    }
    chunks
}

/// Read channel auth credentials from `channels/<platform>/auth.json`.
#[must_use]
pub fn read_channel_auth(home: &Path, platform: &str) -> Option<serde_json::Value> {
    let path = cortex_kernel::ChannelFileSet::from_instance_home(home, platform).auth;
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

/// Save channel auth credentials to `channels/<platform>/auth.json`.
pub fn save_channel_auth(home: &Path, platform: &str, auth: &serde_json::Value) {
    let files = cortex_kernel::ChannelFileSet::from_instance_home(home, platform);
    let _ = std::fs::create_dir_all(&files.dir);
    if let Ok(json) = serde_json::to_string_pretty(auth) {
        let _ = std::fs::write(files.auth, json);
    }
}
