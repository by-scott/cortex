//! Telegram Bot API channel -- runs inside the daemon process.

use std::sync::Arc;

use sha2::Digest;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

const TELEGRAM_API: &str = "https://api.telegram.org";
const MAX_MSG_LEN: usize = 4096;
/// Maximum file download size (10 MB).
const MAX_DOWNLOAD_BYTES: usize = 10 * 1024 * 1024;

/// Internal chunk type for streaming turn events to Telegram.
enum StreamChunk {
    Text(String),
    Tool(String, String),
    Trace(String, String),
    Done,
}

/// Mutable state for typewriter-style text bubble rendering.
struct WatcherBubbleState {
    text_buf: String,
    msg_id: Option<i64>,
    last_edit: std::time::Instant,
    throttle: std::time::Duration,
}

impl Default for WatcherBubbleState {
    fn default() -> Self {
        Self {
            text_buf: String::new(),
            msg_id: None,
            last_edit: std::time::Instant::now(),
            throttle: std::time::Duration::from_millis(500),
        }
    }
}

pub struct TelegramChannel {
    bot_token: String,
    client: reqwest::Client,
    store: ChannelStore,
    state: Arc<DaemonState>,
}

impl TelegramChannel {
    #[must_use]
    pub fn new(bot_token: String, store: ChannelStore, state: Arc<DaemonState>) -> Self {
        Self {
            bot_token,
            client: reqwest::Client::new(),
            store,
            state,
        }
    }

    /// Spawn a per-session watcher for each paired user.
    ///
    /// The watcher subscribes to the user's active session broadcast channel
    /// and forwards events from **other** transports (non-`"tg"`) to the
    /// Telegram chat with typewriter-style text editing and separate bubbles
    /// for tool/trace events.  When the active session changes the watcher
    /// re-subscribes automatically.
    fn spawn_session_watchers(self: &Arc<Self>) {
        for user in self.store.paired_users() {
            let Some(chat_id) = user.user_id.parse::<i64>().ok() else {
                continue;
            };
            self.spawn_session_watcher(&user.user_id, chat_id);
        }
    }

