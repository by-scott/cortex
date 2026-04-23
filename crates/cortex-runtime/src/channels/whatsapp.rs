//! `WhatsApp` channel -- supports Cloud API and Web (`whatsapp-web.js`) modes.
//! Runs inside the daemon process, sharing `DaemonState` directly.

use std::sync::Arc;

use reqwest::multipart;
use sha2::Digest;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

const GRAPH_API: &str = "https://graph.facebook.com/v21.0";
const MAX_MSG_LEN: usize = 65536;

/// Cloud API channel.
pub struct WhatsAppCloudChannel {
    access_token: String,
    phone_number_id: String,
    verify_token: String,
    client: reqwest::Client,
    store: ChannelStore,
    state: Arc<DaemonState>,
}

impl WhatsAppCloudChannel {
    #[must_use]
    pub fn new(
        access_token: String,
        phone_number_id: String,
        verify_token: String,
        store: ChannelStore,
        state: Arc<DaemonState>,
    ) -> Self {
        Self {
            access_token,
            phone_number_id,
            verify_token,
            client: reqwest::Client::new(),
            store,
            state,
        }
    }

    /// Spawn per-session watchers for each subscribed paired user.
    ///
    /// Each watcher subscribes to the user's active session broadcast channel
    /// and forwards events from **other** transports (non-`"whatsapp"`) to the
    /// `WhatsApp` recipient.  When the active session changes the watcher
    /// re-subscribes automatically.
    fn spawn_session_watchers(self: &Arc<Self>) {
        for user in self.store.paired_users() {
            if !user.subscribe {
                continue;
            }
            self.spawn_session_watcher(&user.user_id);
        }
    }

