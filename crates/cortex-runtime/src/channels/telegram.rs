//! Telegram Bot API channel -- runs inside the daemon process.

use std::sync::Arc;

use tokio::sync::watch;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

mod attachments;
mod bot_api;
mod callbacks;
mod keyboard;
mod render;
mod streaming;
mod transport;
mod turn_trace;
mod watchers;

pub(in crate::channels::telegram) use streaming::{StreamChunk, WatcherBubbleState};

const TELEGRAM_API: &str = "https://api.telegram.org";
const TELEGRAM_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Maximum file download size (10 MB).
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

pub struct TelegramChannel {
    bot_token: String,
    api_client: reqwest::Client,
    poll_client: tokio::sync::RwLock<reqwest::Client>,
    store: ChannelStore,
    state: Arc<DaemonState>,
    chat_locks: Arc<std::sync::Mutex<std::collections::HashMap<i64, Arc<tokio::sync::Mutex<()>>>>>,
    session_watchers: Arc<std::sync::Mutex<std::collections::HashMap<String, watch::Sender<bool>>>>,
}

impl TelegramChannel {
    #[must_use]
    pub fn new(bot_token: String, store: ChannelStore, state: Arc<DaemonState>) -> Self {
        let api_client = Self::build_http_client(false);
        Self {
            bot_token,
            api_client,
            poll_client: tokio::sync::RwLock::new(Self::build_http_client(true)),
            store,
            state,
            chat_locks: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            session_watchers: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    /// Build the effective user text from message body, caption, and attachment metadata.
    ///
    /// Falls back through: text -> caption -> video analysis -> voice transcript.
    fn resolve_effective_text(
        text: &str,
        caption: &str,
        attachments: &[cortex_types::Attachment],
    ) -> String {
        let voice_transcript = attachments
            .iter()
            .find(|a| a.media_type == "audio")
            .and_then(|a| a.caption.clone())
            .unwrap_or_default();
        let video_analysis = attachments
            .iter()
            .find(|a| a.media_type == "video")
            .and_then(|a| a.caption.clone())
            .unwrap_or_default();
        let image_analysis = attachments
            .iter()
            .find(|a| a.media_type == "image")
            .and_then(|a| a.caption.clone())
            .unwrap_or_default();

        // Build media analysis prefix
        let mut prefix = String::new();
        if !image_analysis.is_empty() {
            prefix.push_str("[Image analysis] ");
            prefix.push_str(&image_analysis);
            prefix.push('\n');
        }
        if !video_analysis.is_empty() {
            prefix.push_str("[Video analysis] ");
            prefix.push_str(&video_analysis);
            prefix.push('\n');
        }

        if !text.is_empty() {
            if prefix.is_empty() {
                text.to_string()
            } else {
                format!("{prefix}{text}")
            }
        } else if !caption.is_empty() {
            if prefix.is_empty() {
                caption.to_string()
            } else {
                format!("{prefix}{caption}")
            }
        } else if !prefix.is_empty() {
            prefix.trim_end().to_string()
        } else if !voice_transcript.is_empty() {
            voice_transcript
        } else {
            String::new()
        }
    }

    async fn process_update(&self, update: &serde_json::Value) {
        // Handle inline-keyboard button clicks (callback_query)
        if let Some(callback) = update.get("callback_query") {
            self.handle_callback_query(callback).await;
            return;
        }

        let msg = update
            .get("message")
            .or_else(|| update.get("edited_message"));
        let Some(msg) = msg else { return };

        // Text from message body or caption (for media messages)
        let text = msg
            .get("text")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let caption = msg
            .get("caption")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");

        let Some(chat_id) = msg
            .get("chat")
            .and_then(|c| c.get("id"))
            .and_then(serde_json::Value::as_i64)
        else {
            return;
        };
        let user_id = msg
            .get("from")
            .and_then(|f| f.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let user_name = msg
            .get("from")
            .and_then(|f| f.get("first_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown");

        let uid = user_id.to_string();

        // Extract multimedia attachments (photo, voice, video, document)
        let attachments = self
            .extract_attachments(msg)
            .await
            .into_iter()
            .map(|attachment| attachment.with_source_actor(format!("telegram:{uid}")))
            .collect::<Vec<_>>();

        let effective_text = Self::resolve_effective_text(text, caption, &attachments);

        // Nothing to process
        if effective_text.is_empty() && attachments.is_empty() {
            return;
        }

        Self::log_inbound_message(chat_id, user_id, text, caption, &attachments);

        // Default prompt when user sends media without text
        let effective_text = if effective_text.is_empty() && !attachments.is_empty() {
            Self::default_prompt_for_attachments(&attachments)
        } else {
            effective_text
        };

        // Strip @botname suffix from commands (Telegram appends it in groups)
        let text = effective_text
            .split('@')
            .next()
            .unwrap_or(&effective_text)
            .to_string();

        // Check pairing first (synchronous, quick)
        let store_dir = self.store.dir().to_path_buf();
        let user_name_owned = user_name.to_string();
        let uid_clone = uid.clone();
        let pairing_result = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::pairing::check_user(&store, &uid_clone, &user_name_owned, "telegram")
        })
        .await;

        match pairing_result {
            Ok(super::pairing::PairingAction::Allowed) => {}
            Ok(super::pairing::PairingAction::SendPairingPrompt(msg)) => {
                let _ = self.send_message(chat_id, &msg).await;
                return;
            }
            Ok(super::pairing::PairingAction::Denied) | Err(_) => return,
        }

        // Commands (start with /)
        if text.starts_with('/') {
            self.handle_slash_command(chat_id, &uid, user_name, &text)
                .await;
            return;
        }

        // Resolve session from store (handle_message creates one if missing)
        let actor = crate::daemon::DaemonState::channel_actor("telegram", &uid);
        let uid2 = uid.clone();
        let state = Arc::clone(&self.state);
        let session_id = tokio::task::spawn_blocking(move || {
            let actor = crate::daemon::DaemonState::channel_actor("telegram", &uid2);
            state.resolve_actor_session(&actor)
        })
        .await
        .unwrap_or_else(|_| self.state.resolve_actor_session(&actor));

        self.execute_with_queue(chat_id, &session_id, &text, &attachments)
            .await;
    }

    fn log_inbound_message(
        chat_id: i64,
        user_id: i64,
        text: &str,
        caption: &str,
        attachments: &[cortex_types::Attachment],
    ) {
        tracing::info!(
            chat_id,
            user_id,
            text_len = text.len(),
            caption_len = caption.len(),
            attachments = attachments.len(),
            image_attachments = attachments
                .iter()
                .filter(|a| a.media_type == "image")
                .count(),
            "[telegram] inbound message"
        );
    }

    /// Execute a turn, queueing if one is already in progress, then drain
    /// any messages that arrived during execution.
    async fn execute_with_queue(
        &self,
        chat_id: i64,
        session_id: &str,
        text: &str,
        attachments: &[cortex_types::Attachment],
    ) {
        match self.state.inject_message(session_id, text.to_string()) {
            crate::daemon::InjectMessageResult::Accepted => {
                let _ = self
                    .send_message(
                        chat_id,
                        "Message received. It has been injected into the running turn and will be handled after the current execution step finishes.",
                    )
                    .await;
                self.ensure_injected_message_is_delivered(chat_id, session_id, text, attachments)
                    .await;
                return;
            }
            crate::daemon::InjectMessageResult::InputClosed => {
                let _ = self
                    .send_message(
                        chat_id,
                        "The current turn is finalizing; a new turn will be started for this message.",
                    )
                    .await;
            }
            crate::daemon::InjectMessageResult::NoActiveTurn => {}
        }
        self.stream_turn_to_chat(chat_id, session_id, text, attachments, false)
            .await;
    }

    async fn ensure_injected_message_is_delivered(
        &self,
        chat_id: i64,
        session_id: &str,
        text: &str,
        attachments: &[cortex_types::Attachment],
    ) {
        for _ in 0..300 {
            if !self.state.has_active_turn(session_id) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }
        tokio::time::sleep(std::time::Duration::from_millis(150)).await;
        if self.state.session_has_recent_user_message(session_id, text) {
            return;
        }
        tracing::warn!(
            chat_id,
            session_id,
            "Injected Telegram message was not consumed before turn ended; starting a follow-up turn",
        );
        self.stream_turn_to_chat(chat_id, session_id, text, attachments, true)
            .await;
    }

    /// Dispatch a slash command received in chat.
    async fn handle_slash_command(&self, chat_id: i64, uid: &str, _user_name: &str, text: &str) {
        let actor = crate::daemon::DaemonState::channel_actor("telegram", uid);
        let bare_cmd = text.split_whitespace().next().unwrap_or(text);
        // Bare command (no extra arguments) with an inline keyboard
        if bare_cmd == text.trim()
            && let Some(keyboard) = self.command_keyboard(bare_cmd)
        {
            match crate::channels::resolve_channel_slash(&self.state, &actor, text) {
                crate::channels::ChannelSlashAction::Reply(resp) => {
                    let msg_text = if resp.is_empty() {
                        bare_cmd.to_string()
                    } else {
                        resp
                    };
                    let _ = self
                        .send_message_with_keyboard(chat_id, &msg_text, &keyboard)
                        .await;
                    return;
                }
                crate::channels::ChannelSlashAction::RunPrompt { session_id, prompt } => {
                    self.stream_turn_to_chat(chat_id, &session_id, &prompt, &[], false)
                        .await;
                    return;
                }
            }
        }

        match crate::channels::resolve_channel_slash(&self.state, &actor, text) {
            crate::channels::ChannelSlashAction::Reply(resp) => {
                if !resp.is_empty() {
                    let _ = self.send_message(chat_id, &resp).await;
                }
            }
            crate::channels::ChannelSlashAction::RunPrompt { session_id, prompt } => {
                self.stream_turn_to_chat(chat_id, &session_id, &prompt, &[], false)
                    .await;
            }
        }
    }
}

impl TelegramChannel {
    fn command_keyboard(&self, cmd: &str) -> Option<serde_json::Value> {
        let cfg = self.state.config();
        keyboard::command_keyboard(cmd, cfg.risk.auto_approve_up_to, !cfg.turn.strip_think_tags)
    }

    fn root_command_keyboard_for_callback(&self, data: &str) -> Option<serde_json::Value> {
        let cfg = self.state.config();
        keyboard::root_command_keyboard_for_callback(
            data,
            cfg.risk.auto_approve_up_to,
            !cfg.turn.strip_think_tags,
        )
    }
}