    /// Spawn a single session watcher for the given user / chat.
    fn spawn_session_watcher(self: &Arc<Self>, user_id: &str, chat_id: i64) {
        let channel = Arc::clone(self);
        let uid = user_id.to_string();
        tokio::spawn(async move {
            let mut current_session = String::new();
            loop {
                // Resolve the user's active session.
                let active = channel.store.active_session(&uid).unwrap_or_default();
                if active.is_empty() {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
                if active != current_session {
                    current_session = active.clone();
                }

                // Subscribe to this session's broadcast channel.
                let mut rx = channel.state.subscribe_session(&current_session);
                let mut st = WatcherBubbleState::default();

                loop {
                    match tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await
                    {
                        Ok(Ok(msg)) => {
                            if msg.source != "tg" {
                                channel.render_event(chat_id, &msg.event, &mut st).await;
                            }
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!(
                                "[telegram] Session broadcast lagged, skipped {n} messages"
                            );
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                        Err(_) => {
                            // Timeout -- check if active session changed.
                            let new_active = channel.store.active_session(&uid).unwrap_or_default();
                            if new_active != current_session {
                                break; // outer loop will re-subscribe
                            }
                        }
                    }
                }
            }
        });
    }

    /// Render a single broadcast event into Telegram bubbles, updating shared
    /// bubble state for typewriter-style text editing.
    async fn render_event(
        &self,
        chat_id: i64,
        event: &crate::daemon::BroadcastEvent,
        st: &mut WatcherBubbleState,
    ) {
        use crate::daemon::BroadcastEvent;
        match event {
            BroadcastEvent::Text(c) => {
                st.text_buf.push_str(c);
                if st.text_buf.len() > MAX_MSG_LEN - 100 {
                    self.flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                        .await;
                    st.text_buf.clear();
                    st.msg_id = None;
                }
                if st.last_edit.elapsed() >= st.throttle {
                    st.msg_id = self
                        .flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                        .await;
                    st.last_edit = std::time::Instant::now();
                }
            }
            BroadcastEvent::Tool { name, status } => {
                // Finalize current text bubble, then start fresh after tool
                self.flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                    .await;
                st.text_buf.clear();
                st.msg_id = None;
                let _ = self
                    .send_message(chat_id, &format!("\u{1f527} {name}: {status}"))
                    .await;
            }
            BroadcastEvent::Trace { category, message } => {
                if self.state.config().turn.trace.is_enabled(category) {
                    self.flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                        .await;
                    st.text_buf.clear();
                    st.msg_id = None;
                    let _ = self
                        .send_message(chat_id, &format!("[{category}] {message}"))
                        .await;
                }
            }
            BroadcastEvent::Done(r) => {
                if st.text_buf.is_empty() {
                    st.text_buf.push_str(r);
                }
                if has_media_markers(&st.text_buf) {
                    self.send_with_media(chat_id, &st.text_buf).await;
                } else {
                    self.flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                        .await;
                }
                st.text_buf.clear();
                st.msg_id = None;
            }
            BroadcastEvent::Error(e) => {
                self.flush_text_bubble(chat_id, &st.text_buf, st.msg_id)
                    .await;
                let _ = self.send_message(chat_id, &format!("\u{274c} {e}")).await;
                st.text_buf.clear();
                st.msg_id = None;
            }
        }
    }

    /// Run polling loop with graceful shutdown support.
    pub async fn run_polling(self: &Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        // Register bot commands with Telegram
        if let Err(e) = self.register_commands().await {
            tracing::warn!("[telegram] Failed to register commands: {e}");
        }
        // Start per-session watchers for cross-transport sync
        self.spawn_session_watchers();
        // Skip stale updates from before this restart by fetching the latest
        // update_id with offset=-1 and confirming it.
        let mut offset: i64 = self.get_updates(-1).await.map_or(0, |updates| {
            updates
                .last()
                .and_then(|u| u.get("update_id"))
                .and_then(serde_json::Value::as_i64)
                .map_or(0, |id| id + 1)
        });
        tracing::info!("[telegram] Polling started (offset={offset})");
        loop {
            tokio::select! {
                biased;
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        tracing::info!("[telegram] Shutting down polling");
                        break;
                    }
                }
                result = self.get_updates(offset) => {
                    match result {
                        Ok(updates) => {
                            for update in updates {
                                if let Some(new_offset) =
                                    update.get("update_id").and_then(serde_json::Value::as_i64)
                                {
                                    offset = new_offset + 1;
                                }
                                self.process_update(&update).await;
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[telegram] Poll error: {e}");
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }
    }

    /// Run webhook mode with graceful shutdown support.
    ///
    /// # Panics
    ///
    /// Panics if the fallback address literal cannot be parsed (should never happen).
    pub async fn run_webhook(
        self: &Arc<Self>,
        addr: &str,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        use axum::extract::State;
        use axum::routing::post;
        use axum::{Json, Router};

        tracing::info!("[telegram] Webhook mode: listening on {addr}");
        // Start per-session watchers for cross-transport sync (if enabled in auth.json)
        self.spawn_session_watchers();

        let parsed_addr = addr
            .parse::<std::net::SocketAddr>()
            .unwrap_or_else(|_| "127.0.0.1:8443".parse().expect("fallback addr"));

        let app =
            Router::new()
                .route(
                    "/telegram/webhook",
                    post(
                        |State(ch): State<Arc<Self>>,
                         Json(update): Json<serde_json::Value>| async move {
                            ch.process_update(&update).await;
                            "ok"
                        },
                    ),
                )
                .with_state(Arc::clone(self));

        let Ok(listener) = tokio::net::TcpListener::bind(parsed_addr).await else {
            tracing::error!("[telegram] Failed to bind {parsed_addr}");
            return;
        };

        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                loop {
                    if shutdown.changed().await.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
            })
            .await
            .unwrap_or_else(|e| tracing::error!("[telegram] Webhook error: {e}"));
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
        let attachments = self.extract_attachments(msg).await;

        let effective_text = Self::resolve_effective_text(text, caption, &attachments);

        // Nothing to process
        if effective_text.is_empty() && attachments.is_empty() {
            return;
        }

        // Default prompt when user sends media without text
        let effective_text = if effective_text.is_empty() && !attachments.is_empty() {
            let types: Vec<&str> = attachments.iter().map(|a| a.media_type.as_str()).collect();
            if types.contains(&"image") {
                "The user sent an image. Describe what you see.".to_string()
            } else if types.contains(&"video") {
                "The user sent a video. Describe the content.".to_string()
            } else if types.contains(&"audio") {
                "The user sent an audio message.".to_string()
            } else {
                "The user sent a file.".to_string()
            }
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
        let store_dir = self.store.dir().to_path_buf();
        let uid2 = uid.clone();
        let session_id = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            store.active_session(&uid2).unwrap_or_else(|| {
                let sid = format!("tg-{uid2}");
                store.set_active_session(&uid2, &sid);
                sid
            })
        })
        .await
        .unwrap_or_else(|_| format!("tg-{uid}"));

        self.execute_with_queue(chat_id, &session_id, &text, &attachments)
            .await;
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
        if self.state.inject_message(session_id, text.to_string()) {
            let _ = self
                .send_message(chat_id, "Message injected into running turn.")
                .await;
            return;
        }
        self.stream_turn_to_chat(chat_id, session_id, text, attachments)
            .await;
    }

    /// Dispatch a slash command received in chat.
    async fn handle_slash_command(&self, chat_id: i64, uid: &str, user_name: &str, text: &str) {
        let bare_cmd = text.split_whitespace().next().unwrap_or(text);
        // Bare command (no extra arguments) with an inline keyboard
        if bare_cmd == text.trim()
            && let Some(keyboard) = command_keyboard(bare_cmd)
        {
            // Dispatch command to get help text, then send with buttons
            let st = Arc::clone(&self.state);
            let t = text.to_string();
            let help = tokio::task::spawn_blocking(move || st.dispatch_command(&t))
                .await
                .unwrap_or_else(|e| format!("Error: {e}"));
            let msg_text = if help.is_empty() {
                bare_cmd.to_string()
            } else {
                help
            };
            let _ = self
                .send_message_with_keyboard(chat_id, &msg_text, &keyboard)
                .await;
            return;
        }

        // Commands with arguments or no keyboard -- dispatch via handle_message
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uname = user_name.to_string();
        let uid2 = uid.to_string();
        let cmd = text.to_string();
        let resp = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &uid2, &uname, &cmd, "tg")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"));
        if !resp.is_empty() {
            let _ = self.send_message(chat_id, &resp).await;
        }
    }

    /// Handle an inline-keyboard button click (`callback_query`).
    async fn handle_callback_query(&self, callback: &serde_json::Value) {
        let callback_id = callback
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let data = callback
            .get("data")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let chat_id = callback
            .get("message")
            .and_then(|m| m.get("chat"))
            .and_then(|c| c.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let user_id = callback
            .get("from")
            .and_then(|f| f.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let user_name = callback
            .get("from")
            .and_then(|f| f.get("first_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown");

        // Acknowledge the callback to remove the loading spinner
        self.answer_callback_query(callback_id).await;

        if data.is_empty() || chat_id == 0 {
            return;
        }

        let uid = user_id.to_string();

        // Special case: bare `/session switch` shows inline keyboard of sessions
        if data == "/session switch" {
            let st = Arc::clone(&self.state);
            let sessions =
                tokio::task::spawn_blocking(move || st.session_manager().list_sessions())
                    .await
                    .unwrap_or_default();

            if sessions.is_empty() {
                let _ = self.send_message(chat_id, "No sessions available.").await;
                return;
            }

            let buttons: Vec<Vec<serde_json::Value>> = sessions
                .iter()
                .filter(|s| s.ended_at.is_some())
                .take(10)
                .map(|s| {
                    let id = s.id.to_string();
                    let short_id = &id[..id.len().min(8)];
                    let label = s.name.as_deref().unwrap_or(short_id);
                    vec![serde_json::json!({
                        "text": format!("{label}  (turns: {})", s.turn_count),
                        "callback_data": format!("/session switch {id}"),
                    })]
                })
                .collect();

            if buttons.is_empty() {
                let _ = self
                    .send_message(chat_id, "No ended sessions to switch to.")
                    .await;
                return;
            }

            let keyboard = serde_json::json!({"inline_keyboard": buttons});
            let _ = self
                .send_message_with_keyboard(chat_id, "Choose a session:", &keyboard)
                .await;
            return;
        }

        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uname = user_name.to_string();
        let cmd = data.to_string();
        let response = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &uid, &uname, &cmd, "tg")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"));

        if !response.is_empty() {
            let _ = self.send_message(chat_id, &response).await;
        }
    }

    async fn answer_callback_query(&self, callback_id: &str) {
        let url = format!("{TELEGRAM_API}/bot{}/answerCallbackQuery", self.bot_token);
        let _ = self
            .client
            .post(&url)
            .json(&serde_json::json!({"callback_query_id": callback_id}))
            .send()
            .await;
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
    ) {
        let (typing_stop, typing_handle) = self.spawn_typing_indicator(chat_id);

        let (tx, mut rx) = tokio::sync::mpsc::channel::<StreamChunk>(64);

        let state = Arc::clone(&self.state);
        let sid = session_id.to_string();
        let prompt_text = prompt.to_string();
        let attachments_owned = attachments.to_vec();
        let tx_text = tx.clone();
        let tx_tool = tx.clone();
        let trace_config = state.config().turn.trace.clone();
        let tx_trace = tx.clone();

        // Run turn in blocking thread, send chunks to channel
        tokio::task::spawn_blocking(move || {
            let tracer = TelegramTracer {
                tx: tx_trace,
                config: trace_config.clone(),
            };
            let turn_input = crate::turn_executor::TurnInput {
                text: &prompt_text,
                attachments: &attachments_owned,
                inline_images: &[],
            };
            let _result = state.execute_turn_streaming(
                &sid,
                &turn_input,
                "tg",
                move |text| {
                    let _ = tx_text.try_send(StreamChunk::Text(text.to_string()));
                },
                move |progress| {
                    let status = match progress.status {
                        cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                        cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                        cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
                    };
                    let _ = tx_tool.try_send(StreamChunk::Tool(
                        progress.tool_name.clone(),
                        status.to_string(),
                    ));
                },
                &tracer,
            );
            let _ = tx.try_send(StreamChunk::Done);
        });

        // Render chunks to Telegram with proper bubble separation:
        // - Consecutive Text chunks → same bubble (typewriter via edit)
        // - Tool event → flush text bubble, new standalone bubble
        // - Trace event → flush text bubble, new standalone bubble
        // - Done → final flush of text bubble
        // - Overflow (>4096) → start new text bubble

        let mut text_buf = String::new();
        let mut text_msg_id: Option<i64> = None;
        let mut last_edit = std::time::Instant::now();
        let throttle = std::time::Duration::from_millis(500);

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Text(content) => {
                    text_buf.push_str(&content);
                    if text_buf.len() > MAX_MSG_LEN - 100 {
                        self.flush_text_bubble(chat_id, &text_buf, text_msg_id)
                            .await;
                        text_buf.clear();
                        text_msg_id = None;
                    }
                    if last_edit.elapsed() >= throttle {
                        text_msg_id = self
                            .flush_text_bubble(chat_id, &text_buf, text_msg_id)
                            .await;
                        last_edit = std::time::Instant::now();
                    }
                }
                StreamChunk::Tool(name, status) => {
                    // Finalize text bubble, reset so next text starts a new bubble
                    self.flush_text_bubble(chat_id, &text_buf, text_msg_id)
                        .await;
                    text_buf.clear();
                    text_msg_id = None;
                    let _ = self
                        .send_message(chat_id, &format!("🔧 {name}: {status}"))
                        .await;
                }
                StreamChunk::Trace(category, message) => {
                    self.flush_text_bubble(chat_id, &text_buf, text_msg_id)
                        .await;
                    text_buf.clear();
                    text_msg_id = None;
                    let _ = self
                        .send_message(chat_id, &format!("[{category}] {message}"))
                        .await;
                }
                StreamChunk::Done => {
                    if has_media_markers(&text_buf) {
                        // Final text contains media markers — send via media-aware path
                        self.send_with_media(chat_id, &text_buf).await;
                    } else {
                        self.flush_text_bubble(chat_id, &text_buf, text_msg_id)
                            .await;
                    }
                    break;
                }
            }
        }

        // Stop typing indicator
        typing_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        typing_handle.abort();
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
        let client = self.client.clone();
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

    /// Download a file from Telegram by `file_id`.
    ///
    /// Returns `(local_path, extension)` on success.  Files are saved under
    /// `data/blobs/{hash16}.{ext}` inside the Cortex home directory.
    async fn download_telegram_file(&self, file_id: &str) -> Result<(String, String), String> {
        // 1. Resolve file_path via getFile
        let url = format!(
            "{TELEGRAM_API}/bot{}/getFile?file_id={file_id}",
            self.bot_token
        );
        let resp: serde_json::Value = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        let file_path = resp
            .get("result")
            .and_then(|r| r.get("file_path"))
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| "getFile: missing file_path".to_string())?;

        // 2. Download the bytes
        let download_url = format!("{TELEGRAM_API}/file/bot{}/{file_path}", self.bot_token);
        let bytes = self
            .client
            .get(&download_url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .bytes()
            .await
            .map_err(|e| e.to_string())?;

        if bytes.len() > MAX_DOWNLOAD_BYTES {
            return Err(format!(
                "file too large ({} bytes, max {MAX_DOWNLOAD_BYTES})",
                bytes.len()
            ));
        }

        // 3. Save to data/blobs/{hash16}.{ext}
        let hash_full = hex::encode(sha2::Sha256::digest(&bytes));
        let hash = &hash_full[..16];
        let ext = file_path.rsplit('.').next().unwrap_or("bin");
        let blob_dir = self.state.home().join("data").join("blobs");
        let local = blob_dir.join(format!("{hash}.{ext}"));
        std::fs::create_dir_all(&blob_dir).map_err(|e| e.to_string())?;
        std::fs::write(&local, &bytes).map_err(|e| e.to_string())?;

        Ok((local.to_string_lossy().to_string(), ext.to_string()))
    }

    /// Extract a voice attachment: download and transcribe via STT.
    async fn extract_voice_attachment(
        &self,
        voice: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = voice
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, _) = self.download_telegram_file(file_id).await.ok()?;
        let (media_config, api_key) = self.resolve_media_config();
        let transcript =
            crate::media::stt::transcribe(&media_config, &api_key, &path, &self.client)
                .await
                .unwrap_or_default();
        Some(cortex_types::Attachment {
            media_type: "audio".into(),
            mime_type: voice
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("audio/ogg")
                .into(),
            url: path,
            caption: if transcript.is_empty() {
                None
            } else {
                Some(transcript)
            },
            size: voice.get("file_size").and_then(serde_json::Value::as_u64),
        })
    }

    /// Extract a video attachment: download and optionally analyze via video understanding.
    async fn extract_video_attachment(
        &self,
        video: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = video
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, _) = self.download_telegram_file(file_id).await.ok()?;
        let (media_config, api_key) = self.resolve_media_config();
        let video_caption = if media_config.video_understand.is_empty() {
            None
        } else {
            crate::media::video_understand::understand(
                &media_config,
                &api_key,
                &path,
                "Describe the content of this video.",
                &self.client,
            )
            .await
            .ok()
        };
        Some(cortex_types::Attachment {
            media_type: "video".into(),
            mime_type: video
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("video/mp4")
                .into(),
            url: path,
            caption: video_caption,
            size: video.get("file_size").and_then(serde_json::Value::as_u64),
        })
    }

    /// Get media config + API key without holding `RwLockReadGuard` across awaits.
    fn resolve_media_config(&self) -> (cortex_types::config::MediaConfig, String) {
        let cfg = self.state.config();
        let mc = cfg.media.clone();
        let api_key_ref = cfg.api.api_key.clone();
        drop(cfg);
        let key = mc.effective_api_key(&api_key_ref).to_string();
        (mc, key)
    }

    /// Extract a photo attachment (largest size from the array).
    ///
    /// When `[media].image_understand` is configured, the image is analyzed
    /// and the description is set as `caption`. Otherwise passed as-is.
    async fn extract_photo_attachment(
        &self,
        photos: &[serde_json::Value],
    ) -> Option<cortex_types::Attachment> {
        let largest = photos.last()?;
        let file_id = largest
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, _) = self.download_telegram_file(file_id).await.ok()?;

        let (mc, key) = self.resolve_media_config();
        let image_caption = if mc.image_understand.is_empty() {
            None
        } else {
            crate::media::image_understand::understand(
                &mc,
                &key,
                &path,
                "Describe the content of this image.",
                &self.client,
            )
            .await
            .ok()
        };

        Some(cortex_types::Attachment {
            media_type: "image".into(),
            mime_type: "image/jpeg".into(),
            url: path,
            caption: image_caption,
            size: None,
        })
    }