    /// Spawn a single session watcher for a `WhatsApp` user.
    fn spawn_session_watcher(self: &Arc<Self>, user_id: &str) {
        let channel = Arc::clone(self);
        let uid = user_id.to_string();
        tokio::spawn(async move {
            let mut current_session = String::new();
            loop {
                let actor = crate::daemon::DaemonState::channel_actor("whatsapp", &uid);
                let active = channel.state.resolve_actor_session(&actor);
                if active.is_empty() {
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    continue;
                }
                if active != current_session {
                    current_session = active.clone();
                }

                let mut rx = channel.state.subscribe_session(&current_session);

                loop {
                    match tokio::time::timeout(std::time::Duration::from_secs(10), rx.recv()).await
                    {
                        Ok(Ok(msg)) => {
                            // Skip events originating from WhatsApp itself.
                            if msg.source == "whatsapp" {
                                continue;
                            }
                            let recipient = &uid;
                            channel.send_event_sequence(recipient, &[msg.event]).await;
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!(
                                "[whatsapp] Session broadcast lagged, skipped {n} messages"
                            );
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                        Err(_) => {
                            // Timeout -- check if active session changed.
                            let actor = crate::daemon::DaemonState::channel_actor("whatsapp", &uid);
                            let new_active = channel.state.resolve_actor_session(&actor);
                            if new_active != current_session {
                                break; // outer loop will re-subscribe
                            }
                        }
                    }
                }
            }
        });
    }

    /// Run the webhook server with graceful shutdown support.
    ///
    /// # Panics
    ///
    /// Panics if the fallback address literal cannot be parsed (should never happen).
    pub async fn run_webhook(
        self: &Arc<Self>,
        addr: &str,
        mut shutdown: tokio::sync::watch::Receiver<bool>,
    ) {
        use axum::extract::{Query, State};
        use axum::routing::get;
        use axum::{Json, Router};

        // Start per-session watchers for cross-transport sync when enabled.
        self.spawn_session_watchers();

        let parsed_addr = addr
            .parse::<std::net::SocketAddr>()
            .unwrap_or_else(|_| std::net::SocketAddr::from(([127, 0, 0, 1], 8444)));

        let vt = self.verify_token.clone();
        let webhook_handler = get(
            move |Query(p): Query<std::collections::HashMap<String, String>>| {
                let challenge = p.get("hub.challenge").cloned().unwrap_or_default();
                let mode = p.get("hub.mode").cloned().unwrap_or_default();
                let token = p.get("hub.verify_token").cloned().unwrap_or_default();
                let vt_inner = vt.clone();
                async move {
                    if mode == "subscribe" && token == vt_inner {
                        challenge
                    } else {
                        "forbidden".into()
                    }
                }
            },
        )
        .post(
            |State(a): State<Arc<Self>>, Json(body): Json<serde_json::Value>| async move {
                a.process_webhook(&body).await;
                "ok"
            },
        );
        let app = Router::new()
            .route("/whatsapp/webhook", webhook_handler)
            .with_state(Arc::clone(self));

        tracing::info!("[whatsapp-cloud] Webhook listening on {parsed_addr}");
        let Ok(listener) = tokio::net::TcpListener::bind(parsed_addr).await else {
            tracing::error!("[whatsapp] Failed to bind {parsed_addr}");
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
            .unwrap_or_else(|e| tracing::error!("[whatsapp] Error: {e}"));
    }

    async fn process_webhook(&self, body: &serde_json::Value) {
        let Some(entries) = body.get("entry").and_then(serde_json::Value::as_array) else {
            return;
        };
        for entry in entries {
            let Some(changes) = entry.get("changes").and_then(serde_json::Value::as_array) else {
                continue;
            };
            for change in changes {
                let Some(value) = change.get("value") else {
                    continue;
                };
                let Some(messages) = value.get("messages").and_then(serde_json::Value::as_array)
                else {
                    continue;
                };
                for msg in messages {
                    self.handle_wa_message(msg).await;
                }
            }
        }
    }

    async fn handle_wa_message(&self, msg: &serde_json::Value) {
        let Some(from) = msg.get("from").and_then(serde_json::Value::as_str) else {
            return;
        };
        let text = msg
            .get("text")
            .and_then(|t| t.get("body"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let attachments = self.extract_attachments(msg).await;
        let effective_text = super::resolve_effective_inbound_text(text, &attachments);
        if effective_text.is_empty() && attachments.is_empty() {
            return;
        }

        // execute_turn is synchronous -- run in blocking thread
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let from_s = from.to_string();
        let text_s = effective_text;
        let attachments_for_turn = attachments.clone();
        let events = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message_events(
                &state,
                &store,
                &from_s,
                &from_s,
                &text_s,
                &attachments_for_turn,
                "whatsapp",
            )
        })
        .await
        .unwrap_or_else(|e| vec![crate::daemon::BroadcastEvent::Error(format!("Error: {e}"))]);

        self.send_event_sequence(from, &events).await;
    }

    async fn extract_attachments(&self, msg: &serde_json::Value) -> Vec<cortex_types::Attachment> {
        let mut attachments = Vec::new();
        for (media_type, field) in [
            ("image", "image"),
            ("audio", "audio"),
            ("video", "video"),
            ("file", "document"),
        ] {
            let Some(media) = msg.get(field) else {
                continue;
            };
            let Some(id) = media.get("id").and_then(serde_json::Value::as_str) else {
                continue;
            };
            let mime_type = media
                .get("mime_type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(match media_type {
                    "image" => "image/jpeg",
                    "audio" => "audio/ogg",
                    "video" => "video/mp4",
                    _ => "application/octet-stream",
                });
            let caption = media
                .get("caption")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
                .or_else(|| {
                    media
                        .get("filename")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_string)
                });
            match self
                .download_inbound_media(id, mime_type, media_type, caption)
                .await
            {
                Ok(attachment) => attachments.push(attachment),
                Err(error) => tracing::warn!("[whatsapp] inbound attachment failed: {error}"),
            }
        }
        attachments
    }

