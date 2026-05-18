//! Official QQ Bot channel via Tencent QQ Bot API.

use std::sync::Arc;
use std::time::Instant;

use sha2::Digest;
use tokio::sync::Mutex;
use tokio::sync::watch;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

mod delivery;
mod gateway;
mod keyboard;
mod watchers;

use keyboard::{
    QqPermissionCallbackAction, parse_qq_permission_callback, qq_command_keyboard,
    qq_permission_delivery, qq_permission_keyboard, qq_permission_resolved_keyboard,
    qq_permission_resolved_text, qq_root_keyboard_for_callback, qq_session_switch_keyboard,
};

const QQ_TEXT_LIMIT: usize = 4_000;

const QQ_MSG_TYPE_TEXT: i64 = 0;
const QQ_MSG_TYPE_MARKDOWN: i64 = 2;
const QQ_MSG_TYPE_MEDIA: i64 = 7;

#[derive(Clone)]
struct AccessToken {
    value: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct ReplyTarget {
    kind: ReplyTargetKind,
    source_message_id: Option<String>,
}

#[derive(Clone)]
enum ReplyTargetKind {
    C2c { openid: String },
    Group { group_openid: String },
}

pub struct QqChannelConfig {
    pub app_id: String,
    pub app_secret: String,
    pub sandbox: bool,
    pub markdown: bool,
    pub remove_at: bool,
    pub max_retry: usize,
}

pub struct QqChannel {
    app_id: String,
    app_secret: String,
    sandbox: bool,
    markdown: bool,
    remove_at: bool,
    max_retry: usize,
    client: reqwest::Client,
    store: ChannelStore,
    state: Arc<DaemonState>,
    token: Mutex<Option<AccessToken>>,
    session_watchers: Arc<std::sync::Mutex<std::collections::HashMap<String, watch::Sender<bool>>>>,
}

impl QqChannel {
    #[must_use]
    pub fn new(config: QqChannelConfig, store: ChannelStore, state: Arc<DaemonState>) -> Self {
        Self {
            app_id: config.app_id,
            app_secret: config.app_secret,
            sandbox: config.sandbox,
            markdown: config.markdown,
            remove_at: config.remove_at,
            max_retry: config.max_retry,
            client: reqwest::Client::new(),
            store,
            state,
            token: Mutex::new(None),
            session_watchers: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    async fn handle_dispatch(&self, event_type: &str, data: &serde_json::Value) {
        match event_type {
            "C2C_MESSAGE_CREATE" | "GROUP_AT_MESSAGE_CREATE" => {
                let target = if event_type == "C2C_MESSAGE_CREATE" {
                    Self::extract_c2c_target(data)
                } else {
                    self.extract_group_target(data)
                };
                let Some((user_id, user_name, text, attachments, target)) = target else {
                    tracing::info!("[qq] Ignored dispatch event_type={event_type}");
                    return;
                };
                let attachments = self.prepare_inbound_attachments(&attachments).await;
                tracing::info!(
                    "[qq] inbound event_type={event_type} user_id={user_id} user_name={user_name:?} target={} text_len={} attachments={}",
                    target.kind.label(),
                    text.len(),
                    attachments.len()
                );
                self.handle_inbound_message(&user_id, &user_name, &text, &attachments, &target)
                    .await;
            }
            "INTERACTION_CREATE" => {
                self.handle_interaction(data).await;
            }
            _ => {
                tracing::info!("[qq] Ignored dispatch event_type={event_type}");
            }
        }
    }

    async fn handle_inbound_message(
        &self,
        user_id: &str,
        user_name: &str,
        text: &str,
        attachments: &[cortex_types::Attachment],
        target: &ReplyTarget,
    ) {
        let pairing_action = self.pairing_action(user_id, user_name).await;
        match qq_inbound_route(text, &pairing_action) {
            QqInboundRoute::SendPairingPrompt => {
                if let super::pairing::PairingAction::SendPairingPrompt(message) = pairing_action
                    && let Err(error) = self.send_text(target, &message, 1, self.markdown).await
                {
                    tracing::error!("[qq] pairing prompt send failed: {error}");
                }
                return;
            }
            QqInboundRoute::Denied => return,
            QqInboundRoute::SlashCommand => {
                self.handle_slash_command(target, user_id, user_name, text)
                    .await;
                return;
            }
            QqInboundRoute::Turn => {}
        }

        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let user_id_for_turn = user_id.to_string();
        let user_name_for_turn = user_name.to_string();
        let text_for_turn = text.to_string();
        let attachments_for_turn = attachments.to_vec();
        let events = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message_events(
                &state,
                &store,
                &user_id_for_turn,
                &user_name_for_turn,
                &text_for_turn,
                &attachments_for_turn,
                "qq",
            )
        })
        .await
        .unwrap_or_else(|e| vec![crate::daemon::BroadcastEvent::Error(format!("Error: {e}"))]);
        tracing::info!(
            "[qq] turn completed user_id={user_id} target={} events={}",
            target.kind.label(),
            events.len()
        );
        self.send_event_sequence(target, &events, 0).await;
    }

