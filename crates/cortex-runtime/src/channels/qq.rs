//! Official QQ Bot channel via Tencent QQ Bot API.

use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use base64::Engine;
use futures_util::{SinkExt, StreamExt};
use sha2::Digest;
use tokio::sync::Mutex;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::Message;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_SANDBOX_API_BASE: &str = "https://sandbox.api.sgroup.qq.com";
const QQ_TEXT_LIMIT: usize = 4_000;
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_mins(5);

const INTENT_GROUP_AND_C2C: u32 = 1 << 25;
const INTENT_INTERACTION: u32 = 1 << 26;
static QQ_RUSTLS_INIT: Once = Once::new();

const QQ_MSG_TYPE_TEXT: i64 = 0;
const QQ_MSG_TYPE_MARKDOWN: i64 = 2;
const QQ_MSG_TYPE_MEDIA: i64 = 7;

#[derive(Clone)]
struct AccessToken {
    value: String,
    expires_at: Instant,
}

#[derive(Clone)]
struct ReplyTarget {
    kind: ReplyTargetKind,
    source_message_id: Option<String>,
}

#[derive(Clone)]
enum ReplyTargetKind {
    C2c { openid: String },
    Group { group_openid: String },
}

pub struct QqChannelConfig {
    pub app_id: String,
    pub app_secret: String,
    pub sandbox: bool,
    pub markdown: bool,
    pub remove_at: bool,
    pub max_retry: usize,
}

pub struct QqChannel {
    app_id: String,
    app_secret: String,
    sandbox: bool,
    markdown: bool,
    remove_at: bool,
    max_retry: usize,
    client: reqwest::Client,
    store: ChannelStore,
    state: Arc<DaemonState>,
    token: Mutex<Option<AccessToken>>,
    session_watchers: Arc<std::sync::Mutex<std::collections::HashMap<String, watch::Sender<bool>>>>,
}

impl QqChannel {
    #[must_use]
    pub fn new(config: QqChannelConfig, store: ChannelStore, state: Arc<DaemonState>) -> Self {
        Self {
            app_id: config.app_id,
            app_secret: config.app_secret,
            sandbox: config.sandbox,
            markdown: config.markdown,
            remove_at: config.remove_at,
            max_retry: config.max_retry,
            client: reqwest::Client::new(),
            store,
            state,
            token: Mutex::new(None),
            session_watchers: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        }
    }

