use std::collections::HashMap;

use futures_util::StreamExt;
use reqwest::Client;

use super::client::LlmClient;
use super::types::{AuthType, LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};

pub struct OpenAIClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub auth_type: AuthType,
    http: Client,
}

impl OpenAIClient {
    #[must_use]
    pub fn new(base_url: &str, api_key: &str, model: &str, auth_type: AuthType) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            auth_type,
            http: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for OpenAIClient {
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let url = if self.base_url.ends_with("/v1")
            || self.base_url.ends_with("/v4")
            || self.base_url.ends_with("/v1/")
            || self.base_url.ends_with("/v4/")
        {
            let base = self.base_url.trim_end_matches('/');
            format!("{base}/chat/completions")
        } else {
            format!("{}/v1/chat/completions", self.base_url)
        };
        let streaming = request.on_text.is_some();

        let messages = build_messages(&request);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": request.max_tokens,
            "messages": messages,
        });

        if let Some(tools) = request.tools {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }
        if streaming {
            body["stream"] = serde_json::Value::Bool(true);
            body["stream_options"] = serde_json::json!({"include_usage": true});
        }

        let mut req = self.http.post(&url).json(&body);
        if !self.api_key.is_empty() {
            req = match self.auth_type {
                AuthType::XApiKey => req.header("x-api-key", &self.api_key),
                _ => req.bearer_auth(&self.api_key),
            };
        }

        let resp = req
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

fn build_messages(request: &LlmRequest<'_>) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();

    if let Some(system) = request.system {
        messages.push(serde_json::json!({"role": "system", "content": system}));
    }

    for msg in request.messages {
        let role = match msg.role {
            cortex_types::Role::User => "user",
            cortex_types::Role::Assistant => "assistant",
        };

        if msg.has_tool_blocks() {
            // Split into assistant tool_calls + tool results
            let mut tool_calls_json = Vec::new();
            let mut text_parts = Vec::new();

            for block in &msg.content {
                match block {
                    cortex_types::ContentBlock::Text { text } => {
                        text_parts.push(text.clone());
                    }
                    cortex_types::ContentBlock::ToolUse { id, name, input } => {
                        tool_calls_json.push(serde_json::json!({
                            "id": id,
                            "type": "function",
                            "function": {"name": name, "arguments": input.to_string()}
                        }));
                    }
                    cortex_types::ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        ..
                    } => {
                        messages.push(serde_json::json!({
                            "role": "tool",
                            "tool_call_id": tool_use_id,
                            "content": content
                        }));
                    }
                    cortex_types::ContentBlock::Image { .. } => {}
                }
            }

            if !tool_calls_json.is_empty() {
                let mut assistant_msg = serde_json::json!({
                    "role": "assistant",
                    "tool_calls": tool_calls_json
                });
                if !text_parts.is_empty() {
                    assistant_msg["content"] = serde_json::Value::String(text_parts.join(""));
                }
                messages.push(assistant_msg);
            }
        } else if msg.has_images() {
            let mut content_blocks = Vec::new();
            for block in &msg.content {
                match block {
                    cortex_types::ContentBlock::Text { text } => {
                        content_blocks.push(serde_json::json!({"type": "text", "text": text}));
                    }
                    cortex_types::ContentBlock::Image { media_type, data } => {
                        content_blocks.push(serde_json::json!({
                            "type": "image_url",
                            "image_url": {"url": format!("data:{media_type};base64,{data}")}
                        }));
                    }
                    _ => {}
                }
            }
            messages.push(serde_json::json!({"role": role, "content": content_blocks}));
        } else {
            messages.push(serde_json::json!({"role": role, "content": msg.text_content()}));
        }
    }

    messages
}

struct StreamState {
    full_text: String,
    model: String,
    tool_acc: HashMap<usize, (String, String, String)>,
    usage: Usage,
}

