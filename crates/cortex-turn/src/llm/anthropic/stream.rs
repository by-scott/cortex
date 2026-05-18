use futures_util::StreamExt;

use super::super::types::{LlmResponse, LlmToolCall, Usage};
use super::redact_image_blocks;

pub(super) fn ensure_non_empty_response(
    response: LlmResponse,
) -> Result<LlmResponse, StreamFailure> {
    let has_text = response
        .text
        .as_ref()
        .is_some_and(|text| !text.trim().is_empty());
    if has_text || !response.tool_calls.is_empty() {
        Ok(response)
    } else {
        Err(StreamFailure {
            error: "empty Anthropic response".into(),
            emitted_text: false,
        })
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
    current_tool_json_from_start: bool,
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
            current_tool_json_from_start: false,
        }
    }

    fn push_text(&mut self, text: &str, on_text: Option<&(dyn Fn(&str) + Send + Sync)>) {
        if text.is_empty() {
            return;
        }
        self.full_text.push_str(text);
        if let Some(cb) = on_text {
            cb(text);
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
            "message_start" => self.process_message_start(json),
            "content_block_start" => self.process_content_block_start(json, on_text),
            "content_block_delta" => self.process_content_block_delta(json, on_text),
            "content_block_stop" if !self.current_tool_name.is_empty() => {
                self.finish_tool_block();
            }
            "message_delta" => self.process_message_delta(json),
            _ => {}
        }
    }

    fn process_message_start(&mut self, json: &serde_json::Value) {
        let Some(msg) = json.get("message") else {
            return;
        };
        self.model = msg
            .get("model")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        if let Some(u) = msg.get("usage") {
            apply_anthropic_usage(&mut self.usage, u);
        }
    }

    fn process_content_block_start(
        &mut self,
        json: &serde_json::Value,
        on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) {
        let Some(cb) = json.get("content_block") else {
            return;
        };
        match cb.get("type").and_then(serde_json::Value::as_str) {
            Some("text") => {
                if let Some(text) = cb.get("text").and_then(serde_json::Value::as_str) {
                    self.push_text(text, on_text);
                }
            }
            Some("tool_use") => self.start_tool_block(cb),
            _ => {}
        }
    }

    fn start_tool_block(&mut self, content_block: &serde_json::Value) {
        self.current_tool_id = content_block
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        self.current_tool_name = content_block
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string();
        self.current_tool_json.clear();
        self.current_tool_json_from_start = false;
        if let Some(input) = content_block.get("input")
            && !input.is_null()
            && input.as_object().is_none_or(|obj| !obj.is_empty())
        {
            self.current_tool_json = input.to_string();
            self.current_tool_json_from_start = true;
        }
    }

    fn process_content_block_delta(
        &mut self,
        json: &serde_json::Value,
        on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) {
        let Some(delta) = json.get("delta") else {
            return;
        };
        match delta.get("type").and_then(serde_json::Value::as_str) {
            Some("text_delta") => {
                if let Some(text) = delta.get("text").and_then(serde_json::Value::as_str) {
                    self.push_text(text, on_text);
                }
            }
            Some("input_json_delta") => {
                if let Some(json_str) = delta
                    .get("partial_json")
                    .and_then(serde_json::Value::as_str)
                {
                    self.push_tool_json_delta(json_str);
                }
            }
            _ => {}
        }
    }

    fn push_tool_json_delta(&mut self, json_str: &str) {
        if self.current_tool_json_from_start {
            self.current_tool_json.clear();
            self.current_tool_json_from_start = false;
        }
        self.current_tool_json.push_str(json_str);
    }

    fn finish_tool_block(&mut self) {
        let input = serde_json::from_str(&self.current_tool_json)
            .unwrap_or_else(|_| serde_json::Value::Object(serde_json::Map::new()));
        self.tool_calls.push(LlmToolCall {
            id: std::mem::take(&mut self.current_tool_id),
            name: std::mem::take(&mut self.current_tool_name),
            input,
        });
        self.current_tool_json.clear();
        self.current_tool_json_from_start = false;
    }

    fn process_message_delta(&mut self, json: &serde_json::Value) {
        let Some(u) = json.get("usage") else {
            return;
        };
        apply_anthropic_usage(&mut self.usage, u);
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

pub(super) struct StreamFailure {
    pub(super) error: String,
    pub(super) emitted_text: bool,
}

pub(super) async fn parse_stream(
    resp: reqwest::Response,
    on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    request_snapshot: Option<serde_json::Value>,
) -> Result<LlmResponse, StreamFailure> {
    let mut acc = StreamAccumulator::new();
    let mut stream = resp.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();
    let mut emitted_text = false;

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| StreamFailure {
            error: e.to_string(),
            emitted_text,
        })?;
        buffer.extend_from_slice(&bytes);

        while let Some(pos) = buffer.iter().position(|&byte| byte == b'\n') {
            let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
            let line = std::str::from_utf8(&line_bytes[..line_bytes.len().saturating_sub(1)])
                .map_err(|e| StreamFailure {
                    error: format!("invalid UTF-8 in stream: {e}"),
                    emitted_text,
                })?;

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
                    if let Some(mut snapshot) = request_snapshot.clone() {
                        redact_image_blocks(&mut snapshot);
                        tracing::error!(
                            request = %snapshot,
                            error = %msg,
                            "Anthropic image stream request failed"
                        );
                    }
                    return Err(StreamFailure {
                        error: format!("Stream error: {msg}"),
                        emitted_text,
                    });
                }
                acc.process_event(&json, on_text);
                emitted_text = emitted_text || !acc.full_text.is_empty();
            }
        }
    }

    ensure_non_empty_response(acc.into_response()).map_err(|mut failure| {
        failure.error = "empty Anthropic stream response".into();
        failure.emitted_text = emitted_text;
        failure
    })
}

pub(super) fn parse_response(json: &serde_json::Value) -> LlmResponse {
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
                // Treat as text: append the result content to the response.
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

    let usage = json
        .get("usage")
        .map_or_else(Usage::default, parse_anthropic_usage);

    LlmResponse {
        text: if text.is_empty() { None } else { Some(text) },
        tool_calls,
        usage,
        model,
    }
}

fn parse_anthropic_usage(value: &serde_json::Value) -> Usage {
    let mut usage = Usage::default();
    apply_anthropic_usage(&mut usage, value);
    usage
}

fn apply_anthropic_usage(usage: &mut Usage, value: &serde_json::Value) {
    if let Some(tokens) = token_count(value.get("input_tokens")) {
        apply_usage_counter(&mut usage.input_tokens, tokens);
    }
    if let Some(tokens) = token_count(value.get("output_tokens")) {
        apply_usage_counter(&mut usage.output_tokens, tokens);
    }
    if let Some(tokens) = token_count(value.get("cache_read_input_tokens")) {
        apply_usage_counter(&mut usage.cache_read_input_tokens, tokens);
    }
    if let Some(tokens) = token_count(value.get("cache_creation_input_tokens")) {
        apply_usage_counter(&mut usage.cache_creation_input_tokens, tokens);
    }
}

const fn apply_usage_counter(target: &mut usize, tokens: usize) {
    if tokens > 0 || *target == 0 {
        *target = tokens;
    }
}

fn token_count(value: Option<&serde_json::Value>) -> Option<usize> {
    value
        .and_then(serde_json::Value::as_u64)
        .and_then(|tokens| usize::try_from(tokens).ok())
}