    const fn api_base(&self) -> &'static str {
        if self.sandbox {
            QQ_SANDBOX_API_BASE
        } else {
            QQ_API_BASE
        }
    }

    async fn ensure_access_token(&self) -> Result<String, String> {
        let cached = {
            let guard = self.token.lock().await;
            if let Some(token) = &*guard
                && Instant::now() + TOKEN_REFRESH_MARGIN < token.expires_at
            {
                Some(token.value.clone())
            } else {
                None
            }
        };
        if let Some(token) = cached {
            return Ok(token);
        }

        let response = self
            .client
            .post(QQ_TOKEN_URL)
            .json(&serde_json::json!({
                "appId": self.app_id,
                "clientSecret": self.app_secret,
            }))
            .send()
            .await
            .map_err(|e| format!("failed to request QQ access token: {e}"))?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("failed to decode QQ access token response: {e}"))?;
        if !status.is_success() {
            return Err(format!("QQ token request failed: {status} {body}"));
        }
        let access_token = body
            .get("access_token")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| format!("QQ token response missing access_token: {body}"))?
            .to_string();
        let expires_in = body
            .get("expires_in")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(7200);
        let mut guard = self.token.lock().await;
        *guard = Some(AccessToken {
            value: access_token.clone(),
            expires_at: Instant::now() + Duration::from_secs(expires_in),
        });
        drop(guard);
        Ok(access_token)
    }

    async fn gateway_url(&self, access_token: &str) -> Result<String, String> {
        let response = self
            .client
            .get(format!("{}/gateway", self.api_base()))
            .header("Authorization", format!("QQBot {access_token}"))
            .send()
            .await
            .map_err(|e| format!("failed to request QQ gateway url: {e}"))?;
        let status = response.status();
        let body: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("failed to decode QQ gateway response: {e}"))?;
        if !status.is_success() {
            return Err(format!("QQ gateway request failed: {status} {body}"));
        }
        body.get("url")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("QQ gateway response missing url: {body}"))
    }

    fn identify_payload(access_token: &str) -> serde_json::Value {
        serde_json::json!({
            "op": 2,
            "d": {
                "token": format!("QQBot {access_token}"),
                "intents": INTENT_GROUP_AND_C2C | INTENT_INTERACTION,
                "shard": [0, 1],
                "properties": {
                    "$os": std::env::consts::OS,
                    "$sdk": "cortex",
                    "$browser": "cortex",
                }
            }
        })
    }

    fn heartbeat_payload(seq: Option<i64>) -> serde_json::Value {
        serde_json::json!({
            "op": 1,
            "d": seq,
        })
    }

    pub async fn run_websocket(self: &Arc<Self>, mut shutdown: tokio::sync::watch::Receiver<bool>) {
        install_rustls_provider();
        self.spawn_session_watchers();
        self.spawn_subscription_reconciler(shutdown.clone());

        let mut attempts = 0usize;
        loop {
            if *shutdown.borrow() {
                break;
            }

            let access_token = match self.ensure_access_token().await {
                Ok(token) => token,
                Err(error) => {
                    tracing::error!("[qq] {error}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };
            let gateway_url = match self.gateway_url(&access_token).await {
                Ok(url) => url,
                Err(error) => {
                    tracing::error!("[qq] {error}");
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    continue;
                }
            };

            let (stream, next_attempts) = match self.connect_gateway(&gateway_url, attempts).await {
                Ok(parts) => parts,
                Err(next_attempts) => {
                    if next_attempts > self.max_retry {
                        tracing::error!("[qq] Reconnect attempts exhausted");
                        break;
                    }
                    tokio::time::sleep(Self::reconnect_delay(next_attempts)).await;
                    continue;
                }
            };
            attempts = next_attempts;

            if self
                .run_gateway_session(stream, &access_token, &mut shutdown)
                .await
            {
                return;
            }

            attempts += 1;
            if attempts > self.max_retry {
                tracing::error!("[qq] Reconnect attempts exhausted");
                break;
            }
            tokio::time::sleep(Self::reconnect_delay(attempts)).await;
        }
    }

    const fn reconnect_delay(attempt: usize) -> Duration {
        match attempt {
            0 | 1 => Duration::from_secs(1),
            2 => Duration::from_secs(2),
            3 => Duration::from_secs(5),
            4 => Duration::from_secs(10),
            _ => Duration::from_secs(30),
        }
    }

    async fn connect_gateway(
        &self,
        gateway_url: &str,
        attempts: usize,
    ) -> Result<
        (
            tokio_tungstenite::WebSocketStream<
                tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
            >,
            usize,
        ),
        usize,
    > {
        tracing::info!("[qq] Connecting to {gateway_url}");
        match tokio_tungstenite::connect_async(gateway_url).await {
            Ok((stream, _)) => Ok((stream, 0)),
            Err(error) => {
                let next_attempts = attempts + 1;
                tracing::error!("[qq] WebSocket connect failed: {error}");
                Err(next_attempts)
            }
        }
    }

    async fn run_gateway_session(
        &self,
        stream: tokio_tungstenite::WebSocketStream<
            tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
        >,
        access_token: &str,
        shutdown: &mut tokio::sync::watch::Receiver<bool>,
    ) -> bool {
        let (mut write, mut read) = stream.split();
        let mut seq = None::<i64>;
        let mut heartbeat = None::<tokio::time::Interval>;
        let identify = Self::identify_payload(access_token).to_string();

        loop {
            tokio::select! {
                _ = shutdown.changed() => {
                    if *shutdown.borrow() {
                        let _ = write.close().await;
                        return true;
                    }
                }
                () = async {
                    if let Some(interval) = &mut heartbeat {
                        interval.tick().await;
                    } else {
                        futures_util::future::pending::<()>().await;
                    }
                } => {
                    let payload = Self::heartbeat_payload(seq).to_string();
                    if write.send(Message::Text(payload.into())).await.is_err() {
                        break;
                    }
                }
                message = read.next() => {
                    let Some(message) = message else {
                        break;
                    };
                    let message = match message {
                        Ok(message) => message,
                        Err(error) => {
                            tracing::warn!("[qq] WebSocket read error: {error}");
                            break;
                        }
                    };
                    let Message::Text(text) = message else {
                        continue;
                    };
                    let Ok(payload) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if let Some(s) = payload.get("s").and_then(serde_json::Value::as_i64) {
                        seq = Some(s);
                    }
                    if !self.handle_gateway_payload(&mut write, &mut heartbeat, &identify, &payload).await {
                        break;
                    }
                }
            }
        }

        false
    }

    async fn handle_gateway_payload<
        S: futures_util::Sink<Message, Error = tokio_tungstenite::tungstenite::Error> + Unpin,
    >(
        &self,
        write: &mut S,
        heartbeat: &mut Option<tokio::time::Interval>,
        identify: &str,
        payload: &serde_json::Value,
    ) -> bool {
        match payload
            .get("op")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or(-1)
        {
            10 => {
                let interval_ms = payload
                    .get("d")
                    .and_then(|d| d.get("heartbeat_interval"))
                    .and_then(serde_json::Value::as_u64)
                    .unwrap_or(30_000);
                *heartbeat = Some(tokio::time::interval(Duration::from_millis(interval_ms)));
                write
                    .send(Message::Text(identify.to_owned().into()))
                    .await
                    .is_ok()
            }
            7 | 9 => false,
            0 => {
                if let Some(event_type) = payload.get("t").and_then(serde_json::Value::as_str)
                    && let Some(data) = payload.get("d")
                {
                    self.handle_dispatch(event_type, data).await;
                }
                true
            }
            _ => true,
        }
    }

    async fn handle_dispatch(&self, event_type: &str, data: &serde_json::Value) {
        match event_type {
            "C2C_MESSAGE_CREATE" | "GROUP_AT_MESSAGE_CREATE" => {
                let target = if event_type == "C2C_MESSAGE_CREATE" {
                    Self::extract_c2c_target(data)
                } else {
                    self.extract_group_target(data)
                };
                let Some((user_id, user_name, text, attachments, target)) = target else {
                    tracing::info!("[qq] Ignored dispatch event_type={event_type}");
                    return;
                };
                let attachments = self.prepare_inbound_attachments(&attachments).await;
                tracing::info!(
                    "[qq] inbound event_type={event_type} user_id={user_id} user_name={user_name:?} target={} text_len={} attachments={}",
                    target.kind.label(),
                    text.len(),
                    attachments.len()
                );
                self.handle_inbound_message(&user_id, &user_name, &text, &attachments, &target)
                    .await;
            }
            "INTERACTION_CREATE" => {
                self.handle_interaction(data).await;
            }
            _ => {
                tracing::info!("[qq] Ignored dispatch event_type={event_type}");
            }
        }
    }

    async fn handle_inbound_message(
        &self,
        user_id: &str,
        user_name: &str,
        text: &str,
        attachments: &[cortex_types::Attachment],
        target: &ReplyTarget,
    ) {
        if text.starts_with('/') {
            self.handle_slash_command(target, user_id, user_name, text)
                .await;
            return;
        }

        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let user_id_for_turn = user_id.to_string();
        let user_name_for_turn = user_name.to_string();
        let text_for_turn = text.to_string();
        let attachments_for_turn = attachments.to_vec();
        let events = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message_events(
                &state,
                &store,
                &user_id_for_turn,
                &user_name_for_turn,
                &text_for_turn,
                &attachments_for_turn,
                "qq",
            )
        })
        .await
        .unwrap_or_else(|e| vec![crate::daemon::BroadcastEvent::Error(format!("Error: {e}"))]);
        tracing::info!(
            "[qq] turn completed user_id={user_id} target={} events={}",
            target.kind.label(),
            events.len()
        );
        self.send_event_sequence(target, &events, 0).await;
    }

    async fn handle_slash_command(
        &self,
        target: &ReplyTarget,
        user_id: &str,
        user_name: &str,
        text: &str,
    ) {
        let bare_cmd = text.split_whitespace().next().unwrap_or(text);
        if bare_cmd == text.trim()
            && let Some(keyboard) = self.command_keyboard(bare_cmd)
        {
            let response = self.dispatch_slash_command(text, user_id, user_name).await;
            let msg_text = if response.is_empty() {
                bare_cmd.to_string()
            } else {
                response
            };
            let _ = self
                .send_text_with_keyboard(target, &msg_text, 1, self.markdown, &keyboard)
                .await;
            return;
        }

        if text == "/session switch" {
            let keyboard = self.session_switch_keyboard(user_id).await;
            let response = if keyboard.is_some() {
                "🗂️ Choose a session:".to_string()
            } else {
                "🗂️ No other sessions to switch to.".to_string()
            };
            if let Some(keyboard) = keyboard {
                let _ = self
                    .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                    .await;
            } else {
                let _ = self.send_text(target, &response, 1, self.markdown).await;
            }
            return;
        }

        let response = self.dispatch_slash_command(text, user_id, user_name).await;
        if !response.trim().is_empty() {
            let keyboard = self.root_keyboard_for_callback(text);
            if let Some(keyboard) = keyboard {
                let _ = self
                    .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                    .await;
            } else {
                let _ = self.send_text(target, &response, 1, self.markdown).await;
            }
        }
    }

    async fn dispatch_slash_command(&self, text: &str, user_id: &str, user_name: &str) -> String {
        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let uid = user_id.to_string();
        let uname = user_name.to_string();
        let cmd = text.to_string();
        tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message(&state, &store, &uid, &uname, &cmd, "qq")
        })
        .await
        .unwrap_or_else(|e| format!("Error: {e}"))
    }

    async fn session_switch_keyboard(&self, user_id: &str) -> Option<serde_json::Value> {
        let state = Arc::clone(&self.state);
        let actor = crate::daemon::DaemonState::channel_actor("qq", user_id);
        let current_session = state.resolve_actor_session(&actor);
        let sessions = tokio::task::spawn_blocking(move || state.visible_sessions(&actor))
            .await
            .unwrap_or_default();
        qq_session_switch_keyboard(&sessions, Some(&current_session))
    }

    async fn handle_interaction(&self, data: &serde_json::Value) {
        let interaction_id = data
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let button_data = data
            .get("data")
            .and_then(|value| value.get("resolved"))
            .and_then(|value| value.get("button_data"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        if interaction_id.is_empty() || button_data.is_empty() {
            return;
        }

        let token = match self.ensure_access_token().await {
            Ok(token) => token,
            Err(error) => {
                tracing::warn!("[qq] interaction token unavailable: {error}");
                return;
            }
        };
        let _ = self
            .acknowledge_interaction(&token, interaction_id)
            .await
            .map_err(|error| tracing::warn!("[qq] interaction ack failed: {error}"));

        let target = if let Some(group_openid) =
            data.get("group_openid").and_then(serde_json::Value::as_str)
        {
            ReplyTarget {
                kind: ReplyTargetKind::Group {
                    group_openid: group_openid.to_string(),
                },
                source_message_id: None,
            }
        } else if let Some(openid) = data.get("user_openid").and_then(serde_json::Value::as_str) {
            ReplyTarget {
                kind: ReplyTargetKind::C2c {
                    openid: openid.to_string(),
                },
                source_message_id: None,
            }
        } else {
            return;
        };

        let user_id = data
            .get("group_member_openid")
            .or_else(|| data.get("user_openid"))
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let user_name = user_id;

        if let Some(action) = parse_qq_permission_callback(button_data) {
            self.handle_permission_interaction(&target, action).await;
            return;
        }

        if button_data == "/session switch" {
            let keyboard = self.session_switch_keyboard(user_id).await;
            let response = if keyboard.is_some() {
                "🗂️ Choose a session:".to_string()
            } else {
                "🗂️ No other sessions to switch to.".to_string()
            };
            if let Some(keyboard) = keyboard {
                let _ = self
                    .send_text_with_keyboard(&target, &response, 1, self.markdown, &keyboard)
                    .await;
            } else {
                let _ = self.send_text(&target, &response, 1, self.markdown).await;
            }
            return;
        }

        let response = self
            .dispatch_slash_command(button_data, user_id, user_name)
            .await;
        let keyboard = self.root_keyboard_for_callback(button_data);
        if let Some(keyboard) = keyboard {
            let _ = self
                .send_text_with_keyboard(&target, &response, 1, self.markdown, &keyboard)
                .await;
        } else if !response.trim().is_empty() {
            let _ = self.send_text(&target, &response, 1, self.markdown).await;
        }
    }

    async fn handle_permission_interaction(
        &self,
        target: &ReplyTarget,
        action: QqPermissionCallbackAction<'_>,
    ) {
        let (command, pending_id) = match action {
            QqPermissionCallbackAction::Approve(id) => (format!("/approve {id}"), id),
            QqPermissionCallbackAction::Deny(id) => (format!("/deny {id}"), id),
            QqPermissionCallbackAction::Refresh(id) => (String::new(), id),
        };

        let response = match action {
            QqPermissionCallbackAction::Refresh(id) => {
                self.state.pending_permission_info(id).map_or_else(
                    || qq_permission_resolved_text(id),
                    |info| info.prompt_text(),
                )
            }
            QqPermissionCallbackAction::Approve(_) | QqPermissionCallbackAction::Deny(_) => {
                self.state.dispatch_command(&command)
            }
        };

        if self.state.pending_permission_info(pending_id).is_some() {
            let keyboard = qq_permission_keyboard(pending_id);
            let _ = self
                .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                .await;
        } else {
            let keyboard = qq_permission_resolved_keyboard(pending_id);
            let _ = self
                .send_text_with_keyboard(target, &response, 1, self.markdown, &keyboard)
                .await;
        }
    }

    fn command_keyboard(&self, cmd: &str) -> Option<serde_json::Value> {
        qq_command_keyboard(cmd, self.state.config().risk.auto_approve_up_to)
    }

    fn root_keyboard_for_callback(&self, data: &str) -> Option<serde_json::Value> {
        qq_root_keyboard_for_callback(data, self.state.config().risk.auto_approve_up_to)
    }

    async fn acknowledge_interaction(
        &self,
        access_token: &str,
        interaction_id: &str,
    ) -> Result<(), String> {
        let response = self
            .client
            .put(format!("{}/interactions/{interaction_id}", self.api_base()))
            .header("Authorization", format!("QQBot {access_token}"))
            .json(&serde_json::json!({ "code": 0 }))
            .send()
            .await
            .map_err(|e| format!("failed to ack QQ interaction: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ interaction ack failed: {status} {body}"))
        }
    }

    fn extract_c2c_target(
        data: &serde_json::Value,
    ) -> Option<(
        String,
        String,
        String,
        Vec<cortex_types::Attachment>,
        ReplyTarget,
    )> {
        let author = data.get("author")?;
        let user_id = author
            .get("user_openid")
            .or_else(|| author.get("id"))
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let content = data
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        let attachments = Self::extract_raw_attachments(data);
        let message_id = qq_reply_message_id(data);
        Some((
            user_id.clone(),
            user_id.clone(),
            content,
            attachments,
            ReplyTarget {
                kind: ReplyTargetKind::C2c { openid: user_id },
                source_message_id: message_id,
            },
        ))
    }

    fn extract_group_target(
        &self,
        data: &serde_json::Value,
    ) -> Option<(
        String,
        String,
        String,
        Vec<cortex_types::Attachment>,
        ReplyTarget,
    )> {
        let author = data.get("author")?;
        let user_id = author
            .get("member_openid")
            .or_else(|| author.get("id"))
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let user_name = author
            .get("username")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(&user_id)
            .to_string();
        let mut content = data
            .get("content")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        if self.remove_at {
            content = strip_self_mentions(&content, data.get("mentions"));
        }
        let attachments = Self::extract_raw_attachments(data);
        let group_openid = data
            .get("group_openid")
            .and_then(serde_json::Value::as_str)?
            .to_string();
        let message_id = qq_reply_message_id(data);
        Some((
            user_id,
            user_name,
            content,
            attachments,
            ReplyTarget {
                kind: ReplyTargetKind::Group { group_openid },
                source_message_id: message_id,
            },
        ))
    }

    fn extract_raw_attachments(data: &serde_json::Value) -> Vec<cortex_types::Attachment> {
        data.get("attachments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|att| {
                let mime_type = att
                    .get("content_type")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("application/octet-stream");
                let file_name = att
                    .get("filename")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string);
                let url = att
                    .get("voice_wav_url")
                    .and_then(serde_json::Value::as_str)
                    .or_else(|| att.get("url").and_then(serde_json::Value::as_str))?;
                let media_type =
                    super::infer_attachment_media_type(mime_type, file_name.as_deref());
                Some(cortex_types::Attachment {
                    media_type,
                    mime_type: mime_type.to_string(),
                    url: url.to_string(),
                    caption: att
                        .get("asr_refer_text")
                        .and_then(serde_json::Value::as_str)
                        .filter(|s| !s.trim().is_empty())
                        .map(str::to_string)
                        .or(file_name),
                    size: att.get("size").and_then(serde_json::Value::as_u64),
                })
            })
            .collect()
    }

    async fn prepare_inbound_attachments(
        &self,
        attachments: &[cortex_types::Attachment],
    ) -> Vec<cortex_types::Attachment> {
        let mut prepared = Vec::with_capacity(attachments.len());
        for attachment in attachments.iter().cloned() {
            match self.materialize_attachment(attachment).await {
                Ok(local) => {
                    let enriched =
                        super::enrich_inbound_attachment(&self.state, &self.client, local).await;
                    prepared.push(enriched);
                }
                Err(error) => {
                    tracing::warn!("[qq] attachment materialize failed: {error}");
                }
            }
        }
        prepared
    }

    async fn materialize_attachment(
        &self,
        mut attachment: cortex_types::Attachment,
    ) -> Result<cortex_types::Attachment, String> {
        if !is_remote_media_url(&attachment.url) {
            return Ok(attachment);
        }
        let response = self
            .client
            .get(&attachment.url)
            .send()
            .await
            .map_err(|e| format!("download QQ attachment failed: {e}"))?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| format!("read QQ attachment failed: {e}"))?;
        let hash_full = hex::encode(sha2::Sha256::digest(&bytes));
        let hash = &hash_full[..16];
        let ext = attachment
            .mime_type
            .split('/')
            .nth(1)
            .filter(|ext| !ext.is_empty())
            .unwrap_or("bin");
        let blob_dir =
            cortex_kernel::CortexPaths::from_instance_home(self.state.home()).blobs_dir();
        std::fs::create_dir_all(&blob_dir).map_err(|e| format!("create blob dir failed: {e}"))?;
        let local = blob_dir.join(format!("{hash}.{ext}"));
        std::fs::write(&local, &bytes).map_err(|e| format!("write QQ attachment failed: {e}"))?;
        attachment.size = Some(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        attachment.url = local.to_string_lossy().to_string();
        Ok(attachment)
    }

    async fn send_event_sequence(
        &self,
        target: &ReplyTarget,
        events: &[crate::daemon::BroadcastEvent],
        initial_msg_seq: u32,
    ) {
        tracing::info!(
            "[qq] sending event sequence target={} source_message={} events={}",
            target.kind.label(),
            target.source_message_id.is_some(),
            events.len()
        );
        let mut msg_seq = initial_msg_seq;
        for event in events {
            if let Some((text, keyboard)) = qq_permission_delivery(event) {
                msg_seq += 1;
                if let Err(error) = self
                    .send_text_with_keyboard(target, &text, msg_seq, self.markdown, &keyboard)
                    .await
                {
                    tracing::error!("[qq] permission send failed: {error}");
                }
                continue;
            }
            tracing::info!(
                "[qq] outbound event={} target={} text_len={}",
                event.kind_name(),
                target.kind.label(),
                event.plain_text().len()
            );
            for item in super::channel_delivery_items(
                event,
                super::ChannelCapabilities::with_media(
                    if self.markdown {
                        super::ChannelTextCapability::Markdown
                    } else {
                        super::ChannelTextCapability::Plain
                    },
                    super::ChannelCapabilities::IMAGE
                        | super::ChannelCapabilities::AUDIO
                        | super::ChannelCapabilities::VIDEO
                        | super::ChannelCapabilities::FILE,
                ),
            ) {
                match item {
                    super::ChannelDeliveryItem::Text { text, markdown } => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        for chunk in super::split_message(&text, QQ_TEXT_LIMIT) {
                            msg_seq += 1;
                            if let Err(error) =
                                self.send_text(target, &chunk, msg_seq, markdown).await
                            {
                                tracing::error!("[qq] send failed: {error}");
                                return;
                            }
                        }
                    }
                    super::ChannelDeliveryItem::Media { attachment } => {
                        msg_seq += 1;
                        if let Err(error) = self.send_media(target, &attachment, msg_seq).await {
                            tracing::error!("[qq] media send failed: {error}");
                            return;
                        }
                    }
                }
            }
        }
    }

    async fn send_text(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
    ) -> Result<(), String> {
        self.send_text_inner(target, text, msg_seq, markdown, None)
            .await
    }

    async fn send_text_with_keyboard(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
        keyboard: &serde_json::Value,
    ) -> Result<(), String> {
        self.send_text_inner(target, text, msg_seq, markdown, Some(keyboard))
            .await
    }

    async fn send_text_inner(
        &self,
        target: &ReplyTarget,
        text: &str,
        msg_seq: u32,
        markdown: bool,
        keyboard: Option<&serde_json::Value>,
    ) -> Result<(), String> {
        let token = self.ensure_access_token().await?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/messages"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/messages")
            }
        };
        let mut body = if markdown {
            serde_json::json!({
                "markdown": {"content": text},
                "msg_type": QQ_MSG_TYPE_MARKDOWN,
                "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
            })
        } else {
            serde_json::json!({
                "content": text,
                "msg_type": QQ_MSG_TYPE_TEXT,
                "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
            })
        };
        if let Some(msg_id) = &target.source_message_id {
            body["msg_id"] = serde_json::Value::String(msg_id.clone());
        }
        if let Some(keyboard) = keyboard {
            body["keyboard"] = keyboard.clone();
        }
        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to send QQ message: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ send failed: {status} {body}"))
        }
    }

    async fn send_media(
        &self,
        target: &ReplyTarget,
        attachment: &cortex_types::Attachment,
        msg_seq: u32,
    ) -> Result<(), String> {
        let token = self.ensure_access_token().await?;
        let file_info = self.upload_media(&token, target, attachment).await?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/messages"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/messages")
            }
        };
        let mut body = serde_json::json!({
            "msg_type": QQ_MSG_TYPE_MEDIA,
            "media": {"file_info": file_info},
            "msg_seq": if target.source_message_id.is_some() { msg_seq } else { 1 },
        });
        if let Some(caption) = attachment.caption.as_deref().map(str::trim)
            && !caption.is_empty()
        {
            body["content"] = serde_json::Value::String(caption.to_string());
        }
        if let Some(msg_id) = &target.source_message_id {
            body["msg_id"] = serde_json::Value::String(msg_id.clone());
        }

        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to send QQ media message: {e}"))?;
        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            Err(format!("QQ media send failed: {status} {body}"))
        }
    }

    async fn upload_media(
        &self,
        token: &str,
        target: &ReplyTarget,
        attachment: &cortex_types::Attachment,
    ) -> Result<String, String> {
        let file_type = qq_media_type(attachment)?;
        let path = match &target.kind {
            ReplyTargetKind::C2c { openid } => format!("/v2/users/{openid}/files"),
            ReplyTargetKind::Group { group_openid } => {
                format!("/v2/groups/{group_openid}/files")
            }
        };

        let mut body = serde_json::json!({
            "file_type": file_type,
            "srv_send_msg": false,
        });

        if is_remote_media_url(&attachment.url) {
            body["url"] = serde_json::Value::String(attachment.url.clone());
        } else {
            let data = std::fs::read(&attachment.url)
                .map_err(|e| format!("failed to read attachment {}: {e}", attachment.url))?;
            let encoded = base64::engine::general_purpose::STANDARD.encode(data);
            body["file_data"] = serde_json::Value::String(encoded);
            if attachment.media_type == "file"
                && let Some(file_name) = std::path::Path::new(&attachment.url)
                    .file_name()
                    .and_then(|name| name.to_str())
            {
                body["file_name"] = serde_json::Value::String(file_name.to_string());
            }
        }

        let response = self
            .client
            .post(format!("{}{}", self.api_base(), path))
            .header("Authorization", format!("QQBot {token}"))
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("failed to upload QQ media: {e}"))?;
        let status = response.status();
        let payload: serde_json::Value = response
            .json()
            .await
            .map_err(|e| format!("failed to decode QQ media upload response: {e}"))?;
        if !status.is_success() {
            return Err(format!("QQ media upload failed: {status} {payload}"));
        }
        payload
            .get("file_info")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("QQ media upload response missing file_info: {payload}"))
    }

    fn spawn_session_watchers(self: &Arc<Self>) {
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
                let active = channel.state.resolve_actor_session(&actor);
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
                            let new_active = channel.state.resolve_actor_session(&actor);
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

impl ReplyTargetKind {
    const fn label(&self) -> &'static str {
        match self {
            Self::C2c { .. } => "c2c",
            Self::Group { .. } => "group",
        }
    }
}

