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
mod transport;
mod watchers;

const TELEGRAM_API: &str = "https://api.telegram.org";
const TELEGRAM_SEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
/// Maximum file download size (10 MB).
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

/// Internal chunk type for streaming turn events to Telegram.
enum StreamChunk {
    Event(crate::daemon::BroadcastEvent),
    Done {
        text: String,
        parts: Vec<cortex_types::ResponsePart>,
    },
    Error(String),
}

/// Mutable state for typewriter-style text bubble rendering.
struct WatcherBubbleState {
    text_buf: String,
    msg_id: Option<i64>,
    text_msg_ids: Vec<i64>,
    last_edit: std::time::Instant,
    throttle: std::time::Duration,
    observer_buf: String,
    observer_msg_id: Option<i64>,
    observer_last_edit: std::time::Instant,
    observer_throttle: std::time::Duration,
    observer_source: Option<String>,
}

impl Default for WatcherBubbleState {
    fn default() -> Self {
        Self {
            text_buf: String::new(),
            msg_id: None,
            text_msg_ids: Vec::new(),
            last_edit: std::time::Instant::now(),
            throttle: std::time::Duration::from_millis(500),
            observer_buf: String::new(),
            observer_msg_id: None,
            observer_last_edit: std::time::Instant::now(),
            observer_throttle: std::time::Duration::from_millis(700),
            observer_source: None,
        }
    }
}

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

    /// Execute a turn with typewriter streaming effect.
    ///
    /// - Text: one bubble, progressively edited with accumulated content
    /// - Tool/trace: separate bubbles per event
    /// - Overflow (>4096 chars): new bubble continues the stream
    async fn stream_turn_to_chat(
        &self,
        chat_id: i64,
        session_id: &str,
        prompt: &str,
        attachments: &[cortex_types::Attachment],
        anchor_new_bubble: bool,
    ) {
        let _foreground = match self
            .state
            .acquire_foreground_execution(std::time::Duration::from_secs(30))
            .await
        {
            Ok(foreground) => foreground,
            Err(
                err @ (crate::daemon::ForegroundSlotError::ShuttingDown
                | crate::daemon::ForegroundSlotError::Timeout),
            ) => {
                let _ = self.send_message(chat_id, err.user_message()).await;
                return;
            }
        };
        let (typing_stop, typing_handle) = self.spawn_typing_indicator(chat_id);

        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamChunk>(64);
        self.spawn_streaming_turn(session_id, prompt, attachments, tx);
        self.render_stream_chunks(chat_id, &mut rx, anchor_new_bubble)
            .await;

        // Stop typing indicator
        typing_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        typing_handle.abort();
    }

    fn spawn_streaming_turn(
        &self,
        session_id: &str,
        prompt: &str,
        attachments: &[cortex_types::Attachment],
        tx: tokio::sync::mpsc::Sender<StreamChunk>,
    ) {
        let state = Arc::clone(&self.state);
        let sid = session_id.to_string();
        let prompt_text = prompt.to_string();
        let attachments_owned = attachments.to_vec();
        let tx_event = tx.clone();
        let trace_config = state.config().turn.trace.clone();
        let tx_trace = tx.clone();

        tokio::spawn(async move {
            let timeout_secs = {
                let cfg = state.config();
                cfg.turn.execution_timeout_secs
            };
            let result = crate::daemon::run_blocking_streaming_turn_with_timeout(
                crate::daemon::BlockingStreamingTurnRequest {
                    daemon: Arc::clone(&state),
                    timeout: std::time::Duration::from_secs(timeout_secs),
                    session_id: sid,
                    source: "telegram",
                    input_text: prompt_text,
                    attachments: attachments_owned,
                    inline_images: Vec::new(),
                    tracer: TelegramTracer {
                        tx: tx_trace,
                        config: trace_config,
                    },
                    on_event: Arc::new(move |event| {
                        if let Some(event) =
                            crate::daemon::BroadcastEvent::from_turn_stream_event(event)
                        {
                            let _ = tx_event.try_send(StreamChunk::Event(event));
                        }
                    }),
                },
            )
            .await;
            match result {
                Ok(output) => {
                    let _ = tx.try_send(StreamChunk::Done {
                        text: output.response_text.unwrap_or_default(),
                        parts: output.response_parts,
                    });
                }
                Err(error) => {
                    let _ = tx.try_send(StreamChunk::Error(error));
                }
            }
        });
    }

    async fn render_stream_chunks(
        &self,
        chat_id: i64,
        rx: &mut tokio::sync::mpsc::Receiver<StreamChunk>,
        anchor_new_bubble: bool,
    ) {
        let mut st = WatcherBubbleState::default();
        let delay_text_render = anchor_new_bubble;

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Event(event) => {
                    if delay_text_render
                        && let crate::daemon::BroadcastEvent::Text(content) = &event
                    {
                        st.text_buf.push_str(content);
                        continue;
                    }
                    self.render_event(chat_id, &event, &mut st, delay_text_render)
                        .await;
                }
                StreamChunk::Done { text, parts } => {
                    self.finalize_stream_output(chat_id, &text, &parts, &mut st, delay_text_render)
                        .await;
                    break;
                }
                StreamChunk::Error(error) => {
                    self.flush_all_text_bubbles(
                        chat_id,
                        &mut st.text_buf,
                        &mut st.msg_id,
                        &mut st.text_msg_ids,
                    )
                    .await;
                    self.flush_observer_bubble(chat_id, &mut st).await;
                    let _ = self.send_message(chat_id, &format!("❌ {error}")).await;
                    break;
                }
            }
        }
    }

    async fn finalize_stream_output(
        &self,
        chat_id: i64,
        final_text: &str,
        response_parts: &[cortex_types::ResponsePart],
        st: &mut WatcherBubbleState,
        force_new_text_bubble: bool,
    ) {
        prefer_final_text(&mut st.text_buf, final_text, response_parts);
        if force_new_text_bubble {
            st.msg_id = None;
            st.text_msg_ids.clear();
        }
        tracing::info!(
            chat_id,
            text_len = st.text_buf.len(),
            html_len = render::rendered_len(&st.text_buf),
            existing_message = st.msg_id.is_some(),
            "[telegram] finalizing streamed response"
        );
        self.refresh_final_text_bubbles(chat_id, &st.text_buf.clone(), st)
            .await;
        self.send_response_media(chat_id, response_parts).await;
        self.flush_observer_bubble(chat_id, st).await;
    }

    async fn finalize_text_segment(&self, chat_id: i64, st: &mut WatcherBubbleState) {
        self.flush_all_text_bubbles(
            chat_id,
            &mut st.text_buf,
            &mut st.msg_id,
            &mut st.text_msg_ids,
        )
        .await;
        st.text_buf.clear();
        st.msg_id = None;
        st.text_msg_ids.clear();
    }

    async fn refresh_final_text_bubbles(
        &self,
        chat_id: i64,
        final_text: &str,
        st: &mut WatcherBubbleState,
    ) {
        let old_ids = if st.text_msg_ids.is_empty() {
            st.msg_id.into_iter().collect()
        } else {
            std::mem::take(&mut st.text_msg_ids)
        };
        let final_chunks = render::split_text_into_bubbles(final_text);
        if final_chunks.is_empty() {
            for message_id in old_ids {
                self.delete_message(chat_id, message_id).await;
            }
            return;
        }

        let mut final_ids = Vec::with_capacity(final_chunks.len());

        for (idx, chunk) in final_chunks.iter().enumerate() {
            let current_id = old_ids.get(idx).copied();
            let next_id = self
                .flush_final_text_bubble(chat_id, chunk, current_id, &mut final_ids)
                .await;
            if let Some(message_id) = next_id
                && !final_ids.contains(&message_id)
            {
                final_ids.push(message_id);
            }
        }
        for message_id in old_ids.iter().skip(final_chunks.len()).copied() {
            self.delete_message(chat_id, message_id).await;
        }

        st.msg_id = final_ids.last().copied();
        st.text_msg_ids = final_ids;
    }

    async fn flush_final_text_bubble(
        &self,
        chat_id: i64,
        buf: &str,
        msg_id: Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) -> Option<i64> {
        if buf.is_empty() {
            return msg_id;
        }
        if let Some(mid) = msg_id {
            match self
                .edit_single_message_with_keyboard(chat_id, mid, buf, None)
                .await
            {
                Ok(()) => {
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] edited final HTML message"
                    );
                    Some(mid)
                }
                Err(err) => {
                    tracing::warn!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] final text edit failed; sending a fresh final message instead: {err}"
                    );
                    match self.send_message_get_id(chat_id, buf, None).await {
                        Ok(sent) => {
                            self.delete_message(chat_id, mid).await;
                            if !text_msg_ids.contains(&sent) {
                                text_msg_ids.push(sent);
                            }
                            Some(sent)
                        }
                        Err(send_err) => {
                            tracing::warn!(
                                chat_id,
                                message_id = mid,
                                text_len = buf.len(),
                                "[telegram] replacement final text send failed, keeping previous message: {send_err}"
                            );
                            Some(mid)
                        }
                    }
                }
            }
        } else {
            match self.send_message_get_id(chat_id, buf, None).await {
                Ok(mid) => {
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] sent final HTML message"
                    );
                    if !text_msg_ids.contains(&mid) {
                        text_msg_ids.push(mid);
                    }
                    Some(mid)
                }
                Err(err) => {
                    tracing::warn!(
                        chat_id,
                        text_len = buf.len(),
                        "[telegram] final text send failed: {err}"
                    );
                    msg_id
                }
            }
        }
    }

    fn should_flush_text_draft(buf: &str, msg_id: Option<i64>) -> bool {
        if !render::is_markdown_closed(buf) {
            return false;
        }
        if msg_id.is_some() {
            return true;
        }
        let trimmed = buf.trim();
        let chars = trimmed.chars().count();
        chars >= 32 || (chars >= 12 && trimmed.contains('\n'))
    }

    async fn append_observer_chunk(
        &self,
        chat_id: i64,
        source: &str,
        content: &str,
        st: &mut WatcherBubbleState,
    ) {
        if st.observer_source.as_deref() != Some(source) && !st.observer_buf.is_empty() {
            self.flush_observer_bubble(chat_id, st).await;
        }
        st.observer_source = Some(source.to_string());
        st.observer_buf.push_str(content);
        if st.observer_last_edit.elapsed() >= st.observer_throttle {
            st.observer_msg_id = self
                .flush_observer_text(
                    chat_id,
                    &st.observer_buf,
                    st.observer_msg_id,
                    st.observer_source.as_deref(),
                )
                .await;
            st.observer_last_edit = std::time::Instant::now();
        }
    }

    async fn flush_observer_bubble(&self, chat_id: i64, st: &mut WatcherBubbleState) {
        self.flush_observer_text(
            chat_id,
            &st.observer_buf,
            st.observer_msg_id,
            st.observer_source.as_deref(),
        )
        .await;
        st.observer_buf.clear();
        st.observer_msg_id = None;
        st.observer_source = None;
    }

    async fn flush_observer_text(
        &self,
        chat_id: i64,
        observer_buf: &str,
        observer_msg_id: Option<i64>,
        source: Option<&str>,
    ) -> Option<i64> {
        if observer_buf.trim().is_empty() {
            return observer_msg_id;
        }
        let label = source.unwrap_or("observer");
        let rendered = format!("👁 {label}\n{}", observer_buf.trim());
        self.flush_text_bubble(chat_id, &rendered, observer_msg_id, &mut Vec::new())
            .await
    }

    /// Spawn a background task that sends "typing..." chat actions in a loop.
    ///
    /// Returns a stop flag and the task handle; set the flag to `true` and
    /// abort the handle to stop the indicator.
    fn spawn_typing_indicator(
        &self,
        chat_id: i64,
    ) -> (
        Arc<std::sync::atomic::AtomicBool>,
        tokio::task::JoinHandle<()>,
    ) {
        let client = self.api_client.clone();
        let token = self.bot_token.clone();
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = Arc::clone(&stop);
        let handle = tokio::spawn(async move {
            while !flag.load(std::sync::atomic::Ordering::Relaxed) {
                let url = format!("{TELEGRAM_API}/bot{token}/sendChatAction");
                let _ = client
                    .post(&url)
                    .json(&serde_json::json!({"chat_id": chat_id, "action": "typing"}))
                    .send()
                    .await;
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            }
        });
        (stop, handle)
    }
}