    async fn pairing_action(
        &self,
        user_id: &str,
        user_name: &str,
    ) -> super::pairing::PairingAction {
        let store_dir = self.store.dir().to_path_buf();
        let user_id = user_id.to_string();
        let user_name = user_name.to_string();
        tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::pairing::check_user(&store, &user_id, &user_name, "qq")
        })
        .await
        .unwrap_or(super::pairing::PairingAction::Denied)
    }

    async fn handle_slash_command(
        &self,
        target: &ReplyTarget,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) {
        let bare_cmd = text.split_whitespace().next().unwrap_or(text);
        if bare_cmd == text.trim()
            && let Some(keyboard) = self.command_keyboard(bare_cmd)
        {
            let response = self.dispatch_slash_command(text, user_id, user_name).await;
            let msg_text = if response.is_empty() {
                bare_cmd.to_string()
            } else {
                response
            };
            self.send_slash_reply(target, &msg_text, Some(&keyboard))
                .await;
            return;
        }

        if text == "/session switch" {
            let keyboard = self.session_switch_keyboard(user_id).await;
            let response = if keyboard.is_some() {
                "🗂️ Choose a session:".to_string()
            } else {
                "🗂️ No other sessions to switch to.".to_string()
            };
            if let Some(keyboard) = keyboard {
                self.send_slash_reply(target, &response, Some(&keyboard))
                    .await;
            } else {
                self.send_slash_reply(target, &response, None).await;
            }
            return;
        }

        let response = self.dispatch_slash_command(text, user_id, user_name).await;
        if !response.trim().is_empty() {
            let keyboard = self.root_keyboard_for_callback(text);
            if let Some(keyboard) = keyboard {
                self.send_slash_reply(target, &response, Some(&keyboard))
                    .await;
            } else {
                self.send_slash_reply(target, &response, None).await;
            }
        }
    }

    async fn send_slash_reply(
        &self,
        target: &ReplyTarget,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) {
        let send_result = if let Some(keyboard) = keyboard {
            self.send_text_with_keyboard(target, text, 1, self.markdown, keyboard)
                .await
        } else {
            self.send_text(target, text, 1, self.markdown).await
        };

        if let Err(error) = send_result {
            tracing::warn!("[qq] reply send failed: {error}");
            let fallback_target = target.without_source_message();
            let fallback_result = if let Some(keyboard) = keyboard {
                self.send_text_with_keyboard(&fallback_target, text, 1, self.markdown, keyboard)
                    .await
            } else {
                self.send_text(&fallback_target, text, 1, self.markdown)
                    .await
            };
            if let Err(fallback_error) = fallback_result {
                tracing::error!("[qq] reply fallback send failed: {fallback_error}");
            }
        }
    }

    async fn dispatch_slash_command(&self, text: &str, user_id: &str, user_name: &str) -> String {
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uid = user_id.to_string();
        let uname = user_name.to_string();
        let cmd = text.to_string();
        tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &uid, &uname, &cmd, "qq")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"))
    }

    async fn session_switch_keyboard(&self, user_id: &str) -> Option<serde_json::Value> {
        let state = Arc::clone(&self.state);
        let actor = crate::daemon::DaemonState::channel_actor("qq", user_id);
        let current_session = state.active_actor_session(&actor).unwrap_or_default();
        let sessions = tokio::task::spawn_blocking(move || state.visible_sessions(&actor))
            .await
            .unwrap_or_default();
        qq_session_switch_keyboard(&sessions, Some(&current_session))
    }

    async fn handle_interaction(&self, data: &serde_json::Value) {
        let interaction_id = data
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let button_data = data
            .get("data")
            .and_then(|value| value.get("resolved"))
            .and_then(|value| value.get("button_data"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if interaction_id.is_empty() || button_data.is_empty() {
            return;
        }

        let token = match self.ensure_access_token().await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!("[qq] interaction token unavailable: {error}");
                return;
            }
        };
        let _ = self
            .acknowledge_interaction(&token, interaction_id)
            .await
            .map_err(|error| tracing::warn!("[qq] interaction ack failed: {error}"));

        let target = if let Some(group_openid) =
            data.get("group_openid").and_then(serde_json::Value::as_str)
        {
            ReplyTarget {
                kind: ReplyTargetKind::Group {
                    group_openid: group_openid.to_string(),
                },
                source_message_id: None,
            }
        } else if let Some(openid) = data.get("user_openid").and_then(serde_json::Value::as_str) {
            ReplyTarget {
                kind: ReplyTargetKind::C2c {
                    openid: openid.to_string(),
                },
                source_message_id: None,
            }
        } else {
            return;
        };

        let user_id = data
            .get("group_member_openid")
            .or_else(|| data.get("user_openid"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let user_name = user_id;

        if let Some(action) = parse_qq_permission_callback(button_data) {
            self.handle_permission_interaction(&target, action).await;
            return;
        }

        if button_data == "/session switch" {
            let keyboard = self.session_switch_keyboard(user_id).await;
            let response = if keyboard.is_some() {
                "🗂️ Choose a session:".to_string()
            } else {
                "🗂️ No other sessions to switch to.".to_string()
            };
            if let Some(keyboard) = keyboard {
                self.send_slash_reply(&target, &response, Some(&keyboard))
                    .await;
            } else {
                self.send_slash_reply(&target, &response, None).await;
            }
            return;
        }

        let response = self
            .dispatch_slash_command(button_data, user_id, user_name)
            .await;
        let keyboard = self.root_keyboard_for_callback(button_data);
        if let Some(keyboard) = keyboard {
            self.send_slash_reply(&target, &response, Some(&keyboard))
                .await;
        } else if !response.trim().is_empty() {
            self.send_slash_reply(&target, &response, None).await;
        }
    }

    async fn handle_permission_interaction(
        &self,
        target: &ReplyTarget,
        action: QqPermissionCallbackAction<'_>,
    ) {
        let (command, pending_id) = match action {
            QqPermissionCallbackAction::Approve(id) => (format!("/approve {id}"), id),
            QqPermissionCallbackAction::Deny(id) => (format!("/deny {id}"), id),
            QqPermissionCallbackAction::Refresh(id) => (String::new(), id),
        };

        let response = match action {
            QqPermissionCallbackAction::Refresh(id) => {
                self.state.pending_permission_info(id).map_or_else(
                    || qq_permission_resolved_text(id),
                    |info| info.prompt_text(),
                )
            }
            QqPermissionCallbackAction::Approve(_) | QqPermissionCallbackAction::Deny(_) => {
                self.state.dispatch_command(&command)
            }
        };

        if self.state.pending_permission_info(pending_id).is_some() {
            let keyboard = qq_permission_keyboard(pending_id);
            let _ = self
                .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                .await;
        } else {
            let keyboard = qq_permission_resolved_keyboard(pending_id);
            let _ = self
                .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                .await;
        }
    }

    fn command_keyboard(&self, cmd: &str) -> Option<serde_json::Value> {
        let cfg = self.state.config();
        qq_command_keyboard(cmd, cfg.risk.auto_approve_up_to, !cfg.turn.strip_think_tags)
    }

    fn root_keyboard_for_callback(&self, data: &str) -> Option<serde_json::Value> {
        let cfg = self.state.config();
        qq_root_keyboard_for_callback(
            data,
            cfg.risk.auto_approve_up_to,
            !cfg.turn.strip_think_tags,
        )
    }

    async fn acknowledge_interaction(
        &self,
        access_token: &str,
        interaction_id: &str,
    ) -> Result<(), String> {
        let response = self
            .client
            .put(format!("{}/interactions/{interaction_id}", self.api_base()))
            .header("Authorization", format!("QQBot {access_token}"))
            .json(&serde_json::json!({ "code": 0 }))
            .send()
            .await
            .map_err(|e| format!("failed to ack QQ interaction: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ interaction ack failed: {status} {body}"))
        }
    }

    fn extract_c2c_target(
        data: &serde_json::Value,
    ) -> Option<(
        String,
        String,
        String,
        Vec<cortex_types::Attachment>,
        ReplyTarget,
    )> {
        let author = data.get("author")?;
        let user_id = author
            .get("user_openid")
            .or_else(|| author.get("id"))
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let content = data
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let attachments = Self::extract_raw_attachments(data)
            .into_iter()
            .map(|attachment| attachment.with_source_actor(format!("qq:{user_id}")))
            .collect();
        let message_id = qq_reply_message_id(data);
        Some((
            user_id.clone(),
            user_id.clone(),
            content,
            attachments,
            ReplyTarget {
                kind: ReplyTargetKind::C2c { openid: user_id },
                source_message_id: message_id,
            },
        ))
    }

    fn extract_group_target(
        &self,
        data: &serde_json::Value,
    ) -> Option<(
        String,
        String,
        String,
        Vec<cortex_types::Attachment>,
        ReplyTarget,
    )> {
        let author = data.get("author")?;
        let user_id = author
            .get("member_openid")
            .or_else(|| author.get("id"))
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let user_name = author
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&user_id)
            .to_string();
        let mut content = data
            .get("content")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        if self.remove_at {
            content = strip_self_mentions(&content, data.get("mentions"));
        }
        let attachments = Self::extract_raw_attachments(data)
            .into_iter()
            .map(|attachment| attachment.with_source_actor(format!("qq:{user_id}")))
            .collect();
        let group_openid = data
            .get("group_openid")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let message_id = qq_reply_message_id(data);
        Some((
            user_id,
            user_name,
            content,
            attachments,
            ReplyTarget {
                kind: ReplyTargetKind::Group { group_openid },
                source_message_id: message_id,
            },
        ))
    }

    fn extract_raw_attachments(data: &serde_json::Value) -> Vec<cortex_types::Attachment> {
        data.get("attachments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|att| {
                let mime_type = att
                    .get("content_type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("application/octet-stream");
                let file_name = att
                    .get("filename")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let url = att
                    .get("voice_wav_url")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| att.get("url").and_then(serde_json::Value::as_str))?;
                let media_type =
                    super::infer_attachment_media_type(mime_type, file_name.as_deref());
                let mut attachment = cortex_types::Attachment::new(media_type, mime_type, url)
                    .with_taint(cortex_types::MediaTaint::External);
                if let Some(caption) = att
                    .get("asr_refer_text")
                    .and_then(serde_json::Value::as_str)
                    .filter(|s| !s.trim().is_empty())
                    .map(str::to_string)
                    .or(file_name)
                {
                    attachment = attachment.with_caption(caption);
                }
                if let Some(size) = att.get("size").and_then(serde_json::Value::as_u64) {
                    attachment = attachment.with_size(size);
                }
                Some(attachment)
            })
            .collect()
    }

    async fn prepare_inbound_attachments(
        &self,
        attachments: &[cortex_types::Attachment],
    ) -> Vec<cortex_types::Attachment> {
        let mut prepared = Vec::with_capacity(attachments.len());
        for attachment in attachments.iter().cloned() {
            match self.materialize_attachment(attachment).await {
                Ok(local) => {
                    let enriched =
                        super::enrich_inbound_attachment(&self.state, &self.client, local).await;
                    prepared.push(enriched);
                }
                Err(error) => {
                    tracing::warn!("[qq] attachment materialize failed: {error}");
                }
            }
        }
        prepared
    }

    async fn materialize_attachment(
        &self,
        mut attachment: cortex_types::Attachment,
    ) -> Result<cortex_types::Attachment, String> {
        if !is_remote_media_url(&attachment.url) {
            return Ok(attachment);
        }
        let response = self
            .client
            .get(&attachment.url)
            .send()
            .await
            .map_err(|e| format!("download QQ attachment failed: {e}"))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("read QQ attachment failed: {e}"))?;
        let hash_full = hex::encode(sha2::Sha256::digest(&bytes));
        let hash = &hash_full[..16];
        let ext = attachment
            .mime_type
            .split('/')
            .nth(1)
            .filter(|ext| !ext.is_empty())
            .unwrap_or("bin");
        let blob_dir =
            cortex_kernel::CortexPaths::from_instance_home(self.state.home()).blobs_dir();
        std::fs::create_dir_all(&blob_dir).map_err(|e| format!("create blob dir failed: {e}"))?;
        let local = blob_dir.join(format!("{hash}.{ext}"));
        std::fs::write(&local, &bytes).map_err(|e| format!("write QQ attachment failed: {e}"))?;
        attachment.size = Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        attachment.media_id = format!("sha256:{hash_full}");
        attachment.sha256 = hash_full;
        attachment.url = local.to_string_lossy().to_string();
        Ok(attachment)
    }
}

