use futures_util::StreamExt;
use reqwest::Client;

use super::client::LlmClient;
use super::types::{LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};

pub struct AnthropicClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    http: Client,
}

impl AnthropicClient {
    #[must_use]
    pub fn new(base_url: &str, api_key: &str, model: &str) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            http: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let streaming = request.on_text.is_some();

        let mut messages = Vec::new();
        for msg in request.messages {
            let role = match msg.role {
                cortex_types::Role::User => "user",
                cortex_types::Role::Assistant => "assistant",
            };
            if msg.has_tool_blocks() || msg.has_images() {
                let blocks: Vec<serde_json::Value> = msg
                    .content
                    .iter()
                    .map(|b| match b {
                        cortex_types::ContentBlock::Text { text } => {
                            serde_json::json!({"type": "text", "text": text})
                        }
                        cortex_types::ContentBlock::ToolUse { id, name, input } => {
                            serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
                        }
                        cortex_types::ContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error})
                        }
                        cortex_types::ContentBlock::Image { media_type, data } => {
                            serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}})
                        }
                    })
                    .collect();
                messages.push(serde_json::json!({"role": role, "content": blocks}));
            } else {
                messages.push(serde_json::json!({"role": role, "content": msg.text_content()}));
            }
        }

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
        });

        if let Some(system) = request.system {
            body["system"] = serde_json::Value::String(system.to_string());
        }
        if let Some(tools) = request.tools {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }
        if streaming {
            body["stream"] = serde_json::Value::Bool(true);
        }

        let resp = self
            .http
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("anthropic-beta", "advanced-tool-use-2025-11-20")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        let status = resp.status();
        if !status.is_success() {
            let body = resp
                .text()
                .await
                .unwrap_or_else(|_| String::from("<no body>"));
            return Err(LlmError::RequestFailed(format!("HTTP {status}: {body}")));
        }

        if streaming {
            parse_stream(resp, request.on_text).await
        } else {
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;
            Ok(parse_response(&json))
        }
    }
}

struct StreamAccumulator {
    full_text: String,
    tool_calls: Vec<LlmToolCall>,
    usage: Usage,
    model: String,
    current_tool_id: String,
    current_tool_name: String,
    current_tool_json: String,
}

impl StreamAccumulator {
    fn new() -> Self {
        Self {
            full_text: String::new(),
            tool_calls: Vec::new(),
            usage: Usage::default(),
            model: String::new(),
            current_tool_id: String::new(),
            current_tool_name: String::new(),
            current_tool_json: String::new(),
        }
    }