impl StreamState {
    fn process_sse(
        &mut self,
        json: &serde_json::Value,
        on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) {
        if let Some(model) = json.get("model").and_then(serde_json::Value::as_str) {
            self.model = model.to_string();
        }

        if let Some(choices) = json.get("choices").and_then(serde_json::Value::as_array)
            && let Some(delta) = choices.first().and_then(|c| c.get("delta"))
        {
            if let Some(text) = delta.get("content").and_then(serde_json::Value::as_str) {
                self.full_text.push_str(text);
                if let Some(cb) = on_text {
                    cb(text);
                }
            }
            if let Some(tcs) = delta
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
            {
                for tc in tcs {
                    let idx = tc
                        .get("index")
                        .and_then(serde_json::Value::as_u64)
                        .map_or(0, |v| usize::try_from(v).unwrap_or(0));
                    let entry = self
                        .tool_acc
                        .entry(idx)
                        .or_insert_with(|| (String::new(), String::new(), String::new()));
                    if let Some(id) = tc.get("id").and_then(serde_json::Value::as_str) {
                        entry.0 = id.to_string();
                    }
                    if let Some(func) = tc.get("function") {
                        if let Some(name) = func.get("name").and_then(serde_json::Value::as_str) {
                            entry.1 = name.to_string();
                        }
                        if let Some(args) =
                            func.get("arguments").and_then(serde_json::Value::as_str)
                        {
                            entry.2.push_str(args);
                        }
                    }
                }
            }
        }

        if let Some(u) = json.get("usage") {
            self.usage.input_tokens = u
                .get("prompt_tokens")
                .and_then(serde_json::Value::as_u64)
                .map_or(self.usage.input_tokens, |v| usize::try_from(v).unwrap_or(0));
            self.usage.output_tokens = u
                .get("completion_tokens")
                .and_then(serde_json::Value::as_u64)
                .map_or(self.usage.output_tokens, |v| {
                    usize::try_from(v).unwrap_or(0)
                });
        }
    }

    fn into_response(mut self) -> LlmResponse {
        let mut tool_calls: Vec<(usize, LlmToolCall)> = self
            .tool_acc
            .drain()
            .map(|(idx, (id, name, args_json))| {
                let input = serde_json::from_str(&args_json)
                    .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
                (idx, LlmToolCall { id, name, input })
            })
            .collect();
        tool_calls.sort_by_key(|(idx, _)| *idx);

        LlmResponse {
            text: if self.full_text.is_empty() {
                None
            } else {
                Some(self.full_text)
            },
            tool_calls: tool_calls.into_iter().map(|(_, tc)| tc).collect(),
            usage: self.usage,
            model: self.model,
        }
    }
}

async fn parse_stream(
    resp: reqwest::Response,
    on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Result<LlmResponse, LlmError> {
    let mut state = StreamState {
        full_text: String::new(),
        model: String::new(),
        tool_acc: HashMap::new(),
        usage: Usage::default(),
    };
    let mut stream = resp.bytes_stream();
    let mut buffer = String::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| LlmError::RequestFailed(e.to_string()))?;
        buffer.push_str(&String::from_utf8_lossy(&bytes));

        while let Some(pos) = buffer.find('\n') {
            let line = buffer[..pos].to_string();
            buffer = buffer[pos + 1..].to_string();

            let line = line.trim();
            if line == "data: [DONE]" {
                break;
            }
            if let Some(data) = line.strip_prefix("data: ")
                && let Ok(json) = serde_json::from_str::<serde_json::Value>(data)
            {
                // Check for error in stream
                if let Some(err) = json.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("unknown stream error");
                    return Err(LlmError::RequestFailed(format!("Stream error: {msg}")));
                }
                state.process_sse(&json, on_text);
            }
        }
    }

    Ok(state.into_response())
}

fn parse_response(json: &serde_json::Value) -> LlmResponse {
    let model = json
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut text = None;
    let mut tool_calls = Vec::new();

    if let Some(choices) = json.get("choices").and_then(serde_json::Value::as_array)
        && let Some(message) = choices.first().and_then(|c| c.get("message"))
    {
        text = message
            .get("content")
            .and_then(serde_json::Value::as_str)
            .map(String::from);

        if let Some(tcs) = message
            .get("tool_calls")
            .and_then(serde_json::Value::as_array)
        {
            for tc in tcs {
                let id = tc
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(func) = tc.get("function") {
                    let name = func
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = func
                        .get("arguments")
                        .and_then(serde_json::Value::as_str)
                        .and_then(|s| serde_json::from_str(s).ok())
                        .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
                    tool_calls.push(LlmToolCall { id, name, input });
                }
            }
        }
    }

    let usage = json.get("usage").map_or_else(Usage::default, |u| Usage {
        input_tokens: u
            .get("prompt_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |v| usize::try_from(v).unwrap_or(0)),
        output_tokens: u
            .get("completion_tokens")
            .and_then(serde_json::Value::as_u64)
            .map_or(0, |v| usize::try_from(v).unwrap_or(0)),
    });

    LlmResponse {
        text,
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
            "model": "gpt-5.4",
            "choices": [{"message": {"content": "Hello!"}}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });
        let resp = parse_response(&json);
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert_eq!(resp.usage.input_tokens, 10);
    }
}