fn qq_media_type(attachment: &cortex_types::Attachment) -> Result<i64, String> {
    match attachment.media_type.as_str() {
        "image" => Ok(1),
        "video" => Ok(2),
        "audio" => Ok(3),
        "file" => Ok(4),
        other => Err(format!("unsupported QQ media type: {other}")),
    }
}

fn is_remote_media_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("data:")
}

trait BroadcastEventExt {
    fn kind_name(&self) -> &'static str;
}

impl BroadcastEventExt for crate::daemon::BroadcastEvent {
    fn kind_name(&self) -> &'static str {
        match self {
            Self::Text(_) => "text",
            Self::Boundary => "boundary",
            Self::Observer { .. } => "observer",
            Self::Trace { .. } => "trace",
            Self::Done { .. } => "done",
            Self::Error(_) => "error",
            Self::PermissionRequested(_) => "permission",
        }
    }
}

#[derive(Clone, Copy)]
enum QqPermissionCallbackAction<'a> {
    Approve(&'a str),
    Deny(&'a str),
    Refresh(&'a str),
}

fn parse_qq_permission_callback(data: &str) -> Option<QqPermissionCallbackAction<'_>> {
    let mut parts = data.splitn(3, ':');
    let prefix = parts.next()?;
    let action = parts.next()?;
    let id = parts.next()?;
    if prefix != "perm" || id.is_empty() {
        return None;
    }
    match action {
        "approve" => Some(QqPermissionCallbackAction::Approve(id)),
        "deny" => Some(QqPermissionCallbackAction::Deny(id)),
        "refresh" => Some(QqPermissionCallbackAction::Refresh(id)),
        _ => None,
    }
}

fn qq_button(label: &str, visited_label: &str, data: &str, style: i64) -> serde_json::Value {
    serde_json::json!({
        "id": data,
        "render_data": {
            "label": label,
            "visited_label": visited_label,
            "style": style,
        },
        "action": {
            "type": 1,
            "data": data,
            "permission": { "type": 2 },
            "click_limit": 1,
        },
    })
}

fn qq_command_keyboard(
    cmd: &str,
    current_mode: cortex_types::RiskLevel,
) -> Option<serde_json::Value> {
    match cmd {
        "/help" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Status", cmd, "/status"), "Status", "/status", 1),
                        qq_button(&qq_nav_button_label("Permission", cmd, "/permission"), "Permission", "/permission", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Sessions", cmd, "/session"), "Sessions", "/session", 1),
                        qq_button(&qq_nav_button_label("Config", cmd, "/config"), "Config", "/config", 1),
                    ]},
                    {"buttons": [
                        qq_button("Stop", "Stopping", "/stop", 0),
                    ]},
                ]
            }
        })),
        "/status" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("Refresh", "Refreshed", "/status", 1),
                        qq_button(&qq_nav_button_label("Permission", cmd, "/permission"), "Permission", "/permission", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Sessions", cmd, "/session"), "Sessions", "/session", 1),
                        qq_button(&qq_nav_button_label("Config", cmd, "/config"), "Config", "/config", 1),
                    ]},
                ]
            }
        })),
        "/permission" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button(&qq_permission_button_label("Strict", current_mode, cortex_types::RiskLevel::Allow), "Strict", "/permission strict", qq_permission_button_style(current_mode, cortex_types::RiskLevel::Allow)),
                        qq_button(&qq_permission_button_label("Balanced", current_mode, cortex_types::RiskLevel::Review), "Balanced", "/permission balanced", qq_permission_button_style(current_mode, cortex_types::RiskLevel::Review)),
                        qq_button(&qq_permission_button_label("Open", current_mode, cortex_types::RiskLevel::RequireConfirmation), "Open", "/permission open", qq_permission_button_style(current_mode, cortex_types::RiskLevel::RequireConfirmation)),
                    ]},
                    {"buttons": [
                        qq_button("Refresh", "Refreshed", "/permission", 1),
                        qq_button(&qq_nav_button_label("Status", cmd, "/status"), "Status", "/status", 1),
                    ]},
                ]
            }
        })),
        "/session" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("List", "Listed", "/session list", 1),
                        qq_button("New", "Created", "/session new", 1),
                    ]},
                    {"buttons": [
                        qq_button("Switch", "Switch", "/session switch", 1),
                        qq_button("End", "Ended", "/quit", 0),
                    ]},
                ]
            }
        })),
        "/config" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("API", "API", "/config get api", 1),
                        qq_button("Memory", "Memory", "/config get memory", 1),
                        qq_button("Tools", "Tools", "/config get tools", 1),
                    ]},
                    {"buttons": [
                        qq_button("Web", "Web", "/config get web", 1),
                        qq_button("Skills", "Skills", "/config get skills", 1),
                        qq_button("Summary", "Summary", "/config list", 1),
                    ]},
                ]
            }
        })),
        _ => None,
    }
}

