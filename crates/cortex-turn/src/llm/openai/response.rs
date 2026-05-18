use std::collections::HashMap;

use futures_util::StreamExt;

use super::super::types::{LlmError, LlmResponse, LlmToolCall, Usage};

fn openai_reasoning_text(value: &serde_json::Value) -> Option<&str> {
    value
        .get("reasoning")
        .or_else(|| value.get("reasoning_content"))
        .and_then(serde_json::Value::as_str)
}

struct StreamState {
    full_text: String,
    model: String,
    tool_acc: HashMap<usize, (String, String, String)>,
    usage: Usage,
    thinking: bool,
    reasoning_started: bool,
    reasoning_closed: bool,
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
            if let Some(reasoning) = openai_reasoning_text(delta) {
                if self.thinking {
                    self.append_reasoning(reasoning, on_text);
                } else {
                    self.append_visible_text(reasoning, on_text);
                }
            }
            if let Some(text) = delta.get("content").and_then(serde_json::Value::as_str) {
                if self.thinking {
                    self.close_reasoning(on_text);
                }
                self.append_visible_text(text, on_text);
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
            apply_openai_usage(&mut self.usage, u);
        }
    }

    fn append_visible_text(&mut self, text: &str, on_text: Option<&(dyn Fn(&str) + Send + Sync)>) {
        self.full_text.push_str(text);
        if let Some(cb) = on_text {
            cb(text);
        }
    }

    fn append_reasoning(
        &mut self,
        reasoning: &str,
        on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    ) {
        if reasoning.is_empty() {
            return;
        }
        if !self.reasoning_started {
            self.reasoning_started = true;
            self.full_text.push_str("<think>");
            if let Some(cb) = on_text {
                cb("<think>");
            }
        }
        self.full_text.push_str(reasoning);
        if let Some(cb) = on_text {
            cb(reasoning);
        }
    }

    fn close_reasoning(&mut self, on_text: Option<&(dyn Fn(&str) + Send + Sync)>) {
        if self.reasoning_started && !self.reasoning_closed {
            self.reasoning_closed = true;
            self.full_text.push_str("</think>\n");
            if let Some(cb) = on_text {
                cb("</think>\n");
            }
        }
    }

    fn into_response(mut self) -> LlmResponse {
        self.close_reasoning(None);
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

pub(super) async fn parse_stream(
    resp: reqwest::Response,
    on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
    thinking: bool,
) -> Result<LlmResponse, LlmError> {
    let mut state = StreamState {
        full_text: String::new(),
        model: String::new(),
        tool_acc: HashMap::new(),
        usage: Usage::default(),
        thinking,
        reasoning_started: false,
        reasoning_closed: false,
    };
    let mut stream = resp.bytes_stream();
    let mut buffer: Vec<u8> = Vec::new();

    while let Some(chunk) = stream.next().await {
        let bytes = chunk.map_err(|e| LlmError::RequestFailed(e.to_string()))?;
        buffer.extend_from_slice(&bytes);

        while let Some(pos) = buffer.iter().position(|&byte| byte == b'\n') {
            let line_bytes: Vec<u8> = buffer.drain(..=pos).collect();
            let line = std::str::from_utf8(&line_bytes[..line_bytes.len().saturating_sub(1)])
                .map_err(|e| LlmError::RequestFailed(format!("invalid UTF-8 in stream: {e}")))?;

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

pub(super) fn parse_response(json: &serde_json::Value, thinking: bool) -> LlmResponse {
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
        let content = message
            .get("content")
            .and_then(serde_json::Value::as_str)
            .map(String::from);
        text = merge_reasoning_and_content(openai_reasoning_text(message), content, thinking);

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

    let usage = json
        .get("usage")
        .map_or_else(Usage::default, parse_openai_usage);

    LlmResponse {
        text,
        tool_calls,
        usage,
        model,
    }
}

fn merge_reasoning_and_content(
    reasoning: Option<&str>,
    content: Option<String>,
    thinking: bool,
) -> Option<String> {
    let Some(reasoning) = reasoning.filter(|value| !value.is_empty()) else {
        return content;
    };
    if !thinking {
        return content
            .filter(|value| !value.is_empty())
            .or_else(|| Some(reasoning.to_string()));
    }
    let mut text = format!("<think>{reasoning}</think>");
    if let Some(content) = content.filter(|value| !value.is_empty()) {
        text.push('\n');
        text.push_str(&content);
    }
    Some(text)
}

fn parse_openai_usage(value: &serde_json::Value) -> Usage {
    let mut usage = Usage::default();
    apply_openai_usage(&mut usage, value);
    usage
}

fn apply_openai_usage(usage: &mut Usage, value: &serde_json::Value) {
    if let Some(tokens) = token_count(value.get("prompt_tokens")) {
        apply_usage_counter(&mut usage.input_tokens, tokens);
    }
    if let Some(tokens) = token_count(value.get("completion_tokens")) {
        apply_usage_counter(&mut usage.output_tokens, tokens);
    }
    if let Some(tokens) = openai_cache_read_tokens(value) {
        apply_usage_counter(&mut usage.cache_read_input_tokens, tokens);
    }
    if let Some(tokens) = openai_cache_creation_tokens(value) {
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

fn nested_token_count(
    value: &serde_json::Value,
    object_key: &str,
    token_key: &str,
) -> Option<usize> {
    token_count(
        value
            .get(object_key)
            .and_then(|nested| nested.get(token_key)),
    )
}

fn openai_cache_read_tokens(value: &serde_json::Value) -> Option<usize> {
    [
        nested_token_count(value, "prompt_tokens_details", "cached_tokens"),
        nested_token_count(value, "input_tokens_details", "cached_tokens"),
        token_count(value.get("cached_tokens")),
    ]
    .into_iter()
    .flatten()
    .max()
}

fn openai_cache_creation_tokens(value: &serde_json::Value) -> Option<usize> {
    [
        nested_token_count(
            value,
            "prompt_tokens_details",
            "cache_creation_input_tokens",
        ),
        nested_token_count(value, "input_tokens_details", "cache_creation_input_tokens"),
        token_count(value.get("cache_creation_input_tokens")),
    ]
    .into_iter()
    .flatten()
    .max()
}