    /// Extract a document attachment.
    async fn extract_document_attachment(
        &self,
        doc: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = doc
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, _) = self.download_telegram_file(file_id).await.ok()?;
        Some(cortex_types::Attachment {
            media_type: "file".into(),
            mime_type: doc
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("application/octet-stream")
                .into(),
            url: path,
            caption: doc
                .get("file_name")
                .and_then(serde_json::Value::as_str)
                .map(String::from),
            size: doc.get("file_size").and_then(serde_json::Value::as_u64),
        })
    }

    /// Extract multimedia attachments from a Telegram message object.
    async fn extract_attachments(&self, msg: &serde_json::Value) -> Vec<cortex_types::Attachment> {
        let mut attachments = Vec::new();

        if let Some(photos) = msg.get("photo").and_then(serde_json::Value::as_array)
            && let Some(att) = self.extract_photo_attachment(photos).await
        {
            attachments.push(att);
        }

        if let Some(voice) = msg.get("voice")
            && let Some(att) = self.extract_voice_attachment(voice).await
        {
            attachments.push(att);
        }

        if let Some(video) = msg.get("video")
            && let Some(att) = self.extract_video_attachment(video).await
        {
            attachments.push(att);
        }

        if let Some(doc) = msg.get("document")
            && let Some(att) = self.extract_document_attachment(doc).await
        {
            attachments.push(att);
        }

        attachments
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<serde_json::Value>, String> {
        let url = format!(
            "{}/bot{}/getUpdates?offset={offset}&timeout=30",
            TELEGRAM_API, self.bot_token
        );
        let resp = self
            .client
            .get(&url)
            .timeout(std::time::Duration::from_secs(35))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if !json
            .get("ok")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false)
        {
            return Err(format!("Telegram API error: {json}"));
        }
        Ok(json
            .get("result")
            .and_then(serde_json::Value::as_array)
            .cloned()
            .unwrap_or_default())
    }

    /// Register bot commands with Telegram so they appear in the menu.
    async fn register_commands(&self) -> Result<(), String> {
        let url = format!("{}/bot{}/setMyCommands", TELEGRAM_API, self.bot_token);
        let commands = serde_json::json!({
            "commands": [
                {"command": "help", "description": "Show available commands and skills"},
                {"command": "session", "description": "Session management (list, new, switch)"},
                {"command": "config", "description": "View configuration"},
                {"command": "quit", "description": "End current session"},
                {"command": "deliberate", "description": "Structured decision-making"},
                {"command": "diagnose", "description": "Root cause analysis"},
                {"command": "review", "description": "Critical examination"},
                {"command": "orient", "description": "Rapid comprehension"},
                {"command": "plan", "description": "Task decomposition"},
            ]
        });
        let resp = self
            .client
            .post(&url)
            .json(&commands)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        if body.get("ok").and_then(serde_json::Value::as_bool) == Some(true) {
            tracing::info!("[telegram] Bot commands registered");
        } else {
            tracing::warn!("[telegram] setMyCommands response: {body}");
        }
        Ok(())
    }

    /// Flush a text buffer to a bubble: send if new, edit if existing.
    /// Returns the (possibly new) message ID.
    async fn flush_text_bubble(&self, chat_id: i64, buf: &str, msg_id: Option<i64>) -> Option<i64> {
        if buf.is_empty() {
            return msg_id;
        }
        if let Some(mid) = msg_id {
            let _ = self.edit_message(chat_id, mid, buf).await;
            Some(mid)
        } else {
            self.send_message_get_id(chat_id, buf).await.ok().or(msg_id)
        }
    }

    /// Send a voice/audio file to a chat.
    async fn send_voice(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendVoice", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name("voice.ogg")
            .mime_str("audio/mpeg")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("voice", part);
        self.client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a photo file to a chat.
    async fn send_photo(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendPhoto", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name("image.png")
            .mime_str("image/png")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("photo", part);
        self.client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_video(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendVideo", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("video.mp4");
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str("video/mp4")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("video", part);
        self.client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_document(&self, chat_id: i64, file_path: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendDocument", self.bot_token);
        let file_bytes = std::fs::read(file_path).map_err(|e| e.to_string())?;
        let file_name = std::path::Path::new(file_path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file");
        let part = reqwest::multipart::Part::bytes(file_bytes)
            .file_name(file_name.to_string())
            .mime_str("application/octet-stream")
            .map_err(|e| e.to_string())?;
        let form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", part);
        self.client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send text, detecting media markers and delivering actual files.
    /// Markers: `[audio:/path]`, `[image:/path]`, `[video:/path]`, `[file:/path]`.
    async fn send_with_media(&self, chat_id: i64, text: &str) {
        let parts = split_media_markers(text);
        for part in parts {
            match part {
                MediaPart::Text(t) => {
                    if !t.trim().is_empty() {
                        let _ = self.send_message(chat_id, &t).await;
                    }
                }
                MediaPart::Audio(path) => {
                    if let Err(e) = self.send_voice(chat_id, &path).await {
                        tracing::warn!("[telegram] Failed to send voice: {e}");
                        let _ = self.send_message(chat_id, "[audio file unavailable]").await;
                    }
                }
                MediaPart::Image(path) => {
                    if let Err(e) = self.send_photo(chat_id, &path).await {
                        tracing::warn!("[telegram] Failed to send photo: {e}");
                        let _ = self.send_message(chat_id, "[image file unavailable]").await;
                    }
                }
                MediaPart::Video(path) => {
                    if let Err(e) = self.send_video(chat_id, &path).await {
                        tracing::warn!("[telegram] Failed to send video: {e}");
                        let _ = self.send_message(chat_id, "[video file unavailable]").await;
                    }
                }
                MediaPart::File(path) => {
                    if let Err(e) = self.send_document(chat_id, &path).await {
                        tracing::warn!("[telegram] Failed to send document: {e}");
                        let _ = self.send_message(chat_id, "[file unavailable]").await;
                    }
                }
            }
        }
    }

    /// Convert basic Markdown to Telegram-safe HTML.
    fn md_to_html(text: &str) -> String {
        // Escape HTML entities first
        let s = text
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        // Code blocks: ```lang\n...\n``` → <pre><code>...</code></pre>
        let mut result = String::with_capacity(s.len());
        let mut chars = s.as_str();
        while let Some(start) = chars.find("```") {
            result.push_str(&chars[..start]);
            chars = &chars[start + 3..];
            // Skip optional language tag
            if let Some(nl) = chars.find('\n') {
                chars = &chars[nl + 1..];
            }
            result.push_str("<pre><code>");
            if let Some(end) = chars.find("```") {
                result.push_str(&chars[..end]);
                chars = &chars[end + 3..];
            } else {
                result.push_str(chars);
                chars = "";
            }
            result.push_str("</code></pre>");
        }
        result.push_str(chars);
        // Inline code: `...` → <code>...</code>
        let mut out = String::with_capacity(result.len());
        let mut in_code = false;
        for part in result.split('`') {
            if in_code {
                out.push_str("<code>");
                out.push_str(part);
                out.push_str("</code>");
            } else {
                out.push_str(part);
            }
            in_code = !in_code;
        }
        // Bold: **...** → <b>...</b>
        let out = out.replace("**", "\x01");
        let mut final_out = String::with_capacity(out.len());
        let mut in_bold = false;
        for part in out.split('\x01') {
            if in_bold {
                final_out.push_str("<b>");
                final_out.push_str(part);
                final_out.push_str("</b>");
            } else {
                final_out.push_str(part);
            }
            in_bold = !in_bold;
        }
        final_out
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        let _ = self.send_message_get_id(chat_id, text).await?;
        Ok(())
    }

    /// Send a message and return its `message_id` for later editing.
    async fn send_message_get_id(&self, chat_id: i64, text: &str) -> Result<i64, String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let html = Self::md_to_html(text);
        let resp: serde_json::Value = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": html,
                "parse_mode": "HTML",
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        Ok(resp
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0))
    }

    /// Edit an existing message (typewriter effect).
    async fn edit_message(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/editMessageText", self.bot_token);
        let html = Self::md_to_html(text);
        let _ = self
            .client
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": html,
                "parse_mode": "HTML",
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Send a message with an inline keyboard attached.
    async fn send_message_with_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: &serde_json::Value,
    ) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let html = Self::md_to_html(text);
        self.client
            .post(&url)
            .json(&serde_json::json!({
                "chat_id": chat_id,
                "text": html,
                "parse_mode": "HTML",
                "reply_markup": keyboard,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

// ── Inline Keyboard helpers ─────────────────────────────────────

/// Return an inline keyboard for bare commands that benefit from buttons.
fn command_keyboard(cmd: &str) -> Option<serde_json::Value> {
    match cmd {
        "/session" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "List", "callback_data": "/session list"},
                {"text": "New", "callback_data": "/session new"},
            ],[
                {"text": "Switch", "callback_data": "/session switch"},
                {"text": "End", "callback_data": "/quit"},
            ]]
        })),
        "/config" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "API", "callback_data": "/config get api"},
                {"text": "Memory", "callback_data": "/config get memory"},
                {"text": "Tools", "callback_data": "/config get tools"},
            ],[
                {"text": "Web", "callback_data": "/config get web"},
                {"text": "Skills", "callback_data": "/config get skills"},
                {"text": "Summary", "callback_data": "/config list"},
            ]]
        })),
        _ => None,
    }
}

