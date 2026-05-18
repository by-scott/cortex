use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch;

use super::{QqChannel, ReplyTarget, ReplyTargetKind};

impl QqChannel {
    pub(super) fn spawn_session_watchers(self: &Arc<Self>) {
        self.reconcile_session_watchers();
    }

    fn reconcile_session_watchers(self: &Arc<Self>) {
        let subscribed: std::collections::HashSet<String> = self
            .store
            .paired_users()
            .into_iter()
            .filter(|user| user.subscribe)
            .map(|user| user.user_id)
            .collect();
        let mut watchers = self
            .session_watchers
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        watchers.retain(|user_id, stop_tx| {
            if subscribed.contains(user_id) {
                true
            } else {
                let _ = stop_tx.send(true);
                false
            }
        });

        for user_id in subscribed {
            if watchers.contains_key(&user_id) {
                continue;
            }
            let (stop_tx, stop_rx) = watch::channel(false);
            self.spawn_session_watcher(&user_id, stop_rx);
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
                    () = tokio::time::sleep(Duration::from_secs(2)) => {}
                }
            }
            channel.clear_session_watchers();
        });
    }

    fn spawn_session_watcher(self: &Arc<Self>, user_id: &str, mut stop_rx: watch::Receiver<bool>) {
        let channel = Arc::clone(self);
        let uid = user_id.to_string();
        tokio::spawn(async move {
            let mut current_session = String::new();
            loop {
                if *stop_rx.borrow() {
                    return;
                }
                let actor = crate::daemon::DaemonState::channel_actor("qq", &uid);
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
                        () = tokio::time::sleep(Duration::from_secs(5)) => {}
                    }
                    continue;
                }
                if active != current_session {
                    current_session = active.clone();
                }
                let mut rx = channel.state.subscribe_session(&current_session);
                loop {
                    let recv = tokio::time::timeout(Duration::from_secs(10), rx.recv());
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
                            if msg.source == "qq" {
                                continue;
                            }
                            if matches!(msg.event, crate::daemon::BroadcastEvent::Text(_)) {
                                continue;
                            }
                            let target = ReplyTarget {
                                kind: ReplyTargetKind::C2c {
                                    openid: uid.clone(),
                                },
                                source_message_id: None,
                            };
                            channel.send_event_sequence(&target, &[msg.event], 0).await;
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!("[qq] Session broadcast lagged, skipped {n} messages");
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                        Err(_) => {
                            let actor = crate::daemon::DaemonState::channel_actor("qq", &uid);
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
}
