use std::sync::Arc;

use super::{TELEGRAM_API, TelegramChannel, keyboard};

impl TelegramChannel {
    pub(super) fn build_http_client(long_poll: bool) -> reqwest::Client {
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
        let mut commands = keyboard::telegram_builtin_bot_commands();
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
}