fn qq_root_keyboard_for_callback(
    data: &str,
    current_mode: cortex_types::RiskLevel,
) -> Option<serde_json::Value> {
    if data.starts_with("/help") || data.starts_with("/stop") {
        qq_command_keyboard("/help", current_mode)
    } else if data.starts_with("/status") {
        qq_command_keyboard("/status", current_mode)
    } else if data.starts_with("/permission") {
        qq_command_keyboard("/permission", current_mode)
    } else if data.starts_with("/session") || data == "/quit" {
        qq_command_keyboard("/session", current_mode)
    } else if data.starts_with("/config") {
        qq_command_keyboard("/config", current_mode)
    } else {
        None
    }
}

fn qq_permission_button_label(
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

fn qq_permission_button_style(
    current_mode: cortex_types::RiskLevel,
    button_mode: cortex_types::RiskLevel,
) -> i64 {
    i64::from(current_mode == button_mode)
}

fn qq_nav_button_label(label: &str, current_cmd: &str, button_cmd: &str) -> String {
    if current_cmd == button_cmd {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn qq_reply_message_id(data: &serde_json::Value) -> Option<String> {
    ["msg_id", "message_id", "id", "event_id"]
        .into_iter()
        .find_map(|key| data.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn qq_session_switch_keyboard(
    sessions: &[cortex_types::SessionMetadata],
    current_session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut rows = Vec::new();
    for session in sessions
        .iter()
        .filter(|session| {
            current_session_id.is_none_or(|current| session.id.to_string() != current)
        })
        .take(10)
    {
        let id = session.id.to_string();
        let short_id = &id[..id.len().min(8)];
        let label = session.name.as_deref().unwrap_or(short_id);
        rows.push(serde_json::json!({
            "buttons": [
                qq_button(
                    &format!("{label} ({})", session.turn_count),
                    label,
                    &format!("/session switch {id}"),
                    1,
                )
            ]
        }));
    }
    if rows.is_empty() {
        return None;
    }
    rows.push(serde_json::json!({
        "buttons": [qq_button("Back", "Back", "/session", 1)]
    }));
    Some(serde_json::json!({ "content": { "rows": rows } }))
}

fn qq_permission_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "content": {
            "rows": [
                {"buttons": [
                    qq_button("Approve", "Approved", &format!("perm:approve:{id}"), 1),
                    qq_button("Deny", "Denied", &format!("perm:deny:{id}"), 0),
                    qq_button("Refresh", "Refreshed", &format!("perm:refresh:{id}"), 1),
                ]}
            ]
        }
    })
}

fn qq_permission_resolved_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "content": {
            "rows": [
                {"buttons": [
                    qq_button("Refresh", "Refreshed", &format!("perm:refresh:{id}"), 1),
                ]}
            ]
        }
    })
}

