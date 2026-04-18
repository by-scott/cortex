//! `WhatsApp` channel -- supports Cloud API and Web (`whatsapp-web.js`) modes.
//! Runs inside the daemon process, sharing `DaemonState` directly.

use std::sync::Arc;

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

    /// Spawn per-session watchers for each paired user.
    ///
    /// Each watcher subscribes to the user's active session broadcast channel
    /// and forwards events from **other** transports (non-`"wa"`) to the
    /// `WhatsApp` recipient.  When the active session changes the watcher
    /// re-subscribes automatically.
    fn spawn_session_watchers(self: &Arc<Self>) {
        for user in self.store.paired_users() {
            self.spawn_session_watcher(&user.user_id);
        }
    }

    /// Spawn a single session watcher for a `WhatsApp` user.
    fn spawn_session_watcher(self: &Arc<Self>, user_id: &str) {
        use crate::daemon::BroadcastEvent;

        let channel = Arc::clone(self);
        let uid = user_id.to_string();
        tokio::spawn(async move {
            let mut current_session = String::new();
            loop {
                let active = channel.store.active_session(&uid).unwrap_or_default();
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
                            if msg.source == "wa" {
                                continue;
                            }
                            let recipient = &uid;
                            let text = match &msg.event {
                                BroadcastEvent::Text(content) => content.clone(),
                                BroadcastEvent::Tool { name, status } => {
                                    format!("[tool] {name}: {status}")
                                }
                                BroadcastEvent::Trace { category, message } => {
                                    format!("[{category}] {message}")
                                }
                                BroadcastEvent::Done(response) => response.clone(),
                                BroadcastEvent::Error(e) => format!("[error] {e}"),
                            };
                            for chunk in super::split_message(&text, MAX_MSG_LEN) {
                                let _ = channel.send_message(recipient, &chunk).await;
                            }
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!(
                                "[whatsapp] Session broadcast lagged, skipped {n} messages"
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

        // Start per-session watchers for cross-transport sync
        self.spawn_session_watchers();

        let parsed_addr = addr
            .parse::<std::net::SocketAddr>()
            .unwrap_or_else(|_| "127.0.0.1:8444".parse().expect("fallback addr"));

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
        if msg.get("type").and_then(serde_json::Value::as_str) != Some("text") {
            return;
        }
        let Some(from) = msg.get("from").and_then(serde_json::Value::as_str) else {
            return;
        };
        let Some(text) = msg
            .get("text")
            .and_then(|t| t.get("body"))
            .and_then(serde_json::Value::as_str)
        else {
            return;
        };

        // execute_turn is synchronous -- run in blocking thread
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let from_s = from.to_string();
        let text_s = text.to_string();
        let response = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &from_s, &from_s, &text_s, "wa")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"));

        if response.is_empty() {
            return;
        }
        for chunk in super::split_message(&response, MAX_MSG_LEN) {
            let _ = self.send_message(from, &chunk).await;
        }
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

        let response = super::handle_message(state, store, from, name, body, "wa");
        if response.is_empty() {
            continue;
        }

        for chunk in super::split_message(&response, MAX_MSG_LEN) {
            let reply = serde_json::json!({"to": from, "text": chunk});
            let _ = writeln!(stdin, "{reply}");
            let _ = stdin.flush();
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