// ── Media marker helpers ────────────────────────────────────────

/// Check if text contains media markers (`[audio:...]`, `[image:...]`,
/// `[video:...]`, `[file:...]`).
fn has_media_markers(text: &str) -> bool {
    text.contains("[audio:")
        || text.contains("[image:")
        || text.contains("[video:")
        || text.contains("[file:")
}

/// Parsed segment of a response that may contain media markers.
enum MediaPart {
    Text(String),
    Audio(String),
    Image(String),
    Video(String),
    File(String),
}

/// Split text into text segments and media markers.
///
/// Markers: `[audio:/path]`, `[image:/path]`, `[video:/path]`, `[file:/path]`.
fn split_media_markers(text: &str) -> Vec<MediaPart> {
    let mut parts = Vec::new();
    let mut remaining = text;

    let tags: &[(&str, usize)] = &[
        ("[audio:", 7),
        ("[image:", 7),
        ("[video:", 7),
        ("[file:", 6),
    ];

    while let Some(start) = remaining.find('[') {
        let after_bracket = &remaining[start..];
        let tag = tags
            .iter()
            .find(|(prefix, _)| after_bracket.starts_with(*prefix))
            .copied();

        if let Some((prefix, prefix_len)) = tag
            && let Some(end) = after_bracket.find(']')
        {
            let before = &remaining[..start];
            if !before.is_empty() {
                parts.push(MediaPart::Text(before.to_string()));
            }
            let path = &after_bracket[prefix_len..end];
            let kind = &prefix[1..prefix.len() - 1]; // strip [ and :
            match kind {
                "audio" => parts.push(MediaPart::Audio(path.to_string())),
                "image" => parts.push(MediaPart::Image(path.to_string())),
                "video" => parts.push(MediaPart::Video(path.to_string())),
                "file" => parts.push(MediaPart::File(path.to_string())),
                _ => {}
            }
            remaining = &after_bracket[end + 1..];
            continue;
        }
        let before = &remaining[..=start];
        parts.push(MediaPart::Text(before.to_string()));
        remaining = &remaining[start + 1..];
    }

    if !remaining.is_empty() {
        parts.push(MediaPart::Text(remaining.to_string()));
    }
    parts
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
                .try_send(StreamChunk::Trace(cat, message.to_string()));
        }
    }
}
