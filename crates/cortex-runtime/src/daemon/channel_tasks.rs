use std::sync::Arc;

use super::DaemonServer;

impl DaemonServer {
    /// Spawn messaging channel tasks based on config and `auth.json` files.
    /// Returns handles for cleanup on shutdown.
    pub(super) fn spawn_channels(
        &self,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();
        let home = self.state.home();

        if let Some(handle) = self.spawn_telegram_channel(home, shutdown_rx) {
            handles.push(handle);
        }

        if let Some(handle) = self.spawn_whatsapp_channel(home, shutdown_rx) {
            handles.push(handle);
        }

        if let Some(handle) = self.spawn_qq_channel(home, shutdown_rx) {
            handles.push(handle);
        }

        if let Some(handle) = self.spawn_qclaw_channel(home, shutdown_rx) {
            handles.push(handle);
        }

        handles
    }

    fn spawn_telegram_channel(
        &self,
        home: &std::path::Path,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let tg_auth = crate::channels::read_channel_auth(home, "telegram")?;
        let tg_token = tg_auth
            .get("bot_token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let tg_mode = tg_auth
            .get("mode")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("polling")
            .to_string();
        let tg_webhook_addr = tg_auth
            .get("webhook_addr")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        if tg_token.is_empty() {
            return None;
        }
        let store = crate::channels::store::ChannelStore::open(home, "telegram");
        let channel = Arc::new(crate::channels::telegram::TelegramChannel::new(
            tg_token,
            store,
            Arc::clone(&self.state),
        ));
        self.state.add_transport("telegram");

        let rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            if tg_mode == "webhook" && !tg_webhook_addr.is_empty() {
                channel.run_webhook(&tg_webhook_addr, rx).await;
            } else {
                channel.run_polling(rx).await;
            }
        });
        tracing::info!("Telegram channel started");
        Some(handle)
    }

    fn spawn_whatsapp_channel(
        &self,
        home: &std::path::Path,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let wa_auth = crate::channels::read_channel_auth(home, "whatsapp")?;
        let wa_token = wa_auth
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let phone_id = wa_auth
            .get("phone_number_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let verify = wa_auth
            .get("verify_token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let addr = wa_auth
            .get("webhook_addr")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("127.0.0.1:8444")
            .to_string();
        if wa_token.is_empty() || phone_id.is_empty() {
            return None;
        }
        let store = crate::channels::store::ChannelStore::open(home, "whatsapp");
        let channel = Arc::new(crate::channels::whatsapp::WhatsAppCloudChannel::new(
            wa_token,
            phone_id,
            verify,
            store,
            Arc::clone(&self.state),
        ));
        self.state.add_transport("whatsapp");

        let rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            channel.run_webhook(&addr, rx).await;
        });
        tracing::info!("WhatsApp Cloud channel started");
        Some(handle)
    }

    fn spawn_qq_channel(
        &self,
        home: &std::path::Path,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let qq_auth = crate::channels::read_channel_auth(home, "qq")?;
        let app_id = qq_auth
            .get("app_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let app_secret = qq_auth
            .get("app_secret")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let sandbox = qq_auth
            .get("sandbox")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let markdown = qq_auth
            .get("markdown")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let remove_at = qq_auth
            .get("remove_at")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(true);
        let max_retry = qq_auth
            .get("max_retry")
            .and_then(serde_json::Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(10);
        if app_id.is_empty() || app_secret.is_empty() {
            return None;
        }
        let store = crate::channels::store::ChannelStore::open(home, "qq");
        let channel = Arc::new(crate::channels::qq::QqChannel::new(
            crate::channels::qq::QqChannelConfig {
                app_id,
                app_secret,
                sandbox,
                markdown,
                remove_at,
                max_retry,
            },
            store,
            Arc::clone(&self.state),
        ));
        self.state.add_transport("qq");

        let rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            channel.run_websocket(rx).await;
        });
        tracing::info!("QQ channel started");
        Some(handle)
    }

    fn spawn_qclaw_channel(
        &self,
        home: &std::path::Path,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Option<tokio::task::JoinHandle<()>> {
        let auth = crate::channels::read_channel_auth(home, "qclaw")?;
        let config = crate::channels::qclaw::QclawChannelConfig::from_auth(&auth)?;
        let store = crate::channels::store::ChannelStore::open(home, "qclaw");
        let channel = Arc::new(crate::channels::qclaw::QclawChannel::new(
            config,
            store,
            Arc::clone(&self.state),
        ));
        self.state.add_transport("qclaw");

        let rx = shutdown_rx.clone();
        let handle = tokio::spawn(async move {
            channel.run_long_poll(rx).await;
        });
        tracing::info!("QClaw channel started");
        Some(handle)
    }
}
