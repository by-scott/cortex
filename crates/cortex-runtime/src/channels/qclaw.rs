//! `QClaw` channel adapter for Tencent Weixin iLink / `ClawBot`.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use tokio::sync::watch;

use crate::daemon::DaemonState;

use super::store::ChannelStore;

const DEFAULT_BASE_URL: &str = "https://ilinkai.weixin.qq.com";
const DEFAULT_CDN_BASE_URL: &str = "https://novac2c.cdn.weixin.qq.com/c2c";
const DEFAULT_BOT_TYPE: &str = "3";
const ILINK_APP_ID: &str = "bot";
const QCLAW_TEXT_LIMIT: usize = 2_000;
const API_TIMEOUT: Duration = Duration::from_secs(15);
const LONG_POLL_TIMEOUT: Duration = Duration::from_secs(40);
const QR_POLL_TIMEOUT: Duration = Duration::from_secs(40);
const QR_LOGIN_TIMEOUT: Duration = Duration::from_mins(8);
const SESSION_EXPIRED: i64 = -14;

static CLIENT_ID_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone)]
pub struct QclawChannelConfig {
    pub token: String,
    pub base_url: String,
    pub account_id: String,
    pub user_id: String,
    pub route_tag: Option<String>,
    pub bot_agent: String,
    pub max_retry: usize,
}