    async fn download_inbound_media(
        &self,
        media_id: &str,
        mime_type: &str,
        media_type: &str,
        caption: Option<String>,
    ) -> Result<cortex_types::Attachment, String> {
        let meta_url = format!("{GRAPH_API}/{media_id}");
        let meta: serde_json::Value = self
            .client
            .get(&meta_url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| format!("whatsapp media meta failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("whatsapp media meta decode failed: {e}"))?;
        let url = meta
            .get("url")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| format!("whatsapp media meta missing url: {meta}"))?;
        let bytes = self
            .client
            .get(url)
            .bearer_auth(&self.access_token)
            .send()
            .await
            .map_err(|e| format!("whatsapp media download failed: {e}"))?
            .bytes()
            .await
            .map_err(|e| format!("whatsapp media read failed: {e}"))?;
        let hash_full = hex::encode(sha2::Sha256::digest(&bytes));
        let hash = &hash_full[..16];
        let ext = mime_type
            .split('/')
            .nth(1)
            .filter(|ext| !ext.is_empty())
            .unwrap_or("bin");
        let blob_dir =
            cortex_kernel::CortexPaths::from_instance_home(self.state.home()).blobs_dir();
        std::fs::create_dir_all(&blob_dir).map_err(|e| format!("create blob dir failed: {e}"))?;
        let local = blob_dir.join(format!("{hash}.{ext}"));
        std::fs::write(&local, &bytes).map_err(|e| format!("write media failed: {e}"))?;
        let attachment = cortex_types::Attachment {
            media_type: media_type.to_string(),
            mime_type: mime_type.to_string(),
            url: local.to_string_lossy().to_string(),
            caption,
            size: Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX)),
        };
        Ok(super::enrich_inbound_attachment(&self.state, &self.client, attachment).await)
    }

    async fn send_message(&self, to: &str, text: &str) -> Result<(), String> {
        let url = format!("{}/{}/messages", GRAPH_API, self.phone_number_id);
        self.client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&serde_json::json!({
                "messaging_product": "whatsapp",
                "to": to,
                "type": "text",
                "text": {"body": text},
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn send_event_sequence(&self, to: &str, events: &[crate::daemon::BroadcastEvent]) {
        for event in events {
            for item in super::channel_delivery_items(
                event,
                super::ChannelCapabilities::with_media(
                    super::ChannelTextCapability::Plain,
                    super::ChannelCapabilities::IMAGE
                        | super::ChannelCapabilities::AUDIO
                        | super::ChannelCapabilities::VIDEO
                        | super::ChannelCapabilities::FILE,
                ),
            ) {
                match item {
                    super::ChannelDeliveryItem::Text { text, .. } => {
                        if text.is_empty() {
                            continue;
                        }
                        for chunk in super::split_message(&text, MAX_MSG_LEN) {
                            let _ = self.send_message(to, &chunk).await;
                        }
                    }
                    super::ChannelDeliveryItem::Media { attachment } => {
                        let _ = self.send_media(to, &attachment).await;
                    }
                }
            }
        }
    }

    async fn send_media(
        &self,
        to: &str,
        attachment: &cortex_types::Attachment,
    ) -> Result<(), String> {
        let url = format!("{}/{}/messages", GRAPH_API, self.phone_number_id);
        let media_payload = if is_remote_media_url(&attachment.url) {
            whatsapp_media_payload_from_link(attachment)
        } else {
            let media_id = self.upload_media(attachment).await?;
            whatsapp_media_payload_from_id(attachment, &media_id)
        };
        self.client
            .post(&url)
            .bearer_auth(&self.access_token)
            .json(&serde_json::json!({
                "messaging_product": "whatsapp",
                "to": to,
                "type": whatsapp_message_type(attachment),
                whatsapp_message_type(attachment): media_payload,
            }))
            .send()
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn upload_media(&self, attachment: &cortex_types::Attachment) -> Result<String, String> {
        let url = format!("{}/{}/media", GRAPH_API, self.phone_number_id);
        let file_bytes = std::fs::read(&attachment.url)
            .map_err(|e| format!("failed to read attachment {}: {e}", attachment.url))?;
        let file_name = std::path::Path::new(&attachment.url)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("media")
            .to_string();
        let part = multipart::Part::bytes(file_bytes)
            .file_name(file_name)
            .mime_str(&attachment.mime_type)
            .map_err(|e| e.to_string())?;
        let form = multipart::Form::new()
            .text("messaging_product", "whatsapp")
            .text("type", attachment.mime_type.clone())
            .part("file", part);
        let response: serde_json::Value = self
            .client
            .post(&url)
            .bearer_auth(&self.access_token)
            .multipart(form)
            .send()
            .await
            .map_err(|e| format!("whatsapp media upload failed: {e}"))?
            .json()
            .await
            .map_err(|e| format!("whatsapp media upload decode failed: {e}"))?;
        response
            .get("id")
            .and_then(serde_json::Value::as_str)
            .filter(|id| !id.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("whatsapp media upload missing id: {response}"))
    }
}

/// `WhatsApp` Web channel (via `whatsapp-web.js` `Node.js` subprocess).
/// Displays QR code in terminal for scanning.
///
/// This is a blocking function -- call from `spawn_blocking` in async context.
pub fn run_web_mode(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    instance_home: &std::path::Path,
) {
    use std::io::{BufRead, Write};

    tracing::info!("[whatsapp-web] Starting WhatsApp Web bridge...");

    // Check if node is available
    let node_check = std::process::Command::new("node").arg("--version").output();
    match node_check {
        Ok(ref out) if out.status.success() => {}
        _ => {
            tracing::error!("[whatsapp-web] Error: Node.js not found");
            return;
        }
    }

    // Write the bridge script to a temp location
    let script_dir = instance_home
        .join("channels")
        .join("whatsapp")
        .join("bridge");
    let _ = std::fs::create_dir_all(&script_dir);
    let script_path = script_dir.join("bridge.js");
    if let Err(e) = std::fs::write(&script_path, WHATSAPP_WEB_BRIDGE_JS) {
        tracing::error!("[whatsapp-web] Failed to write bridge script: {e}");
        return;
    }

    // Launch node subprocess
    tracing::info!("[whatsapp-web] Scan the QR code with your phone to connect");

    let mut child = match std::process::Command::new("node")
        .arg(&script_path)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::inherit())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            tracing::error!("[whatsapp-web] Failed to start: {e}");
            return;
        }
    };

    // Read messages from stdout (JSON lines), process, write responses to stdin
    let Some(stdout) = child.stdout.take() else {
        tracing::error!("[whatsapp-web] Failed to take stdout");
        return;
    };
    let Some(mut stdin) = child.stdin.take() else {
        tracing::error!("[whatsapp-web] Failed to take stdin");
        return;
    };

    for line in std::io::BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };

        let from = msg
            .get("from")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let body = msg
            .get("body")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let name = msg
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(from);

        if from.is_empty() || body.is_empty() {
            continue;
        }

        let events = super::handle_message_events(state, store, from, name, body, &[], "whatsapp");
        for event in events {
            for text in event.plain_chunks() {
                if text.is_empty() {
                    continue;
                }
                for chunk in super::split_message(&text, MAX_MSG_LEN) {
                    let reply = serde_json::json!({"to": from, "text": chunk});
                    let _ = writeln!(stdin, "{reply}");
                    let _ = stdin.flush();
                }
            }
        }
    }

    let _ = child.wait();
}

