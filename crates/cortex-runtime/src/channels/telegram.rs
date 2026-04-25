//! Telegram Bot API channel -- runs inside the daemon process.

use std::sync::Arc;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use sha2::Digest;
use tokio::sync::watch;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

const TELEGRAM_API: &str = "https://api.telegram.org";
const TELEGRAM_TEXT_LIMIT: usize = 3_600;
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

struct TelegramCallbackContext<'a> {
    callback_id: &'a str,
    data: &'a str,
    chat_id: i64,
    message_id: i64,
    user_id: String,
    user_name: &'a str,
}

impl<'a> TelegramCallbackContext<'a> {
    fn from_value(callback: &'a serde_json::Value) -> Self {
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
            .and_then(|message| message.get("chat"))
            .and_then(|chat| chat.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let message_id = callback
            .get("message")
            .and_then(|message| message.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0);
        let user_id = callback
            .get("from")
            .and_then(|from| from.get("id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0)
            .to_string();
        let user_name = callback
            .get("from")
            .and_then(|from| from.get("first_name"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("Unknown");
        Self {
            callback_id,
            data,
            chat_id,
            message_id,
            user_id,
            user_name,
        }
    }
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

    fn build_http_client(long_poll: bool) -> reqwest::Client {
        let idle_timeout = if long_poll {
            std::time::Duration::from_secs(5)
        } else {
            std::time::Duration::from_secs(90)
        };
        reqwest::Client::builder()
            .http1_only()
            .connect_timeout(std::time::Duration::from_secs(10))
            .pool_idle_timeout(idle_timeout)
            .tcp_keepalive(std::time::Duration::from_secs(30))
            .user_agent("cortex-telegram/1.2")
            .build()
            .unwrap_or_else(|_| reqwest::Client::new())
    }

    async fn reset_poll_client(&self) {
        let mut poll_client = self.poll_client.write().await;
        *poll_client = Self::build_http_client(true);
    }

    /// Spawn a per-session watcher for each subscribed paired user.
    ///
    /// The watcher subscribes to the user's active session broadcast channel
    /// and forwards events from **other** transports (non-`"telegram"`) to the
    /// Telegram chat with typewriter-style text editing and separate bubbles
    /// for tool/trace events.  When the active session changes the watcher
    /// re-subscribes automatically.
    fn spawn_session_watchers(self: &Arc<Self>) {
        self.reconcile_session_watchers();
    }

    fn reconcile_session_watchers(self: &Arc<Self>) {
        let subscribed: std::collections::HashMap<String, i64> = self
            .store
            .paired_users()
            .into_iter()
            .filter(|user| user.subscribe)
            .filter_map(|user| {
                user.user_id
                    .parse::<i64>()
                    .ok()
                    .map(|chat_id| (user.user_id, chat_id))
            })
            .collect();
        let mut watchers = self
            .session_watchers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        watchers.retain(|user_id, stop_tx| {
            if subscribed.contains_key(user_id) {
                true
            } else {
                let _ = stop_tx.send(true);
                false
            }
        });

        for (user_id, chat_id) in subscribed {
            if watchers.contains_key(&user_id) {
                continue;
            }
            let (stop_tx, stop_rx) = watch::channel(false);
            self.spawn_session_watcher(&user_id, chat_id, stop_rx);
            watchers.insert(user_id, stop_tx);
        }
    }

    fn clear_session_watchers(&self) {
        let mut watchers = self
            .session_watchers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for stop_tx in watchers.values() {
            let _ = stop_tx.send(true);
        }
        watchers.clear();
    }

    fn spawn_subscription_reconciler(self: &Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        let channel = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                channel.reconcile_session_watchers();
                tokio::select! {
                    changed = shutdown.changed() => {
                        if changed.is_err() || *shutdown.borrow() {
                            break;
                        }
                    }
                    () = tokio::time::sleep(std::time::Duration::from_secs(2)) => {}
                }
            }
            channel.clear_session_watchers();
        });
    }

    /// Spawn a single session watcher for the given user / chat.
    fn spawn_session_watcher(
        self: &Arc<Self>,
        user_id: &str,
        chat_id: i64,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        let channel = Arc::clone(self);
        let uid = user_id.to_string();
        tokio::spawn(async move {
            let mut current_session = String::new();
            loop {
                if *stop_rx.borrow() {
                    return;
                }
                // Resolve the user's active session.
                let actor = crate::daemon::DaemonState::channel_actor("telegram", &uid);
                let active = channel
                    .state
                    .active_actor_session(&actor)
                    .unwrap_or_default();
                if active.is_empty() {
                    tokio::select! {
                        changed = stop_rx.changed() => {
                            if changed.is_err() || *stop_rx.borrow() {
                                return;
                            }
                        }
                        () = tokio::time::sleep(std::time::Duration::from_secs(5)) => {}
                    }
                    continue;
                }
                if active != current_session {
                    current_session = active.clone();
                }

                // Subscribe to this session's broadcast channel.
                let mut rx = channel.state.subscribe_session(&current_session);
                let mut st = WatcherBubbleState::default();

                loop {
                    let recv = tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv());
                    tokio::pin!(recv);
                    match tokio::select! {
                        changed = stop_rx.changed() => {
                            if changed.is_err() || *stop_rx.borrow() {
                                return;
                            }
                            continue;
                        }
                        result = &mut recv => result,
                    } {
                        Ok(Ok(msg)) => {
                            if msg.source != "telegram" {
                                channel
                                    .render_event(chat_id, &msg.event, &mut st, false)
                                    .await;
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
                            let actor = crate::daemon::DaemonState::channel_actor("telegram", &uid);
                            let new_active = channel
                                .state
                                .active_actor_session(&actor)
                                .unwrap_or_default();
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
        preserve_text_draft: bool,
    ) {
        use crate::daemon::BroadcastEvent;
        match event {
            BroadcastEvent::Text(content) => self.render_text_event(chat_id, content, st).await,
            BroadcastEvent::Boundary => {
                self.render_boundary_event(chat_id, st, preserve_text_draft)
                    .await;
            }
            BroadcastEvent::Observer { source, content } => {
                self.render_observer_event(chat_id, source, content, st, preserve_text_draft)
                    .await;
            }
            BroadcastEvent::Trace { category, message } => {
                self.render_trace_event(chat_id, category, message, st, preserve_text_draft)
                    .await;
            }
            BroadcastEvent::Done {
                response,
                response_parts,
            } => {
                self.render_done_event(chat_id, response, response_parts, st)
                    .await;
            }
            BroadcastEvent::Error(error) => self.render_error_event(chat_id, error, st).await,
            BroadcastEvent::PermissionRequested(info) => {
                self.render_permission_event(chat_id, info, st, preserve_text_draft)
                    .await;
            }
        }
    }

    async fn render_text_event(&self, chat_id: i64, content: &str, st: &mut WatcherBubbleState) {
        if !st.observer_buf.is_empty() {
            self.flush_observer_bubble(chat_id, st).await;
        }
        st.text_buf.push_str(content);
        self.flush_oversized_text_bubbles(
            chat_id,
            &mut st.text_buf,
            &mut st.msg_id,
            &mut st.text_msg_ids,
        )
        .await;
        if st.last_edit.elapsed() >= st.throttle
            && Self::should_flush_text_draft(&st.text_buf, st.msg_id)
        {
            st.msg_id = self
                .flush_text_bubble(chat_id, &st.text_buf, st.msg_id, &mut st.text_msg_ids)
                .await;
            st.last_edit = std::time::Instant::now();
        }
    }

    async fn render_boundary_event(
        &self,
        chat_id: i64,
        st: &mut WatcherBubbleState,
        preserve_text_draft: bool,
    ) {
        if !preserve_text_draft {
            self.finalize_text_segment(chat_id, st).await;
        }
        self.flush_observer_bubble(chat_id, st).await;
    }

    async fn render_observer_event(
        &self,
        chat_id: i64,
        source: &str,
        content: &str,
        st: &mut WatcherBubbleState,
        preserve_text_draft: bool,
    ) {
        if !preserve_text_draft {
            self.finalize_text_segment(chat_id, st).await;
        }
        if source == "permission" {
            self.flush_observer_bubble(chat_id, st).await;
            let _ = self
                .send_permission_card_from_prompt(chat_id, content)
                .await;
            return;
        }
        self.append_observer_chunk(chat_id, source, content, st)
            .await;
    }

    async fn render_trace_event(
        &self,
        chat_id: i64,
        category: &str,
        message: &str,
        st: &mut WatcherBubbleState,
        preserve_text_draft: bool,
    ) {
        if !self.state.config().turn.trace.is_enabled(category) {
            return;
        }
        if !preserve_text_draft {
            self.finalize_text_segment(chat_id, st).await;
        }
        self.flush_observer_bubble(chat_id, st).await;
        let _ = self
            .send_message(chat_id, &format!("[{category}] {message}"))
            .await;
    }

    async fn render_done_event(
        &self,
        chat_id: i64,
        response: &str,
        response_parts: &[cortex_types::ResponsePart],
        st: &mut WatcherBubbleState,
    ) {
        prefer_final_text(&mut st.text_buf, response, response_parts);
        tracing::info!(
            chat_id,
            text_len = st.text_buf.len(),
            html_len = Self::rendered_len(&st.text_buf),
            existing_message = st.msg_id.is_some(),
            "[telegram] finalizing watched response"
        );
        self.refresh_final_text_bubbles(chat_id, &st.text_buf.clone(), st)
            .await;
        self.send_response_media(chat_id, response_parts).await;
        self.flush_observer_bubble(chat_id, st).await;
        st.text_buf.clear();
        st.msg_id = None;
        st.text_msg_ids.clear();
    }

    async fn render_error_event(&self, chat_id: i64, error: &str, st: &mut WatcherBubbleState) {
        self.flush_all_text_bubbles(
            chat_id,
            &mut st.text_buf,
            &mut st.msg_id,
            &mut st.text_msg_ids,
        )
        .await;
        self.flush_observer_bubble(chat_id, st).await;
        let _ = self
            .send_message(chat_id, &format!("\u{274c} {error}"))
            .await;
        st.text_buf.clear();
        st.msg_id = None;
    }

    async fn render_permission_event(
        &self,
        chat_id: i64,
        info: &crate::daemon::PendingPermissionInfo,
        st: &mut WatcherBubbleState,
        preserve_text_draft: bool,
    ) {
        if info.source == "telegram" {
            return;
        }
        if !preserve_text_draft {
            self.finalize_text_segment(chat_id, st).await;
        }
        self.flush_observer_bubble(chat_id, st).await;
        let _ = self.send_permission_card(chat_id, info).await;
    }

    /// Run polling loop with graceful shutdown support.
    pub async fn run_polling(self: &Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        // Register bot commands with Telegram
        if let Err(e) = self.register_commands().await {
            tracing::warn!("[telegram] Failed to register commands: {e}");
        }
        // Start per-session watchers for cross-transport sync when enabled.
        self.spawn_session_watchers();
        self.spawn_subscription_reconciler(shutdown.clone());
        let mut offset = self.store.update_offset();
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
                                    self.store.save_update_offset(offset);
                                }
                                self.spawn_ordered_update(update);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("[telegram] Poll error: {e}");
                            self.reset_poll_client().await;
                            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }
    }

    fn spawn_ordered_update(self: &Arc<Self>, update: serde_json::Value) {
        let channel = Arc::clone(self);
        tokio::spawn(async move {
            let edited_chat_id = update
                .get("edited_message")
                .and_then(|msg| msg.get("chat"))
                .and_then(|chat| chat.get("id"))
                .and_then(serde_json::Value::as_i64);
            let Some(chat_id) = edited_chat_id else {
                channel.process_update(&update).await;
                return;
            };
            let lock = channel.chat_lock(chat_id);
            let _guard = lock.lock().await;
            channel.process_update(&update).await;
        });
    }

    fn chat_lock(&self, chat_id: i64) -> Arc<tokio::sync::Mutex<()>> {
        let mut locks = self
            .chat_locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        Arc::clone(
            locks
                .entry(chat_id)
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(()))),
        )
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
        // Start per-session watchers for cross-transport sync when enabled.
        self.spawn_session_watchers();
        self.spawn_subscription_reconciler(shutdown.clone());

        let parsed_addr = addr
            .parse::<std::net::SocketAddr>()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 8443)));

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

    fn default_prompt_for_attachments(attachments: &[cortex_types::Attachment]) -> String {
        let types: Vec<&str> = attachments.iter().map(|a| a.media_type.as_str()).collect();
        if types.contains(&"image") {
            "The previous user message is an image attachment. Describe what you see in the image."
                .to_string()
        } else if types.contains(&"video") {
            "The user sent a video. Describe the content.".to_string()
        } else if types.contains(&"audio") {
            "The user sent an audio message.".to_string()
        } else {
            "The user sent a file.".to_string()
        }
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

    /// Handle an inline-keyboard button click (`callback_query`).
    async fn handle_callback_query(&self, callback: &serde_json::Value) {
        let ctx = TelegramCallbackContext::from_value(callback);

        // Acknowledge the callback to remove the loading spinner
        self.answer_callback_query(ctx.callback_id).await;

        if ctx.data.is_empty() || ctx.chat_id == 0 {
            return;
        }

        if let Some(action) = parse_permission_callback(ctx.data) {
            self.handle_permission_callback(ctx.chat_id, ctx.message_id, action)
                .await;
            return;
        }

        // Special case: bare `/session switch` shows inline keyboard of sessions
        if ctx.data == "/session switch" {
            self.handle_session_switch_callback(ctx.chat_id, ctx.message_id, &ctx.user_id)
                .await;
            return;
        }

        if let Some(keyboard) = self.command_keyboard(ctx.data) {
            self.handle_builtin_callback(ctx.chat_id, ctx.message_id, ctx.data, &keyboard)
                .await;
            return;
        }

        self.handle_message_callback(&ctx).await;
    }

    async fn handle_session_switch_callback(&self, chat_id: i64, message_id: i64, user_id: &str) {
        let state = Arc::clone(&self.state);
        let actor = crate::daemon::DaemonState::channel_actor("telegram", user_id);
        let current_session = state.active_actor_session(&actor).unwrap_or_default();
        let sessions = tokio::task::spawn_blocking(move || state.visible_sessions(&actor))
            .await
            .unwrap_or_default();

        if sessions.is_empty() {
            let _ = self
                .edit_callback_message(
                    chat_id,
                    message_id,
                    "No sessions available.",
                    self.command_keyboard("/session").as_ref(),
                )
                .await;
            return;
        }

        let keyboard = session_switch_keyboard(&sessions, Some(&current_session));

        if keyboard.is_none() {
            let _ = self
                .edit_callback_message(
                    chat_id,
                    message_id,
                    "🗂️ No other sessions to switch to.",
                    self.command_keyboard("/session").as_ref(),
                )
                .await;
            return;
        }

        let _ = self
            .edit_callback_message(chat_id, message_id, "Choose a session:", keyboard.as_ref())
            .await;
    }

    async fn handle_builtin_callback(
        &self,
        chat_id: i64,
        message_id: i64,
        data: &str,
        keyboard: &serde_json::Value,
    ) {
        let state = Arc::clone(&self.state);
        let cmd = data.to_string();
        let response = tokio::task::spawn_blocking(move || state.dispatch_command(&cmd))
            .await
            .unwrap_or_else(|e| format!("Error: {e}"));
        let message = if response.is_empty() {
            data.to_string()
        } else {
            response
        };
        let _ = self
            .edit_callback_message(chat_id, message_id, &message, Some(keyboard))
            .await;
    }

    async fn handle_message_callback(&self, ctx: &TelegramCallbackContext<'_>) {
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uname = ctx.user_name.to_string();
        let cmd = ctx.data.to_string();
        let uid = ctx.user_id.clone();
        let response = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &uid, &uname, &cmd, "telegram")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"));

        if !response.is_empty() {
            let keyboard = self.root_command_keyboard_for_callback(ctx.data);
            let _ = self
                .edit_callback_message(ctx.chat_id, ctx.message_id, &response, keyboard.as_ref())
                .await;
        }
    }

    async fn handle_permission_callback(
        &self,
        chat_id: i64,
        message_id: i64,
        action: PermissionCallbackAction<'_>,
    ) {
        let (command, pending_id) = match action {
            PermissionCallbackAction::Approve(id) => (format!("/approve {id}"), id),
            PermissionCallbackAction::Deny(id) => (format!("/deny {id}"), id),
            PermissionCallbackAction::Refresh(id) => (String::new(), id),
        };

        let response = match action {
            PermissionCallbackAction::Refresh(id) => self
                .state
                .pending_permission_info(id)
                .map_or_else(|| permission_resolved_text(id), |info| info.prompt_text()),
            PermissionCallbackAction::Approve(_) | PermissionCallbackAction::Deny(_) => {
                self.state.dispatch_command(&command)
            }
        };

        let keyboard = if self.state.pending_permission_info(pending_id).is_some() {
            permission_keyboard(pending_id)
        } else {
            permission_resolved_keyboard(pending_id)
        };
        let _ = self
            .edit_callback_message(chat_id, message_id, &response, Some(&keyboard))
            .await;
    }

    async fn answer_callback_query(&self, callback_id: &str) {
        let url = format!("{TELEGRAM_API}/bot{}/answerCallbackQuery", self.bot_token);
        let _ = self
            .api_client
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
            html_len = Self::rendered_len(&st.text_buf),
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
        let final_chunks = if old_ids.len() > 1 {
            Self::split_text_into_exact_bubbles(final_text, old_ids.len())
        } else {
            Self::split_text_into_bubbles(final_text)
        };
        if final_chunks.is_empty() {
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
                .edit_message_with_keyboard(chat_id, mid, buf, None)
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
                        "[telegram] final HTML edit failed; sending a fresh final message instead: {err}"
                    );
                    let new_mid = self.send_message_get_id(chat_id, buf, None).await.ok();
                    if let Some(sent) = new_mid
                        && !text_msg_ids.contains(&sent)
                    {
                        text_msg_ids.push(sent);
                    }
                    new_mid.or(Some(mid))
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
                        "[telegram] final HTML send failed: {err}"
                    );
                    msg_id
                }
            }
        }
    }

    fn should_flush_text_draft(buf: &str, msg_id: Option<i64>) -> bool {
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
            .api_client
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
            .api_client
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
        let blob_dir =
            cortex_kernel::CortexPaths::from_instance_home(self.state.home()).blobs_dir();
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
        let transcript = crate::media::stt::transcribe(
            &media_config,
            media_config.stt_key(&api_key),
            &path,
            &self.api_client,
        )
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

    /// Extract a video attachment.
    async fn extract_video_attachment(
        &self,
        video: &serde_json::Value,
    ) -> Option<cortex_types::Attachment> {
        let file_id = video
            .get("file_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let (path, _) = self.download_telegram_file(file_id).await.ok()?;
        Some(cortex_types::Attachment {
            media_type: "video".into(),
            mime_type: video
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("video/mp4")
                .into(),
            url: path,
            caption: None,
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

        Some(cortex_types::Attachment {
            media_type: "image".into(),
            mime_type: "image/jpeg".into(),
            url: path,
            caption: None,
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

        let mut enriched = Vec::with_capacity(attachments.len());
        for attachment in attachments {
            enriched.push(
                super::enrich_inbound_attachment(&self.state, &self.api_client, attachment).await,
            );
        }
        enriched
    }

    async fn get_updates(&self, offset: i64) -> Result<Vec<serde_json::Value>, String> {
        let url = format!(
            "{}/bot{}/getUpdates?offset={offset}&timeout=30",
            TELEGRAM_API, self.bot_token
        );
        let resp = self
            .poll_client
            .read()
            .await
            .clone()
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
        let mut commands = vec![
            serde_json::json!({"command": "help", "description": "Show available commands"}),
            serde_json::json!({"command": "status", "description": "Runtime status"}),
            serde_json::json!({"command": "permission", "description": "Permission mode"}),
            serde_json::json!({"command": "stop", "description": "Cancel running turn"}),
            serde_json::json!({"command": "session", "description": "Session management"}),
            serde_json::json!({"command": "config", "description": "View configuration"}),
            serde_json::json!({"command": "quit", "description": "End current session"}),
            serde_json::json!({"command": "exit", "description": "End current session"}),
        ];
        for skill in self.state.skill_registry().user_invocable() {
            let valid = skill
                .name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
            if !valid {
                continue;
            }
            let already_present = commands.iter().any(|entry| {
                entry.get("command").and_then(serde_json::Value::as_str)
                    == Some(skill.name.as_str())
            });
            if already_present {
                continue;
            }
            let mut description = skill.description.trim().replace('\n', " ");
            if description.len() > 256 {
                description.truncate(253);
                description.push_str("...");
            }
            commands.push(serde_json::json!({
                "command": skill.name,
                "description": description,
            }));
        }
        let commands = serde_json::json!({
            "commands": commands
        });
        let resp = self
            .api_client
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
    async fn flush_text_bubble(
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
            match self.edit_text_plain(chat_id, mid, buf, None).await {
                Ok(()) => {
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] edited message"
                    );
                    Some(mid)
                }
                Err(err) => {
                    tracing::warn!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] plain-text edit failed; sending a fresh message instead: {err}"
                    );
                    let new_mid = self
                        .send_text_plain(chat_id, buf, None)
                        .await
                        .ok()
                        .and_then(|resp| {
                            resp.get("result")
                                .and_then(|r| r.get("message_id"))
                                .and_then(serde_json::Value::as_i64)
                        });
                    if let Some(sent) = new_mid
                        && !text_msg_ids.contains(&sent)
                    {
                        text_msg_ids.push(sent);
                    }
                    new_mid.or(Some(mid))
                }
            }
        } else {
            match self.send_text_plain(chat_id, buf, None).await {
                Ok(resp) => {
                    let Some(mid) = resp
                        .get("result")
                        .and_then(|r| r.get("message_id"))
                        .and_then(serde_json::Value::as_i64)
                    else {
                        tracing::warn!(
                            chat_id,
                            text_len = buf.len(),
                            "[telegram] plain-text send succeeded without message_id"
                        );
                        return msg_id;
                    };
                    tracing::debug!(
                        chat_id,
                        message_id = mid,
                        text_len = buf.len(),
                        "[telegram] sent plain-text message"
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
                        "[telegram] plain-text send failed: {err}"
                    );
                    msg_id
                }
            }
        }
    }

    async fn flush_oversized_text_bubbles(
        &self,
        chat_id: i64,
        buf: &mut String,
        msg_id: &mut Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) {
        while Self::rendered_len(buf) > TELEGRAM_TEXT_LIMIT {
            let Some((prefix, suffix)) = Self::split_text_for_bubble(buf, TELEGRAM_TEXT_LIMIT)
            else {
                break;
            };
            *msg_id = self
                .flush_text_bubble(chat_id, &prefix, *msg_id, text_msg_ids)
                .await;
            *buf = suffix;
            *msg_id = None;
        }
    }

    async fn flush_all_text_bubbles(
        &self,
        chat_id: i64,
        buf: &mut String,
        msg_id: &mut Option<i64>,
        text_msg_ids: &mut Vec<i64>,
    ) {
        self.flush_oversized_text_bubbles(chat_id, buf, msg_id, text_msg_ids)
            .await;
        if buf.is_empty() {
            return;
        }
        *msg_id = self
            .flush_text_bubble(chat_id, buf, *msg_id, text_msg_ids)
            .await;
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
        self.api_client
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
        self.api_client
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
        self.api_client
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
        self.api_client
            .post(&url)
            .multipart(form)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_response_media(
        &self,
        chat_id: i64,
        response_parts: &[cortex_types::ResponsePart],
    ) {
        for part in response_parts {
            let cortex_types::ResponsePart::Media { attachment } = part else {
                continue;
            };
            let result = match attachment.media_type.as_str() {
                "audio" => self.send_voice(chat_id, &attachment.url).await,
                "image" => self.send_photo(chat_id, &attachment.url).await,
                "video" => self.send_video(chat_id, &attachment.url).await,
                _ => self.send_document(chat_id, &attachment.url).await,
            };
            if let Err(error) = result {
                tracing::warn!("[telegram] Failed to send media: {error}");
                let _ = self.send_message(chat_id, "[media unavailable]").await;
            }
        }
    }

    /// Convert basic Markdown to Telegram-safe HTML.
    fn md_to_html(text: &str) -> String {
        let mut options = Options::empty();
        options.insert(Options::ENABLE_STRIKETHROUGH);
        options.insert(Options::ENABLE_TABLES);
        options.insert(Options::ENABLE_TASKLISTS);
        let parser = Parser::new_ext(text, options);
        let mut html = String::with_capacity(text.len() + text.len() / 4);
        let mut list_stack: Vec<Option<u64>> = Vec::new();
        let mut blockquote_depth = 0usize;

        for event in parser {
            Self::render_markdown_event(&mut html, &mut list_stack, &mut blockquote_depth, event);
        }

        trim_redundant_blank_lines(&html)
    }

    fn render_markdown_event(
        html: &mut String,
        list_stack: &mut Vec<Option<u64>>,
        blockquote_depth: &mut usize,
        event: Event<'_>,
    ) {
        match event {
            Event::Start(tag) => {
                Self::render_markdown_start(html, list_stack, blockquote_depth, tag);
            }
            Event::End(tag) => Self::render_markdown_end(html, list_stack, blockquote_depth, tag),
            Event::Text(text) => Self::render_markdown_text(html, *blockquote_depth, text.as_ref()),
            Event::Code(code) => Self::push_inline_code(html, code.as_ref()),
            Event::SoftBreak | Event::HardBreak => html.push('\n'),
            Event::Rule => html.push_str("\n────────\n"),
            Event::Html(raw) | Event::InlineHtml(raw) => {
                html.push_str(&escape_html(raw.as_ref()));
            }
            Event::FootnoteReference(name) => {
                html.push('[');
                html.push_str(&escape_html(name.as_ref()));
                html.push(']');
            }
            Event::TaskListMarker(checked) => {
                html.push_str(if checked { "☑ " } else { "☐ " });
            }
            Event::InlineMath(expr) => Self::push_inline_code(html, expr.as_ref()),
            Event::DisplayMath(expr) => {
                html.push_str("<pre><code>");
                html.push_str(&escape_html(expr.as_ref()));
                html.push_str("</code></pre>");
            }
        }
    }

    fn render_markdown_start(
        html: &mut String,
        list_stack: &mut Vec<Option<u64>>,
        blockquote_depth: &mut usize,
        tag: Tag<'_>,
    ) {
        match tag {
            Tag::Heading { level, .. } => {
                let _ = level;
                html.push_str("<b>");
            }
            Tag::BlockQuote(_) => {
                *blockquote_depth += 1;
                if !html.ends_with('\n') && !html.is_empty() {
                    html.push('\n');
                }
                html.push_str("&gt; ");
            }
            Tag::CodeBlock(kind) => Self::push_code_block_start(html, kind),
            Tag::List(start) => {
                list_stack.push(start);
                if !html.ends_with('\n') && !html.is_empty() {
                    html.push('\n');
                }
            }
            Tag::Item => Self::push_list_item_prefix(html, list_stack),
            Tag::Emphasis => html.push_str("<i>"),
            Tag::Strong => html.push_str("<b>"),
            Tag::Strikethrough => html.push_str("<s>"),
            Tag::Link { dest_url, .. } | Tag::Image { dest_url, .. } => {
                html.push_str("<a href=\"");
                html.push_str(&escape_html(dest_url.as_ref()));
                html.push_str("\">");
            }
            Tag::Paragraph
            | Tag::FootnoteDefinition(_)
            | Tag::HtmlBlock
            | Tag::DefinitionList
            | Tag::DefinitionListTitle
            | Tag::DefinitionListDefinition
            | Tag::Superscript
            | Tag::Subscript
            | Tag::MetadataBlock(_)
            | Tag::TableHead
            | Tag::TableCell => {}
            Tag::Table(_) => {
                if !html.ends_with('\n') && !html.is_empty() {
                    html.push_str("\n\n");
                }
            }
            Tag::TableRow => {
                if !html.ends_with('\n') && !html.is_empty() {
                    html.push('\n');
                }
                html.push_str("• ");
            }
        }
    }

    fn render_markdown_end(
        html: &mut String,
        list_stack: &mut Vec<Option<u64>>,
        blockquote_depth: &mut usize,
        tag: TagEnd,
    ) {
        match tag {
            TagEnd::Paragraph | TagEnd::Table => html.push_str("\n\n"),
            TagEnd::Heading(_) => html.push_str("</b>\n\n"),
            TagEnd::BlockQuote(_) => {
                *blockquote_depth = blockquote_depth.saturating_sub(1);
                html.push_str("\n\n");
            }
            TagEnd::CodeBlock => html.push_str("</code></pre>\n\n"),
            TagEnd::List(_) => {
                let _ = list_stack.pop();
                if !html.ends_with("\n\n") {
                    html.push('\n');
                }
            }
            TagEnd::Emphasis => html.push_str("</i>"),
            TagEnd::Strong => html.push_str("</b>"),
            TagEnd::Strikethrough => html.push_str("</s>"),
            TagEnd::Link => html.push_str("</a>"),
            TagEnd::Image => {
                if html.ends_with("\">") {
                    html.push_str("[image]");
                }
                html.push_str("</a>");
            }
            TagEnd::Item
            | TagEnd::FootnoteDefinition
            | TagEnd::HtmlBlock
            | TagEnd::DefinitionList
            | TagEnd::DefinitionListTitle
            | TagEnd::DefinitionListDefinition
            | TagEnd::Superscript
            | TagEnd::Subscript
            | TagEnd::MetadataBlock(_) => {}
            TagEnd::TableHead | TagEnd::TableRow => html.push('\n'),
            TagEnd::TableCell => html.push_str("  |  "),
        }
    }

    fn render_markdown_text(html: &mut String, blockquote_depth: usize, text: &str) {
        if blockquote_depth > 0 && html.ends_with('\n') {
            html.push_str("&gt; ");
        }
        html.push_str(&escape_html(text));
    }

    fn push_inline_code(html: &mut String, code: &str) {
        html.push_str("<code>");
        html.push_str(&escape_html(code));
        html.push_str("</code>");
    }

    fn push_code_block_start(html: &mut String, kind: CodeBlockKind<'_>) {
        match kind {
            CodeBlockKind::Indented => html.push_str("<pre><code>"),
            CodeBlockKind::Fenced(lang) => {
                if lang.is_empty() {
                    html.push_str("<pre><code>");
                } else {
                    html.push_str("<pre><code class=\"language-");
                    html.push_str(&escape_html(lang.as_ref()));
                    html.push_str("\">");
                }
            }
        }
    }

    fn push_list_item_prefix(html: &mut String, list_stack: &mut [Option<u64>]) {
        if !html.ends_with('\n') && !html.is_empty() {
            html.push('\n');
        }
        let indent = "  ".repeat(list_stack.len().saturating_sub(1));
        html.push_str(&indent);
        match list_stack.last_mut() {
            Some(Some(next)) => {
                html.push_str(&next.to_string());
                html.push_str(". ");
                *next += 1;
            }
            _ => html.push_str("• "),
        }
    }

    fn rendered_len(text: &str) -> usize {
        Self::md_to_html(text).len()
    }

    fn split_text_for_bubble(text: &str, limit: usize) -> Option<(String, String)> {
        if Self::rendered_len(text) <= limit {
            return None;
        }
        if let Some(idx) = find_safe_split_index(text, limit) {
            return Some(split_at_boundary(text, idx));
        }
        Some(force_split_text(text, limit))
    }

    fn split_text_into_bubbles(text: &str) -> Vec<String> {
        let mut remaining = text.to_string();
        let mut bubbles = Vec::new();

        while let Some((prefix, suffix)) =
            Self::split_text_for_bubble(&remaining, TELEGRAM_TEXT_LIMIT)
        {
            bubbles.push(prefix);
            remaining = suffix;
        }

        if !remaining.is_empty() {
            bubbles.push(remaining);
        }

        bubbles
    }

    fn split_text_into_exact_bubbles(text: &str, target_count: usize) -> Vec<String> {
        if target_count <= 1 {
            return vec![text.to_string()];
        }

        let mut bubbles = Self::split_text_into_bubbles(text);
        while bubbles.len() < target_count {
            let Some((idx, rendered_len)) = bubbles
                .iter()
                .enumerate()
                .map(|(idx, chunk)| (idx, Self::rendered_len(chunk)))
                .filter(|(_, rendered_len)| *rendered_len > 1)
                .max_by_key(|(_, rendered_len)| *rendered_len)
            else {
                break;
            };

            let split_limit = rendered_len.saturating_sub(1);
            let Some((prefix, suffix)) = Self::split_text_for_bubble(&bubbles[idx], split_limit)
            else {
                break;
            };
            bubbles.splice(idx..=idx, vec![prefix, suffix]);
        }

        bubbles
    }

    async fn send_text_html(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let html = Self::md_to_html(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": html,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(resp)
    }

    async fn send_text_plain(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<serde_json::Value, String> {
        let url = format!("{TELEGRAM_API}/bot{}/sendMessage", self.bot_token);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(resp)
    }

    async fn edit_text_html(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/editMessageText", self.bot_token);
        let html = Self::md_to_html(text);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": html,
            "parse_mode": "HTML",
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(())
    }

    async fn edit_text_plain(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let url = format!("{TELEGRAM_API}/bot{}/editMessageText", self.bot_token);
        let mut payload = serde_json::json!({
            "chat_id": chat_id,
            "message_id": message_id,
            "text": text,
        });
        if let Some(keyboard) = keyboard {
            payload["reply_markup"] = keyboard.clone();
        }
        let resp: serde_json::Value = self
            .api_client
            .post(&url)
            .timeout(TELEGRAM_SEND_TIMEOUT)
            .json(&payload)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .json()
            .await
            .map_err(|e| e.to_string())?;
        if resp.get("ok").and_then(serde_json::Value::as_bool) != Some(true) {
            return Err(resp.to_string());
        }
        Ok(())
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), String> {
        let _ = self.send_message_get_id(chat_id, text, None).await?;
        Ok(())
    }

    /// Send a message and return its `message_id` for later editing.
    async fn send_message_get_id(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<i64, String> {
        let resp = match self.send_text_html(chat_id, text, keyboard).await {
            Ok(resp) => resp,
            Err(html_err) => {
                tracing::warn!("[telegram] HTML send failed, retrying plain text: {html_err}");
                self.send_text_plain(chat_id, text, keyboard).await?
            }
        };
        Ok(resp
            .get("result")
            .and_then(|r| r.get("message_id"))
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(0))
    }

    async fn edit_message_with_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        match self
            .edit_text_html(chat_id, message_id, text, keyboard)
            .await
        {
            Ok(()) => Ok(()),
            Err(html_err) => {
                if html_err.contains("message is not modified") {
                    return Ok(());
                }
                tracing::warn!("[telegram] HTML edit failed, retrying plain text: {html_err}");
                self.edit_text_plain(chat_id, message_id, text, keyboard)
                    .await
            }
        }
    }

    /// Send a message with an inline keyboard attached.
    async fn send_message_with_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: &serde_json::Value,
    ) -> Result<(), String> {
        let _ = self
            .send_message_get_id(chat_id, text, Some(keyboard))
            .await?;
        Ok(())
    }

    async fn edit_callback_message(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        if message_id == 0 {
            if let Some(keyboard) = keyboard {
                return self
                    .send_message_with_keyboard(chat_id, text, keyboard)
                    .await;
            }
            return self.send_message(chat_id, text).await;
        }
        self.edit_message_with_keyboard(chat_id, message_id, text, keyboard)
            .await
    }

    async fn send_permission_card(
        &self,
        chat_id: i64,
        info: &crate::daemon::PendingPermissionInfo,
    ) -> Result<(), String> {
        self.send_message_with_keyboard(
            chat_id,
            &info.prompt_text(),
            &permission_keyboard(&info.id),
        )
        .await
    }

    async fn send_permission_card_from_prompt(
        &self,
        chat_id: i64,
        prompt: &str,
    ) -> Result<(), String> {
        let Some(permission_id) = parse_permission_prompt_id(prompt) else {
            return self.send_message(chat_id, prompt).await;
        };
        self.send_message_with_keyboard(chat_id, prompt, &permission_keyboard(permission_id))
            .await
    }
}

// ── Inline Keyboard helpers ─────────────────────────────────────

impl TelegramChannel {
    fn command_keyboard(&self, cmd: &str) -> Option<serde_json::Value> {
        command_keyboard(cmd, self.state.config().risk.auto_approve_up_to)
    }

    fn root_command_keyboard_for_callback(&self, data: &str) -> Option<serde_json::Value> {
        root_command_keyboard_for_callback(data, self.state.config().risk.auto_approve_up_to)
    }
}

/// Return an inline keyboard for bare commands that benefit from buttons.
fn command_keyboard(cmd: &str, current_mode: cortex_types::RiskLevel) -> Option<serde_json::Value> {
    match cmd {
        "/help" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": nav_button_label("Status", cmd, "/status"), "callback_data": "/status"},
                {"text": nav_button_label("Permission", cmd, "/permission"), "callback_data": "/permission"},
            ],[
                {"text": nav_button_label("Sessions", cmd, "/session"), "callback_data": "/session"},
                {"text": nav_button_label("Config", cmd, "/config"), "callback_data": "/config"},
            ],[
                {"text": "Stop", "callback_data": "/stop"},
            ]]
        })),
        "/status" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "Refresh", "callback_data": "/status"},
                {"text": nav_button_label("Permission", cmd, "/permission"), "callback_data": "/permission"},
            ],[
                {"text": nav_button_label("Sessions", cmd, "/session"), "callback_data": "/session"},
                {"text": nav_button_label("Config", cmd, "/config"), "callback_data": "/config"},
            ]]
        })),
        "/permission" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": permission_button_label("Strict", current_mode, cortex_types::RiskLevel::Allow), "callback_data": "/permission strict"},
                {"text": permission_button_label("Balanced", current_mode, cortex_types::RiskLevel::Review), "callback_data": "/permission balanced"},
                {"text": permission_button_label("Open", current_mode, cortex_types::RiskLevel::RequireConfirmation), "callback_data": "/permission open"},
            ],[
                {"text": "Refresh", "callback_data": "/permission"},
                {"text": nav_button_label("Status", cmd, "/status"), "callback_data": "/status"},
            ]]
        })),
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

