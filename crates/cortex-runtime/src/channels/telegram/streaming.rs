use std::sync::Arc;

use super::{TelegramChannel, render, turn_trace::TelegramTracer};

/// Internal chunk type for streaming turn events to Telegram.
pub(in crate::channels::telegram) enum StreamChunk {
    Event(crate::daemon::BroadcastEvent),
    Done {
        text: String,
        parts: Vec<cortex_types::ResponsePart>,
    },
    Error(String),
}

/// Mutable state for typewriter-style text bubble rendering.
pub(in crate::channels::telegram) struct WatcherBubbleState {
    pub(in crate::channels::telegram) text_buf: String,
    pub(in crate::channels::telegram) msg_id: Option<i64>,
    pub(in crate::channels::telegram) text_msg_ids: Vec<i64>,
    pub(in crate::channels::telegram) last_edit: std::time::Instant,
    pub(in crate::channels::telegram) throttle: std::time::Duration,
    pub(in crate::channels::telegram) observer_buf: String,
    pub(in crate::channels::telegram) observer_msg_id: Option<i64>,
    pub(in crate::channels::telegram) observer_last_edit: std::time::Instant,
    pub(in crate::channels::telegram) observer_throttle: std::time::Duration,
    pub(in crate::channels::telegram) observer_source: Option<String>,
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

impl TelegramChannel {
    /// Execute a turn with typewriter streaming effect.
    ///
    /// - Text: one bubble, progressively edited with accumulated content
    /// - Tool/trace: separate bubbles per event
    /// - Overflow (>4096 chars): new bubble continues the stream
    pub(super) async fn stream_turn_to_chat(
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

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<StreamChunk>();
        self.spawn_streaming_turn(session_id, prompt, attachments, tx);
        self.render_stream_chunks(chat_id, &mut rx, anchor_new_bubble)
            .await;

        typing_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        typing_handle.abort();
    }

    fn spawn_streaming_turn(
        &self,
        session_id: &str,
        prompt: &str,
        attachments: &[cortex_types::Attachment],
        tx: tokio::sync::mpsc::UnboundedSender<StreamChunk>,
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
                            let _ = tx_event.send(StreamChunk::Event(event));
                        }
                    }),
                },
            )
            .await;
            match result {
                Ok(output) => {
                    let _ = tx.send(StreamChunk::Done {
                        text: output.response_text.unwrap_or_default(),
                        parts: output.response_parts,
                    });
                }
                Err(error) => {
                    let _ = tx.send(StreamChunk::Error(error));
                }
            }
        });
    }

    async fn render_stream_chunks(
        &self,
        chat_id: i64,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<StreamChunk>,
        _anchor_new_bubble: bool,
    ) {
        let mut st = WatcherBubbleState::default();
        let preserve_text_draft = false;
        let mut terminal_received = false;

        while let Some(chunk) = rx.recv().await {
            match chunk {
                StreamChunk::Event(event) => {
                    self.render_event(chat_id, &event, &mut st, preserve_text_draft)
                        .await;
                }
                StreamChunk::Done { text, parts } => {
                    terminal_received = true;
                    self.finalize_stream_output(chat_id, &text, &parts, &mut st, false)
                        .await;
                    break;
                }
                StreamChunk::Error(error) => {
                    terminal_received = true;
                    self.flush_all_text_bubbles(
                        chat_id,
                        &mut st.text_buf,
                        &mut st.msg_id,
                        &mut st.text_msg_ids,
                    )
                    .await;
                    self.flush_observer_bubble(chat_id, &mut st).await;
                    let _ = self
                        .send_message(chat_id, &format!("\u{274c} {error}"))
                        .await;
                    break;
                }
            }
        }

        if !terminal_received {
            self.flush_all_text_bubbles(
                chat_id,
                &mut st.text_buf,
                &mut st.msg_id,
                &mut st.text_msg_ids,
            )
            .await;
            self.flush_observer_bubble(chat_id, &mut st).await;
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
        render::prefer_final_text(&mut st.text_buf, final_text, response_parts);
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

    pub(super) async fn finalize_text_segment(&self, chat_id: i64, st: &mut WatcherBubbleState) {
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

    pub(super) async fn refresh_final_text_bubbles(
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

    pub(super) fn should_flush_text_draft(buf: &str, msg_id: Option<i64>) -> bool {
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

    pub(super) async fn append_observer_chunk(
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

    pub(super) async fn flush_observer_bubble(&self, chat_id: i64, st: &mut WatcherBubbleState) {
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
        let rendered = format!("\u{1f441} {label}\n{}", observer_buf.trim());
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
                let url = format!("{}/bot{token}/sendChatAction", super::TELEGRAM_API);
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
