use futures_util::StreamExt;
use reqwest::Client;

use super::client::LlmClient;
use super::types::{LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};
use super::{max_tokens_for_api, normalize_messages_for_api};

pub struct OllamaClient {
    pub base_url: String,
    pub model: String,
    pub vision_max_output_tokens: usize,
    http: Client,
}

impl OllamaClient {
    #[must_use]
    pub fn new(base_url: &str, model: &str, vision_max_output_tokens: usize) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            vision_max_output_tokens,
            http: Client::new(),
        }
    }
}

#[async_trait::async_trait]
impl LlmClient for OllamaClient {
    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/api/chat", self.base_url);
        let streaming = request.on_text.is_some();
        let normalized_messages = normalize_messages_for_api(request.messages);
        let max_tokens = max_tokens_for_api(
            request.max_tokens,
            &normalized_messages,
            self.vision_max_output_tokens,
        );
        let normalized_request = LlmRequest {
            system: request.system,
            messages: &normalized_messages,
            tools: request.tools,
            max_tokens,
            transient_retries: request.transient_retries,
            on_text: request.on_text,
        };

        let mut messages = Vec::new();
        if let Some(system) = normalized_request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }

        for msg in normalized_request.messages {
            let role = match msg.role {
                cortex_types::Role::User => "user",
                cortex_types::Role::Assistant => "assistant",
            };

            if msg.has_tool_blocks() {
                // Ollama uses OpenAI-like tool format
                let mut tool_calls_json = Vec::new();
                for block in &msg.content {
                    match block {
                        cortex_types::ContentBlock::ToolUse { name, input, .. } => {
                            tool_calls_json.push(serde_json::json!({
                                "function": {"name": name, "arguments": input}
                            }));
                        }
                        cortex_types::ContentBlock::ToolResult { content, .. } => {
                            messages.push(serde_json::json!({
                                "role": "tool",
                                "content": content
                            }));
                        }
                        _ => {}
                    }
                }
                if !tool_calls_json.is_empty() {
                    messages.push(serde_json::json!({
                        "role": "assistant",
                        "tool_calls": tool_calls_json
                    }));
                }
            } else {
                let mut msg_json = serde_json::json!({"role": role, "content": msg.text_content()});
                // Ollama uses "images" array for multimodal
                if msg.has_images() {
                    let images: Vec<&str> = msg
                        .content
                        .iter()
                        .filter_map(|b| match b {
                            cortex_types::ContentBlock::Image { data, .. } => Some(data.as_str()),
                            _ => None,
                        })
                        .collect();
                    if !images.is_empty() {
                        msg_json["images"] = serde_json::Value::Array(
                            images.iter().map(|d| serde_json::json!(d)).collect(),
                        );
                    }
                }
                messages.push(msg_json);
            }
        }

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "stream": streaming,
            "options": {"num_predict": normalized_request.max_tokens},
        });

        if let Some(tools) = normalized_request.tools {
            body["tools"] = serde_json::Value::Array(tools.to_vec());
        }

        let resp = self
            .http
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(e.to_string()))?;

        if streaming {
            parse_stream(resp, normalized_request.on_text).await
        } else {
            let json: serde_json::Value = resp
                .json()
                .await
                .map_err(|e| LlmError::ParseError(e.to_string()))?;
            Ok(parse_response(&json))
        }
    }
}

async fn parse_stream(
    resp: reqwest::Response,
    on_text: Option<&(dyn Fn(&str) + Send + Sync)>,
) -> Result<LlmResponse, LlmError> {
    let mut full_text = String::new();
    let mut model = String::new();
    let mut tool_calls = Vec::new();
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
            if line.is_empty() {
                continue;
            }

            let Ok(json) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };

            if json.get("done").and_then(serde_json::Value::as_bool) == Some(true) {
                // Extract tool calls from final message
                if let Some(msg) = json.get("message") {
                    extract_tool_calls(msg, &mut tool_calls);
                }
                if let Some(m) = json.get("model").and_then(serde_json::Value::as_str) {
                    model = m.to_string();
                }
                continue;
            }

            if let Some(msg) = json.get("message")
                && let Some(text) = msg.get("content").and_then(serde_json::Value::as_str)
            {
                full_text.push_str(text);
                if let Some(cb) = on_text {
                    cb(text);
                }
            }
        }
    }

    Ok(LlmResponse {
        text: if full_text.is_empty() {
            None
        } else {
            Some(full_text)
        },
        tool_calls,
        usage: Usage::default(),
        model,
    })
}

fn parse_response(json: &serde_json::Value) -> LlmResponse {
    let model = json
        .get("model")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut text = None;
    let mut tool_calls = Vec::new();

    if let Some(msg) = json.get("message") {
        text = msg
            .get("content")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
            .map(String::from);
        extract_tool_calls(msg, &mut tool_calls);
    }

    LlmResponse {
        text,
        tool_calls,
        usage: Usage::default(),
        model,
    }
}

fn extract_tool_calls(msg: &serde_json::Value, tool_calls: &mut Vec<LlmToolCall>) {
    if let Some(tcs) = msg.get("tool_calls").and_then(serde_json::Value::as_array) {
        for tc in tcs {
            if let Some(func) = tc.get("function") {
                let name = func
                    .get("name")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = func
                    .get("arguments")
                    .cloned()
                    .unwrap_or_else(|| serde_json::Value::Object(serde_json::Map::new()));
                tool_calls.push(LlmToolCall {
                    id: format!("ollama_{}", tool_calls.len()),
                    name,
                    input,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_text() {
        let json = serde_json::json!({
            "model": "llama3",
            "message": {"content": "Hello!"},
            "done": true
        });
        let resp = parse_response(&json);
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert_eq!(resp.model, "llama3");
    }

    #[test]
    fn parse_tool_call() {
        let json = serde_json::json!({
            "model": "llama3",
            "message": {
                "content": "",
                "tool_calls": [{
                    "function": {
                        "name": "read",
                        "arguments": {"path": "/tmp"}
                    }
                }]
            },
            "done": true
        });
        let resp = parse_response(&json);
        assert!(resp.text.is_none());
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "read");
        assert_eq!(resp.tool_calls[0].id, "ollama_0");
    }

    #[test]
    fn images_in_message() {
        // Verify the JSON structure built for Ollama multimodal
        let msg = cortex_types::Message::user_with_images(
            "describe",
            vec![("image/png".into(), "base64data".into())],
        );
        let mut msg_json = serde_json::json!({"role": "user", "content": msg.text_content()});
        let images: Vec<&str> = msg
            .content
            .iter()
            .filter_map(|b| match b {
                cortex_types::ContentBlock::Image { data, .. } => Some(data.as_str()),
                _ => None,
            })
            .collect();
        msg_json["images"] =
            serde_json::Value::Array(images.iter().map(|d| serde_json::json!(d)).collect());
        assert!(msg_json.get("images").is_some());
        assert_eq!(msg_json["images"].as_array().unwrap().len(), 1);
    }
}