fn root_command_keyboard_for_callback(
    data: &str,
    current_mode: cortex_types::RiskLevel,
) -> Option<serde_json::Value> {
    if data.starts_with("/help") || data.starts_with("/stop") {
        command_keyboard("/help", current_mode)
    } else if data.starts_with("/status") {
        command_keyboard("/status", current_mode)
    } else if data.starts_with("/permission") {
        command_keyboard("/permission", current_mode)
    } else if data.starts_with("/session") || data == "/quit" {
        command_keyboard("/session", current_mode)
    } else if data.starts_with("/config") {
        command_keyboard("/config", current_mode)
    } else {
        None
    }
}

fn permission_button_label(
    label: &str,
    current_mode: cortex_types::RiskLevel,
    button_mode: cortex_types::RiskLevel,
) -> String {
    if current_mode == button_mode {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn nav_button_label(label: &str, current_cmd: &str, button_cmd: &str) -> String {
    if current_cmd == button_cmd {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn session_switch_keyboard(
    sessions: &[cortex_types::SessionMetadata],
    current_session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut buttons: Vec<Vec<serde_json::Value>> = sessions
        .iter()
        .filter(|session| {
            current_session_id.is_none_or(|current| session.id.to_string() != current)
        })
        .take(10)
        .map(|session| {
            let id = session.id.to_string();
            let short_id = &id[..id.len().min(8)];
            let label = session.name.as_deref().unwrap_or(short_id);
            vec![serde_json::json!({
                "text": format!("{label}  (turns: {})", session.turn_count),
                "callback_data": format!("/session switch {id}"),
            })]
        })
        .collect();

    if buttons.is_empty() {
        return None;
    }
    buttons.push(vec![serde_json::json!({
        "text": "Back",
        "callback_data": "/session",
    })]);
    Some(serde_json::json!({ "inline_keyboard": buttons }))
}

#[derive(Clone, Copy)]
enum PermissionCallbackAction<'a> {
    Approve(&'a str),
    Deny(&'a str),
    Refresh(&'a str),
}

fn parse_permission_callback(data: &str) -> Option<PermissionCallbackAction<'_>> {
    let mut parts = data.splitn(3, ':');
    let prefix = parts.next()?;
    let action = parts.next()?;
    let id = parts.next()?;
    if prefix != "perm" || id.is_empty() {
        return None;
    }
    match action {
        "approve" => Some(PermissionCallbackAction::Approve(id)),
        "deny" => Some(PermissionCallbackAction::Deny(id)),
        "refresh" => Some(PermissionCallbackAction::Refresh(id)),
        _ => None,
    }
}

fn permission_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[
            {"text": "Approve", "callback_data": format!("perm:approve:{id}")},
            {"text": "Deny", "callback_data": format!("perm:deny:{id}")},
        ],[
            {"text": "Refresh", "callback_data": format!("perm:refresh:{id}")},
        ]]
    })
}

fn permission_resolved_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[
            {"text": "Refresh", "callback_data": format!("perm:refresh:{id}")},
        ]]
    })
}