/// Minimal `Node.js` bridge script for `whatsapp-web.js`.
const WHATSAPP_WEB_BRIDGE_JS: &str = r"
const { Client, LocalAuth } = require('whatsapp-web.js');
const qrcode = require('qrcode-terminal');
const readline = require('readline');

const client = new Client({
    authStrategy: new LocalAuth({ dataPath: './session' }),
    puppeteer: { headless: true, args: ['--no-sandbox'] }
});

client.on('qr', qr => { qrcode.generate(qr, { small: true }); });
client.on('ready', () => { process.stderr.write('[whatsapp-web] Connected!\n'); });

client.on('message', msg => {
    if (msg.body && !msg.isStatus) {
        const data = JSON.stringify({ from: msg.from, body: msg.body, name: msg._data.notifyName || msg.from });
        process.stdout.write(data + '\n');
    }
});

const rl = readline.createInterface({ input: process.stdin });
rl.on('line', line => {
    try {
        const { to, text } = JSON.parse(line);
        if (to && text) client.sendMessage(to, text);
    } catch {}
});

client.initialize();
";

fn is_remote_media_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn whatsapp_message_type(attachment: &cortex_types::Attachment) -> &'static str {
    match attachment.media_type.as_str() {
        "image" => "image",
        "audio" => "audio",
        "video" => "video",
        _ => "document",
    }
}

fn whatsapp_media_payload_from_link(attachment: &cortex_types::Attachment) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "link": attachment.url,
    });
    if let Some(caption) = attachment.caption.as_deref().map(str::trim)
        && !caption.is_empty()
        && matches!(attachment.media_type.as_str(), "image" | "video" | "file")
    {
        payload["caption"] = serde_json::Value::String(caption.to_string());
    }
    if attachment.media_type == "file"
        && let Some(filename) = std::path::Path::new(&attachment.url)
            .file_name()
            .and_then(|name| name.to_str())
    {
        payload["filename"] = serde_json::Value::String(filename.to_string());
    }
    payload
}

fn whatsapp_media_payload_from_id(
    attachment: &cortex_types::Attachment,
    media_id: &str,
) -> serde_json::Value {
    let mut payload = serde_json::json!({
        "id": media_id,
    });
    if let Some(caption) = attachment.caption.as_deref().map(str::trim)
        && !caption.is_empty()
        && matches!(attachment.media_type.as_str(), "image" | "video" | "file")
    {
        payload["caption"] = serde_json::Value::String(caption.to_string());
    }
    if attachment.media_type == "file"
        && let Some(filename) = std::path::Path::new(&attachment.url)
            .file_name()
            .and_then(|name| name.to_str())
    {
        payload["filename"] = serde_json::Value::String(filename.to_string());
    }
    payload
}
