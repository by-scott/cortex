use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use tokio_tungstenite::tungstenite::Message;

use super::{AccessToken, QqChannel};

const QQ_TOKEN_URL: &str = "https://bots.qq.com/app/getAppAccessToken";
const QQ_API_BASE: &str = "https://api.sgroup.qq.com";
const QQ_SANDBOX_API_BASE: &str = "https://sandbox.api.sgroup.qq.com";
const TOKEN_REFRESH_MARGIN: Duration = Duration::from_mins(5);

const INTENT_GROUP_AND_C2C: u32 = 1 << 25;
const INTENT_INTERACTION: u32 = 1 << 26;
static QQ_RUSTLS_INIT: Once = Once::new();

impl QqChannel {
    pub(super) const fn api_base(&self) -> &'static str {
        if self.sandbox {
            QQ_SANDBOX_API_BASE
        } else {
            QQ_API_BASE
        }
    }

    pub(super) async fn ensure_access_token(&self) -> Result<String, String> {
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
}

fn install_rustls_provider() {
    QQ_RUSTLS_INIT.call_once(|| {
        let _ = rustls::crypto::ring::default_provider().install_default();
    });
}