impl ReplyTargetKind {
    const fn label(&self) -> &'static str {
        match self {
            Self::C2c { .. } => "c2c",
            Self::Group { .. } => "group",
        }
    }
}

impl ReplyTarget {
    fn without_source_message(&self) -> Self {
        Self {
            kind: self.kind.clone(),
            source_message_id: None,
        }
    }
}

fn qq_media_type(attachment: &cortex_types::Attachment) -> Result<i64, String> {
    match attachment.media_type.as_str() {
        "image" => Ok(1),
        "video" => Ok(2),
        "audio" => Ok(3),
        "file" => Ok(4),
        other => Err(format!("unsupported QQ media type: {other}")),
    }
}

fn is_remote_media_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
}

trait BroadcastEventExt {
    fn kind_name(&self) -> &'static str;
}

impl BroadcastEventExt for crate::daemon::BroadcastEvent {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Boundary => "boundary",
            Self::Observer { .. } => "observer",
            Self::Trace { .. } => "trace",
            Self::Done { .. } => "done",
            Self::Error(_) => "error",
            Self::PermissionRequested(_) => "permission",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QqInboundRoute {
    SendPairingPrompt,
    Denied,
    SlashCommand,
    Turn,
}

fn qq_inbound_route(text: &str, pairing_action: &super::pairing::PairingAction) -> QqInboundRoute {
    match pairing_action {
        super::pairing::PairingAction::Allowed if text.starts_with('/') => {
            QqInboundRoute::SlashCommand
        }
        super::pairing::PairingAction::Allowed => QqInboundRoute::Turn,
        super::pairing::PairingAction::SendPairingPrompt(_) => QqInboundRoute::SendPairingPrompt,
        super::pairing::PairingAction::Denied => QqInboundRoute::Denied,
    }
}

fn qq_reply_message_id(data: &serde_json::Value) -> Option<String> {
    ["msg_id", "message_id", "id", "event_id"]
        .into_iter()
        .find_map(|key| data.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn strip_self_mentions(text: &str, mentions: Option<&serde_json::Value>) -> String {
    let mut cleaned = text.to_string();
    let Some(mentions) = mentions.and_then(serde_json::Value::as_array) else {
        return cleaned.trim().to_string();
    };
    for mention in mentions {
        let openid = mention
            .get("member_openid")
            .or_else(|| mention.get("id"))
            .or_else(|| mention.get("user_openid"))
            .and_then(serde_json::Value::as_str);
        let Some(openid) = openid else {
            continue;
        };
        if mention.get("is_you").and_then(serde_json::Value::as_bool) == Some(true) {
            cleaned = cleaned.replace(&format!("<@{openid}>"), "");
            cleaned = cleaned.replace(&format!("<@!{openid}>"), "");
        } else if let Some(name) = mention
            .get("nickname")
            .or_else(|| mention.get("username"))
            .and_then(serde_json::Value::as_str)
        {
            cleaned = cleaned.replace(&format!("<@{openid}>"), &format!("@{name}"));
            cleaned = cleaned.replace(&format!("<@!{openid}>"), &format!("@{name}"));
        }
    }
    cleaned.trim().to_string()
}