fn parse_permission_prompt_id(prompt: &str) -> Option<&str> {
    for line in prompt.lines() {
        if let Some(id) = line.strip_prefix("Approve: /approve ") {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn permission_resolved_text(id: &str) -> String {
    format!("Permission request {id} is already resolved.")
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

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn trim_redundant_blank_lines(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut newline_run = 0usize;
    for ch in text.trim().chars() {
        if ch == '\n' {
            newline_run += 1;
            if newline_run <= 2 {
                out.push(ch);
            }
        } else {
            newline_run = 0;
            out.push(ch);
        }
    }
    out
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct MarkdownSplitState {
    in_fenced_code_block: bool,
    in_inline_code: bool,
}

fn find_safe_split_index(text: &str, limit: usize) -> Option<usize> {
    let (paragraphs, lines, spaces) = split_boundaries(text);
    <[Vec<usize>; 3]>::from((paragraphs, lines, spaces))
        .into_iter()
        .find_map(|candidates| {
            candidates.into_iter().rev().find(|&idx| {
                let prefix = &text[..idx];
                TelegramChannel::rendered_len(prefix) <= limit && markdown_state(prefix).is_closed()
            })
        })
}

fn split_boundaries(text: &str) -> (Vec<usize>, Vec<usize>, Vec<usize>) {
    let mut paragraphs = Vec::new();
    let mut lines = Vec::new();
    let mut spaces = Vec::new();
    let mut chars = text.char_indices().peekable();

    while let Some((idx, ch)) = chars.next() {
        match ch {
            '\n' => {
                let mut boundary = idx + ch.len_utf8();
                let mut run_len = 1usize;
                while let Some(&(next_idx, next_ch)) = chars.peek() {
                    if next_ch != '\n' {
                        break;
                    }
                    let _ = chars.next();
                    boundary = next_idx + next_ch.len_utf8();
                    run_len += 1;
                }
                if run_len >= 2 {
                    paragraphs.push(boundary);
                } else {
                    lines.push(boundary);
                }
            }
            ' ' | '\t' => spaces.push(idx + ch.len_utf8()),
            _ => {}
        }
    }

    (paragraphs, lines, spaces)
}

fn force_split_text(text: &str, limit: usize) -> (String, String) {
    let mut boundaries: Vec<usize> = text.char_indices().map(|(idx, _)| idx).collect();
    boundaries.push(text.len());
    let first = boundaries.get(1).copied().unwrap_or(text.len());
    let mut low = 1usize;
    let mut high = boundaries.len() - 1;
    let mut best = first;

    while low <= high {
        let mid = low + (high - low) / 2;
        let candidate = boundaries[mid];
        let (prefix, _) = rebalance_split(&text[..candidate], "");
        if TelegramChannel::rendered_len(&prefix) <= limit {
            best = candidate;
            low = mid + 1;
        } else if mid == 0 {
            break;
        } else {
            high = mid - 1;
        }
    }

    split_at_boundary(text, best)
}

fn split_at_boundary(text: &str, idx: usize) -> (String, String) {
    let prefix = text[..idx].trim_end_matches(char::is_whitespace);
    let suffix = text[idx..].trim_start_matches(char::is_whitespace);
    rebalance_split(prefix, suffix)
}

fn rebalance_split(prefix: &str, suffix: &str) -> (String, String) {
    let state = markdown_state(prefix);
    let mut left = prefix.to_string();
    let mut right = suffix.to_string();

    if state.in_inline_code {
        left.push('`');
        if !right.is_empty() {
            right.insert(0, '`');
        }
    }

    if state.in_fenced_code_block {
        if !left.ends_with('\n') {
            left.push('\n');
        }
        left.push_str("```");
        if !right.is_empty() {
            right.insert_str(0, "```\n");
        }
    }

    (left, right)
}

fn markdown_state(text: &str) -> MarkdownSplitState {
    let mut state = MarkdownSplitState::default();
    for line in text.split_inclusive('\n') {
        if toggles_fenced_code_block(line) {
            state.in_fenced_code_block = !state.in_fenced_code_block;
            continue;
        }
        if !state.in_fenced_code_block {
            scan_inline_code_state(line, &mut state.in_inline_code);
        }
    }
    state
}

fn toggles_fenced_code_block(line: &str) -> bool {
    let trimmed = line.trim_end_matches('\n');
    let without_indent = trimmed.trim_start_matches([' ', '\t']);
    without_indent.starts_with("```")
}

fn scan_inline_code_state(line: &str, in_inline_code: &mut bool) {
    let mut escaped = false;
    for ch in line.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match ch {
            '\\' => escaped = true,
            '`' => *in_inline_code = !*in_inline_code,
            _ => {}
        }
    }
}

impl MarkdownSplitState {
    const fn is_closed(self) -> bool {
        !self.in_fenced_code_block && !self.in_inline_code
    }
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
