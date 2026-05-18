use std::sync::Arc;

use tokio::sync::watch;

use super::{TelegramChannel, WatcherBubbleState};

impl TelegramChannel {
    /// Spawn a per-session watcher for each subscribed paired user.
    ///
    /// The watcher subscribes to the user's active session broadcast channel
    /// and forwards events from other transports to the Telegram chat.
    pub(super) fn spawn_session_watchers(self: &Arc<Self>) {
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

    pub(super) fn spawn_subscription_reconciler(
        self: &Arc<Self>,
        mut shutdown: watch::Receiver<bool>,
    ) {
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
                            let actor = crate::daemon::DaemonState::channel_actor("telegram", &uid);
                            let new_active = channel
                                .state
                                .active_actor_session(&actor)
                                .unwrap_or_default();
                            if new_active != current_session {
                                break;
                            }
                        }
                    }
                }
            }
        });
    }

    pub(super) async fn render_event(
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
        super::prefer_final_text(&mut st.text_buf, response, response_parts);
        tracing::info!(
            chat_id,
            text_len = st.text_buf.len(),
            html_len = super::render::rendered_len(&st.text_buf),
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
}
