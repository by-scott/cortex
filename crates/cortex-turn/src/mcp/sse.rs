use async_trait::async_trait;
use cortex_types::mcp::{McpNotification, McpRequest, McpResponse};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

use super::{McpTransport, McpTransportError};

pub struct SseTransport {
    url: String,
    client: Client,
    headers: HashMap<String, String>,
    next_id: AtomicU64,
}

impl SseTransport {
    #[must_use]
    pub fn new(url: &str, headers: &HashMap<String, String>) -> Self {
        Self {
            url: url.to_string(),
            client: Client::new(),
            headers: headers.clone(),
            next_id: AtomicU64::new(1),
        }
    }

    fn build_request(&self, body: Vec<u8>) -> reqwest::RequestBuilder {
        let mut req = self
            .client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream");

        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        req.body(body)
    }

    async fn send_and_parse(&self, body: Vec<u8>) -> Result<McpResponse, McpTransportError> {
        let resp = self
            .build_request(body)
            .send()
            .await
            .map_err(|e| McpTransportError::Io(format!("HTTP request failed: {e}")))?;

        let content_type = resp
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let text = resp
            .text()
            .await
            .map_err(|e| McpTransportError::Io(format!("read body failed: {e}")))?;

        if content_type.contains("text/event-stream") {
            parse_sse_response(&text)
        } else {
            serde_json::from_str(&text)
                .map_err(|e| McpTransportError::Protocol(format!("invalid JSON response: {e}")))
        }
    }
}

fn parse_sse_response(text: &str) -> Result<McpResponse, McpTransportError> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(data) = line.strip_prefix("data:") {
            let data = data.trim();
            if data.is_empty() {
                continue;
            }
            if let Ok(resp) = serde_json::from_str::<McpResponse>(data) {
                return Ok(resp);
            }
        }
    }
    Err(McpTransportError::Protocol(
        "no valid JSON-RPC message found in SSE stream".into(),
    ))
}

#[async_trait]
impl McpTransport for SseTransport {
    async fn send_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<McpResponse, McpTransportError> {
        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        let request = McpRequest::new(id, method, params);
        let body = serde_json::to_vec(&request)
            .map_err(|e| McpTransportError::Protocol(format!("serialize failed: {e}")))?;
        self.send_and_parse(body).await
    }

    async fn send_notification(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> Result<(), McpTransportError> {
        let notif = McpNotification::new(method, params);
        let body = serde_json::to_vec(&notif)
            .map_err(|e| McpTransportError::Protocol(format!("serialize failed: {e}")))?;
        let _ = self
            .build_request(body)
            .send()
            .await
            .map_err(|e| McpTransportError::Io(format!("HTTP request failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_extracts_json_rpc() {
        let sse_text =
            "event: message\ndata: {\"jsonrpc\":\"2.0\",\"result\":{\"tools\":[]},\"id\":1}\n\n";
        let resp = parse_sse_response(sse_text).unwrap();
        assert_eq!(resp.id, Some(1));
        assert!(resp.result.is_some());
    }

    #[test]
    fn parse_sse_skips_empty_data() {
        let sse_text = "data: \ndata: {\"jsonrpc\":\"2.0\",\"result\":{},\"id\":2}\n\n";
        let resp = parse_sse_response(sse_text).unwrap();
        assert_eq!(resp.id, Some(2));
    }

    #[test]
    fn parse_sse_no_valid_message() {
        let sse_text = "event: ping\ndata: \n\n";
        assert!(parse_sse_response(sse_text).is_err());
    }
}