#[derive(Debug, Clone, Default)]
pub struct QclawLoginOptions {
    pub base_url: Option<String>,
    pub route_tag: Option<String>,
    pub bot_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QclawLoginCredentials {
    pub token: String,
    pub base_url: String,
    pub account_id: String,
    pub user_id: Option<String>,
}

pub struct QclawChannel {
    config: QclawChannelConfig,
    client: reqwest::Client,
    poll_client: reqwest::Client,
    store: ChannelStore,
    state: Arc<DaemonState>,
    context_tokens: Arc<Mutex<HashMap<String, String>>>,
    session_watchers: Arc<Mutex<HashMap<String, watch::Sender<bool>>>>,
}

#[derive(Debug, Serialize, Deserialize)]
struct SyncState {
    get_updates_buf: String,
}

#[derive(Debug, Deserialize)]
struct GetUpdatesResp {
    ret: Option<i64>,
    errcode: Option<i64>,
    errmsg: Option<String>,
    msgs: Option<Vec<QclawMessage>>,
    get_updates_buf: Option<String>,
    longpolling_timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct QclawMessage {
    from_user_id: Option<String>,
    context_token: Option<String>,
    item_list: Option<Vec<QclawMessageItem>>,
}

#[derive(Debug, Deserialize)]
struct QclawMessageItem {
    #[serde(rename = "type")]
    item_type: Option<i64>,
    text_item: Option<QclawTextItem>,
    voice_item: Option<QclawVoiceItem>,
    file_item: Option<QclawFileItem>,
}

#[derive(Debug, Deserialize)]
struct QclawTextItem {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QclawVoiceItem {
    text: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QclawFileItem {
    file_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiStatus {
    ret: Option<i64>,
    errcode: Option<i64>,
    errmsg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrCodeResp {
    qrcode: Option<String>,
    qrcode_img_content: Option<String>,
}

#[derive(Debug, Deserialize)]
struct QrStatusResp {
    status: String,
    bot_token: Option<String>,
    ilink_bot_id: Option<String>,
    ilink_user_id: Option<String>,
    baseurl: Option<String>,
    redirect_host: Option<String>,
}

impl QclawChannelConfig {
    #[must_use]
    pub fn from_auth(auth: &serde_json::Value) -> Option<Self> {
        let token = auth
            .get("token")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        if token.is_empty() {
            return None;
        }

        let base_url = auth
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty())
            .unwrap_or(DEFAULT_BASE_URL)
            .trim()
            .to_string();
        let account_id = auth
            .get("account_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let user_id = auth
            .get("user_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .trim()
            .to_string();
        let route_tag = auth
            .get("route_tag")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned);
        let bot_agent = auth
            .get("bot_agent")
            .and_then(serde_json::Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map_or_else(|| default_bot_agent().to_string(), ToOwned::to_owned);
        let max_retry = auth
            .get("max_retry")
            .and_then(serde_json::Value::as_u64)
            .and_then(|value| usize::try_from(value).ok())
            .unwrap_or(10);

        Some(Self {
            token,
            base_url,
            account_id,
            user_id,
            route_tag,
            bot_agent,
            max_retry,
        })
    }
}

impl QclawChannel {
    #[must_use]
    pub fn new(config: QclawChannelConfig, store: ChannelStore, state: Arc<DaemonState>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(API_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let poll_client = reqwest::Client::builder()
            .timeout(LONG_POLL_TIMEOUT)
            .build()
            .unwrap_or_else(|_| reqwest::Client::new());
        let context_tokens = Arc::new(Mutex::new(load_context_tokens(store.dir())));
        Self {
            config,
            client,
            poll_client,
            store,
            state,
            context_tokens,
            session_watchers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub async fn run_long_poll(self: &Arc<Self>, mut shutdown: watch::Receiver<bool>) {
        self.spawn_session_watchers();
        self.spawn_subscription_reconciler(shutdown.clone());

        let mut sync = load_sync_state(self.store.dir());
        let mut timeout = LONG_POLL_TIMEOUT;
        let mut failures = 0usize;

        tracing::info!(
            "[qclaw] long-poll started account={}",
            self.config.account_id
        );
        loop {
            if *shutdown.borrow() {
                break;
            }

            let poll = self.get_updates(&sync.get_updates_buf, timeout);
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        break;
                    }
                }
                result = poll => {
                    match result {
                        Ok(resp) => {
                            failures = 0;
                            if let Some(next) = resp.longpolling_timeout_ms.and_then(duration_from_millis) {
                                timeout = next + Duration::from_secs(5);
                            }
                            if Self::response_is_session_expired(&resp) {
                                tracing::error!("[qclaw] session expired; run `cortex channel qclaw login`");
                                tokio::time::sleep(Duration::from_mins(1)).await;
                                continue;
                            }
                            if Self::response_is_error(&resp) {
                                tracing::warn!(
                                    "[qclaw] getupdates failed ret={:?} errcode={:?} errmsg={:?}",
                                    resp.ret,
                                    resp.errcode,
                                    resp.errmsg
                                );
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                continue;
                            }
                            if let Some(buf) = resp.get_updates_buf.as_deref().filter(|buf| !buf.is_empty()) {
                                sync.get_updates_buf = buf.to_string();
                                save_sync_state(self.store.dir(), &sync);
                            }
                            for message in resp.msgs.unwrap_or_default() {
                                self.process_message(message).await;
                            }
                        }
                        Err(error) => {
                            failures += 1;
                            tracing::warn!("[qclaw] getupdates error ({failures}): {error}");
                            if failures > self.config.max_retry {
                                tracing::error!("[qclaw] reconnect attempts exhausted");
                                break;
                            }
                            tokio::time::sleep(retry_delay(failures)).await;
                        }
                    }
                }
            }
        }
        self.clear_session_watchers();
        tracing::info!("[qclaw] long-poll stopped");
    }

    async fn get_updates(
        &self,
        sync_buf: &str,
        timeout: Duration,
    ) -> Result<GetUpdatesResp, String> {
        let body = serde_json::json!({
            "get_updates_buf": sync_buf,
            "base_info": self.base_info(),
        });
        let response = self
            .request(
                self.poll_client
                    .post(api_url(&self.config.base_url, "ilink/bot/getupdates")),
            )
            .timeout(timeout)
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        decode_json_response(response, "getupdates").await
    }

    fn response_is_session_expired(resp: &GetUpdatesResp) -> bool {
        resp.errcode == Some(SESSION_EXPIRED) || resp.ret == Some(SESSION_EXPIRED)
    }

    fn response_is_error(resp: &GetUpdatesResp) -> bool {
        resp.ret.is_some_and(|ret| ret != 0) || resp.errcode.is_some_and(|err| err != 0)
    }

    async fn process_message(self: &Arc<Self>, message: QclawMessage) {
        let Some(from_user_id) = message
            .from_user_id
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
        else {
            return;
        };

        if let Some(token) = message
            .context_token
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            self.set_context_token(&from_user_id, token);
        }

        let text = inbound_text_from_items(message.item_list.as_deref());
        if text.trim().is_empty() {
            return;
        }

        let state = Arc::clone(&self.state);
        let store_dir = self.store.dir().to_path_buf();
        let user_id = from_user_id.clone();
        let events = tokio::task::spawn_blocking(move || {
            let store = ChannelStore::open_dir(store_dir);
            super::handle_message_events(&state, &store, &user_id, &user_id, &text, &[], "qclaw")
        })
        .await
        .unwrap_or_else(|error| vec![crate::daemon::BroadcastEvent::Error(format!("{error}"))]);

        self.send_event_sequence(&from_user_id, &events).await;
    }

    async fn send_event_sequence(&self, to: &str, events: &[crate::daemon::BroadcastEvent]) {
        for event in events {
            for item in super::channel_delivery_items(
                event,
                super::ChannelCapabilities::text_only(super::ChannelTextCapability::Plain),
            ) {
                match item {
                    super::ChannelDeliveryItem::Text { text, .. } => {
                        for chunk in super::split_message(&text, QCLAW_TEXT_LIMIT) {
                            if chunk.trim().is_empty() {
                                continue;
                            }
                            if let Err(error) = self.send_text(to, &chunk).await {
                                tracing::error!("[qclaw] send failed to={to}: {error}");
                                return;
                            }
                        }
                    }
                    super::ChannelDeliveryItem::Media { .. } => {}
                }
            }
        }
    }

    async fn send_text(&self, to: &str, text: &str) -> Result<(), String> {
        let context_token = self
            .context_token(to)
            .ok_or_else(|| format!("missing context token for {to}"))?;
        let body = serde_json::json!({
            "msg": {
                "from_user_id": "",
                "to_user_id": to,
                "client_id": generate_client_id(),
                "message_type": 2,
                "message_state": 2,
                "context_token": context_token,
                "item_list": [{
                    "type": 1,
                    "text_item": {"text": text},
                }],
            },
            "base_info": self.base_info(),
        });
        let response = self
            .request(
                self.client
                    .post(api_url(&self.config.base_url, "ilink/bot/sendmessage")),
            )
            .json(&body)
            .send()
            .await
            .map_err(|error| format!("request failed: {error}"))?;
        let response_status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| format!("read response failed: {error}"))?;
        if !response_status.is_success() {
            return Err(format!("sendmessage failed: {response_status} {body}"));
        }
        if body.trim().is_empty() {
            return Ok(());
        }
        let status: ApiStatus = serde_json::from_str(&body).unwrap_or(ApiStatus {
            ret: None,
            errcode: None,
            errmsg: None,
        });
        if status.ret.is_some_and(|ret| ret != 0) || status.errcode.is_some_and(|err| err != 0) {
            return Err(format!(
                "ret={:?} errcode={:?} errmsg={:?}",
                status.ret, status.errcode, status.errmsg
            ));
        }
        Ok(())
    }

    fn request(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let mut builder = builder
            .header("Content-Type", "application/json")
            .header("AuthorizationType", "ilink_bot_token")
            .header("Authorization", format!("Bearer {}", self.config.token))
            .header("X-WECHAT-UIN", random_wechat_uin())
            .header("iLink-App-Id", ILINK_APP_ID)
            .header("iLink-App-ClientVersion", client_version_header());
        if let Some(route_tag) = self.config.route_tag.as_deref() {
            builder = builder.header("SKRouteTag", route_tag);
        }
        builder
    }

    fn base_info(&self) -> serde_json::Value {
        serde_json::json!({
            "channel_version": env!("CARGO_PKG_VERSION"),
            "bot_agent": self.config.bot_agent,
        })
    }

    fn context_token(&self, user_id: &str) -> Option<String> {
        self.context_tokens
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(user_id)
            .cloned()
    }

    fn set_context_token(&self, user_id: &str, token: &str) {
        let snapshot = {
            let mut tokens = self
                .context_tokens
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            tokens.insert(user_id.to_string(), token.to_string());
            tokens.clone()
        };
        save_context_tokens(self.store.dir(), &snapshot);
    }

    fn spawn_session_watchers(self: &Arc<Self>) {
        self.reconcile_session_watchers();
    }

    fn reconcile_session_watchers(self: &Arc<Self>) {
        let subscribed: HashSet<String> = self
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
                let actor = DaemonState::channel_actor("qclaw", &uid);
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
                            if msg.source == "qclaw" {
                                continue;
                            }
                            channel.send_event_sequence(&uid, &[msg.event]).await;
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(n))) => {
                            tracing::warn!(
                                "[qclaw] Session broadcast lagged, skipped {n} messages"
                            );
                        }
                        Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => break,
                        Err(_) => {
                            let actor = DaemonState::channel_actor("qclaw", &uid);
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

/// Run the `QClaw` QR login flow and persist the resulting channel credentials.
///
/// # Errors
///
/// Returns an error when the QR API fails, the login expires, verification fails,
/// credentials are incomplete, or the credentials cannot be written to disk.
pub async fn login_with_qr<F, R>(
    home: &Path,
    options: &QclawLoginOptions,
    on_qr: F,
    mut read_verify_code: R,
) -> Result<QclawLoginCredentials, String>
where
    F: Fn(&str),
    R: FnMut(&str) -> Result<String, String>,
{
    let base_url = options
        .base_url
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(DEFAULT_BASE_URL)
        .trim()
        .to_string();
    let client = reqwest::Client::builder()
        .timeout(QR_POLL_TIMEOUT)
        .build()
        .map_err(|error| format!("failed to build HTTP client: {error}"))?;
    let qr = fetch_qr_code(&client, &base_url, options).await?;
    let qrcode = qr
        .qrcode
        .ok_or_else(|| "QClaw login did not return qrcode".to_string())?;
    let qrcode_url = qr
        .qrcode_img_content
        .ok_or_else(|| "QClaw login did not return qrcode URL".to_string())?;
    on_qr(&qrcode_url);

    let deadline = tokio::time::Instant::now() + QR_LOGIN_TIMEOUT;
    let mut current_base = base_url;
    let mut verify_code: Option<String> = None;
    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err("QClaw login timed out".into());
        }
        let status = poll_qr_status(
            &client,
            &current_base,
            &qrcode,
            verify_code.as_deref(),
            options,
        )
        .await?;
        match status.status.as_str() {
            "wait" => {}
            "scaned" => {
                verify_code = None;
                eprintln!("QClaw login scanned; waiting for confirmation...");
            }
            "need_verifycode" => {
                let code = read_verify_code("Enter the number shown in WeChat: ")?;
                verify_code = Some(code);
            }
            "verify_code_blocked" => {
                return Err("QClaw login verification code was rejected too many times".into());
            }
            "scaned_but_redirect" => {
                if let Some(host) = status
                    .redirect_host
                    .as_deref()
                    .filter(|host| !host.is_empty())
                {
                    current_base = format!("https://{host}");
                }
            }
            "expired" => return Err("QClaw login QR code expired; run login again".into()),
            "binded_redirect" => {
                if let Some(credentials) = load_login_credentials(home) {
                    return Ok(credentials);
                }
                return Err(
                    "This QClaw account is already bound, but no local credentials exist".into(),
                );
            }
            "confirmed" => {
                let credentials = QclawLoginCredentials {
                    token: status
                        .bot_token
                        .ok_or_else(|| "QClaw login confirmed without bot_token".to_string())?,
                    base_url: status.baseurl.unwrap_or(current_base),
                    account_id: status
                        .ilink_bot_id
                        .ok_or_else(|| "QClaw login confirmed without account id".to_string())?,
                    user_id: status.ilink_user_id,
                };
                save_login_credentials(home, &credentials, options)?;
                return Ok(credentials);
            }
            other => return Err(format!("unexpected QClaw login status: {other}")),
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

fn inbound_text_from_items(items: Option<&[QclawMessageItem]>) -> String {
    let Some(items) = items else {
        return String::new();
    };
    let mut parts = Vec::new();
    for item in items {
        match item.item_type.unwrap_or_default() {
            1 => {
                if let Some(text) = item
                    .text_item
                    .as_ref()
                    .and_then(|text_item| text_item.text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    parts.push(text.to_string());
                }
            }
            2 => parts.push("[image]".into()),
            3 => {
                if let Some(text) = item
                    .voice_item
                    .as_ref()
                    .and_then(|voice_item| voice_item.text.as_deref())
                    .map(str::trim)
                    .filter(|text| !text.is_empty())
                {
                    parts.push(text.to_string());
                } else {
                    parts.push("[voice]".into());
                }
            }
            4 => {
                let name = item
                    .file_item
                    .as_ref()
                    .and_then(|file_item| file_item.file_name.as_deref())
                    .map(str::trim)
                    .filter(|name| !name.is_empty())
                    .unwrap_or("file");
                parts.push(format!("[file: {name}]"));
            }
            5 => parts.push("[video]".into()),
            _ => {}
        }
    }
    parts.join("\n")
}

async fn fetch_qr_code(
    client: &reqwest::Client,
    base_url: &str,
    options: &QclawLoginOptions,
) -> Result<QrCodeResp, String> {
    let response = apply_public_headers(
        client
            .post(api_url(base_url, "ilink/bot/get_bot_qrcode"))
            .query(&[("bot_type", DEFAULT_BOT_TYPE)])
            .json(&serde_json::json!({"local_token_list": []})),
        options,
    )
    .send()
    .await
    .map_err(|error| format!("QClaw QR request failed: {error}"))?;
    decode_json_response(response, "get_bot_qrcode").await
}

async fn poll_qr_status(
    client: &reqwest::Client,
    base_url: &str,
    qrcode: &str,
    verify_code: Option<&str>,
    options: &QclawLoginOptions,
) -> Result<QrStatusResp, String> {
    let mut request = client
        .get(api_url(base_url, "ilink/bot/get_qrcode_status"))
        .query(&[("qrcode", qrcode)]);
    if let Some(code) = verify_code {
        request = request.query(&[("verify_code", code)]);
    }
    let response = apply_public_headers(request, options)
        .send()
        .await
        .map_err(|error| format!("QClaw QR status request failed: {error}"))?;
    decode_json_response(response, "get_qrcode_status").await
}

fn apply_public_headers(
    mut builder: reqwest::RequestBuilder,
    options: &QclawLoginOptions,
) -> reqwest::RequestBuilder {
    builder = builder
        .header("iLink-App-Id", ILINK_APP_ID)
        .header("iLink-App-ClientVersion", client_version_header());
    if let Some(route_tag) = options
        .route_tag
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        builder = builder.header("SKRouteTag", route_tag);
    }
    builder
}

fn save_login_credentials(
    home: &Path,
    credentials: &QclawLoginCredentials,
    options: &QclawLoginOptions,
) -> Result<(), String> {
    let files = cortex_kernel::ChannelFileSet::from_instance_home(home, "qclaw");
    std::fs::create_dir_all(&files.dir).map_err(|error| {
        format!(
            "failed to create QClaw channel dir {}: {error}",
            files.dir.display()
        )
    })?;
    let bot_agent = options
        .bot_agent
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map_or_else(|| default_bot_agent().to_string(), ToOwned::to_owned);
    let auth = serde_json::json!({
        "token": credentials.token,
        "base_url": credentials.base_url,
        "cdn_base_url": DEFAULT_CDN_BASE_URL,
        "account_id": credentials.account_id,
        "user_id": credentials.user_id,
        "route_tag": options.route_tag,
        "bot_agent": bot_agent,
        "max_retry": 10,
    });
    let json = serde_json::to_string_pretty(&auth)
        .map_err(|error| format!("failed to encode QClaw auth: {error}"))?;
    cortex_kernel::atomic_write_text(&files.auth, json)
        .map_err(|error| format!("failed to write {}: {error}", files.auth.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = std::fs::set_permissions(&files.auth, std::fs::Permissions::from_mode(0o600));
    }

    if !files.policy.exists() {
        let policy = serde_json::json!({
            "mode": "pairing",
            "whitelist": [],
            "blacklist": [],
            "pair_code_ttl_secs": 300,
            "max_pending": 10,
        });
        let json = serde_json::to_string_pretty(&policy)
            .map_err(|error| format!("failed to encode QClaw policy: {error}"))?;
        cortex_kernel::atomic_write_text(&files.policy, json)
            .map_err(|error| format!("failed to write {}: {error}", files.policy.display()))?;
    }
    Ok(())
}

fn load_login_credentials(home: &Path) -> Option<QclawLoginCredentials> {
    let auth = super::read_channel_auth(home, "qclaw")?;
    Some(QclawLoginCredentials {
        token: auth
            .get("token")
            .and_then(serde_json::Value::as_str)?
            .to_string(),
        base_url: auth
            .get("base_url")
            .and_then(serde_json::Value::as_str)
            .unwrap_or(DEFAULT_BASE_URL)
            .to_string(),
        account_id: auth
            .get("account_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        user_id: auth
            .get("user_id")
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned),
    })
}

async fn decode_json_response<T>(response: reqwest::Response, label: &str) -> Result<T, String>
where
    T: serde::de::DeserializeOwned,
{
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| format!("{label} response read failed: {error}"))?;
    if !status.is_success() {
        return Err(format!("{label} failed: {status} {body}"));
    }
    serde_json::from_str(&body)
        .map_err(|error| format!("{label} response decode failed: {error}; body={body}"))
}

fn api_url(base_url: &str, endpoint: &str) -> String {
    format!("{}/{}", base_url.trim_end_matches('/'), endpoint)
}

fn random_wechat_uin() -> String {
    let mut bytes = [0_u8; 4];
    OsRng.fill_bytes(&mut bytes);
    let value = u32::from_be_bytes(bytes).to_string();
    base64::engine::general_purpose::STANDARD.encode(value)
}

fn client_version_header() -> String {
    let mut parts = env!("CARGO_PKG_VERSION").split('.');
    let major = version_part(parts.next());
    let minor = version_part(parts.next());
    let patch = version_part(parts.next());
    ((major << 16) | (minor << 8) | patch).to_string()
}

fn version_part(raw: Option<&str>) -> u32 {
    raw.and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(0)
        .min(255)
}

fn generate_client_id() -> String {
    let counter = CLIENT_ID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_millis());
    format!("cortex-qclaw:{millis}:{counter}")
}

const fn duration_from_millis(value: u64) -> Option<Duration> {
    if value == 0 {
        None
    } else {
        Some(Duration::from_millis(value))
    }
}

const fn retry_delay(attempt: usize) -> Duration {
    match attempt {
        0 | 1 => Duration::from_secs(1),
        2 => Duration::from_secs(2),
        3 => Duration::from_secs(5),
        4 => Duration::from_secs(10),
        _ => Duration::from_secs(30),
    }
}

const fn default_bot_agent() -> &'static str {
    concat!("Cortex/", env!("CARGO_PKG_VERSION"))
}

fn sync_state_path(dir: &Path) -> std::path::PathBuf {
    dir.join("sync.json")
}

fn context_tokens_path(dir: &Path) -> std::path::PathBuf {
    dir.join("context_tokens.json")
}

fn load_sync_state(dir: &Path) -> SyncState {
    std::fs::read_to_string(sync_state_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or(SyncState {
            get_updates_buf: String::new(),
        })
}

fn save_sync_state(dir: &Path, state: &SyncState) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = cortex_kernel::atomic_write_text(&sync_state_path(dir), json);
    }
}

fn load_context_tokens(dir: &Path) -> HashMap<String, String> {
    std::fs::read_to_string(context_tokens_path(dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_context_tokens(dir: &Path, tokens: &HashMap<String, String>) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(json) = serde_json::to_string(tokens) {
        let _ = cortex_kernel::atomic_write_text(&context_tokens_path(dir), json);
    }
}