// ── Inline Keyboard helpers ─────────────────────────────────────

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

fn prefer_final_text(
    buf: &mut String,
    final_text: &str,
    response_parts: &[cortex_types::ResponsePart],
) {
    let parts_text = response_parts
        .iter()
        .filter_map(|part| match part {
            cortex_types::ResponsePart::Text { text, .. } if !text.is_empty() => {
                Some(text.as_str())
            }
            _ => None,
        })
        .collect::<String>();

    let replacement = if parts_text.len() >= final_text.len() && !parts_text.is_empty() {
        parts_text.as_str()
    } else {
        final_text
    };

    if replacement.is_empty() || replacement.len() < buf.len() {
        return;
    }

    buf.clear();
    buf.push_str(replacement);
}

// ── Telegram Tracer ─────────────────────────────────────────────

/// Turn tracer that sends trace events to the Telegram streaming channel.
struct TelegramTracer {
    tx: tokio::sync::mpsc::Sender<StreamChunk>,
    config: cortex_types::config::TurnTraceConfig,
}

impl cortex_turn::orchestrator::TurnTracer for TelegramTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat) >= level {
            tracing::info!(category = cat.as_str(), "{message}");
            let _ = self
                .tx
                .try_send(StreamChunk::Event(crate::daemon::BroadcastEvent::Trace {
                    category: cat,
                    message: message.to_string(),
                }));
        }
    }
}