    fn process_event(
        &mut self,
        json: &serde_json::Value,
        on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) {
        let event_type = json
            .get("type")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        match event_type {
            "message_start" => {
                if let Some(msg) = json.get("message") {
                    self.model = msg
                        .get("model")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if let Some(u) = msg.get("usage") {
                        self.usage.input_tokens = u
                            .get("input_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .map_or(0, |v| usize::try_from(v).unwrap_or(0));
                    }
                }
            }
            "content_block_start" => {
                if let Some(cb) = json.get("content_block")
                    && cb.get("type").and_then(serde_json::Value::as_str) == Some("tool_use")
                {
                    self.current_tool_id = cb
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    self.current_tool_name = cb
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    self.current_tool_json.clear();
                }
            }
            "content_block_delta" => {
                if let Some(delta) = json.get("delta") {
                    let delta_type = delta
                        .get("type")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("");
                    if delta_type == "text_delta" {
                        if let Some(text) = delta.get("text").and_then(serde_json::Value::as_str) {
                            self.full_text.push_str(text);
                            if let Some(cb) = on_text {
                                cb(text);
                            }
                        }
                    } else if delta_type == "input_json_delta"
                        && let Some(json_str) = delta
                            .get("partial_json")
                            .and_then(serde_json::Value::as_str)
                    {
                        self.current_tool_json.push_str(json_str);
                    }
                }
            }
            "content_block_stop" if !self.current_tool_name.is_empty() => {
                let input = serde_json::from_str(&self.current_tool_json)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                self.tool_calls.push(LlmToolCall {
                    id: std::mem::take(&mut self.current_tool_id),
                    name: std::mem::take(&mut self.current_tool_name),
                    input,
                });
                self.current_tool_json.clear();
            }
            "message_delta" => {
                if let Some(u) = json.get("usage") {
                    // Some providers (e.g. ZAI) send input_tokens in
                    // message_delta instead of message_start.
                    if self.usage.input_tokens == 0 {
                        self.usage.input_tokens = u
                            .get("input_tokens")
                            .and_then(serde_json::Value::as_u64)
                            .map_or(0, |v| usize::try_from(v).unwrap_or(0));
                    }
                    self.usage.output_tokens = u
                        .get("output_tokens")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(0, |v| usize::try_from(v).unwrap_or(0));
                }
            }
            _ => {}
        }
    }

    fn into_response(self) -> LlmResponse {
        LlmResponse {
            text: if self.full_text.is_empty() {
                None
            } else {
                Some(self.full_text)
            },
            tool_calls: self.tool_calls,
            usage: self.usage,
            model: self.model,
        }
    }
}

async fn parse_stream(
    resp: reqwest::Response,
    on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Result<LlmResponse, LlmError> {
    let mut acc = StreamAccumulator::new();
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| LlmError::RequestFailed(e.to_string()))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();

            let line = line.trim();
            if let Some(data) = line.strip_prefix("data: ")
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
            {
                // Check for SSE error events (provider returns error in stream)
                if let Some(err) = json.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown stream error");
                    return Err(LlmError::RequestFailed(format!("Stream error: {msg}")));
                }
                acc.process_event(&json, on_text);
            }
        }
    }

    Ok(acc.into_response())
}

fn parse_response(json: &serde_json::Value) -> LlmResponse {
    let model = json
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut text = String::new();
    let mut tool_calls = Vec::new();

    if let Some(content) = json.get("content").and_then(serde_json::Value::as_array) {
        for block in content {
            let block_type = block
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            match block_type {
                "text" => {
                    if let Some(t) = block.get("text").and_then(serde_json::Value::as_str) {
                        text.push_str(t);
                    }
                }
                // server_tool_use: provider-side tool (web search, etc.)
                // Treat as text — append the result content to the response
                "server_tool_use" => {
                    if let Some(query) = block
                        .get("input")
                        .and_then(|i| i.get("query"))
                        .and_then(serde_json::Value::as_str)
                    {
                        use std::fmt::Write;
                        let _ = writeln!(text, "[Searching: {query}]");
                    }
                }
                "web_search_tool_result" => {
                    if let Some(content) =
                        block.get("content").and_then(serde_json::Value::as_array)
                    {
                        for item in content {
                            if let Some(t) = item.get("text").and_then(serde_json::Value::as_str) {
                                text.push_str(t);
                                text.push('\n');
                            }
                        }
                    }
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = block
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
                    tool_calls.push(LlmToolCall { id, name, input });
                }
                _ => {}
            }
        }
    }

    let usage = json.get("usage").map_or_else(Usage::default, |u| Usage {
        input_tokens: u
            .get("input_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |v| usize::try_from(v).unwrap_or(0)),
        output_tokens: u
            .get("output_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |v| usize::try_from(v).unwrap_or(0)),
    });

    LlmResponse {
        text: if text.is_empty() { None } else { Some(text) },
        tool_calls,
        usage,
        model,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text() {
        let json = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "content": [{"type": "text", "text": "Hello!"}],
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp = parse_response(&json);
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.model, "claude-sonnet-4-20250514");
    }

    #[test]
    fn parse_tool_call() {
        let json = serde_json::json!({
            "model": "claude-sonnet-4-20250514",
            "content": [{
                "type": "tool_use",
                "id": "t1",
                "name": "read",
                "input": {"path": "/tmp"}
            }],
            "usage": {"input_tokens": 20, "output_tokens": 15}
        });
        let resp = parse_response(&json);
        assert!(resp.text.is_none());
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "read");
    }
}