fn qq_permission_resolved_text(id: &str) -> String {
    format!("✅ Permission request {id} has already been resolved.")
}

fn qq_permission_delivery(
    event: &crate::daemon::BroadcastEvent,
) -> Option<(String, serde_json::Value)> {
    match event {
        crate::daemon::BroadcastEvent::PermissionRequested(info) => {
            Some((info.prompt_text(), qq_permission_keyboard(&info.id)))
        }
        crate::daemon::BroadcastEvent::Observer { source, content } if source == "permission" => {
            parse_permission_prompt_id(content)
                .map(|id| (content.clone(), qq_permission_keyboard(id)))
        }
        _ => None,
    }
}

fn parse_permission_prompt_id(prompt: &str) -> Option<&str> {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix("Approve: /approve "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn install_rustls_provider() {
    QQ_RUSTLS_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}

fn strip_self_mentions(text: &str, mentions: Option<&serde_json::Value>) -> String {
    let mut cleaned = text.to_string();
    let Some(mentions) = mentions.and_then(serde_json::Value::as_array) else {
        return cleaned.trim().to_string();
    };
    for mention in mentions {
        let openid = mention
            .get("member_openid")
            .or_else(|| mention.get("id"))
            .or_else(|| mention.get("user_openid"))
            .and_then(serde_json::Value::as_str);
        let Some(openid) = openid else {
            continue;
        };
        if mention.get("is_you").and_then(serde_json::Value::as_bool) == Some(true) {
            cleaned = cleaned.replace(&format!("<@{openid}>"), "");
            cleaned = cleaned.replace(&format!("<@!{openid}>"), "");
        } else if let Some(name) = mention
            .get("nickname")
            .or_else(|| mention.get("username"))
            .and_then(serde_json::Value::as_str)
        {
            cleaned = cleaned.replace(&format!("<@{openid}>"), &format!("@{name}"));
            cleaned = cleaned.replace(&format!("<@!{openid}>"), &format!("@{name}"));
        }
    }
    cleaned.trim().to_string()
}
