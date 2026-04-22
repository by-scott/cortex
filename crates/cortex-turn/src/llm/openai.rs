use std::collections::{HashMap, VecDeque};

use base64::Engine;
use futures_util::StreamExt;
use reqwest::Client;

use super::client::LlmClient;
use super::types::{
    AuthType, LlmError, LlmRequest, LlmResponse, LlmToolCall, OpenAiImageInputMode,
    ResolvedEndpoint, Usage,
};
use super::{max_tokens_for_api, project_messages_for_llm};

pub struct OpenAIClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub auth_type: AuthType,
    pub image_input_mode: OpenAiImageInputMode,
    pub files_base_url: String,
    pub stream_options: bool,
    pub vision_max_output_tokens: usize,
    http: Client,
}

impl OpenAIClient {
    #[must_use]
    pub fn new(endpoint: &ResolvedEndpoint) -> Self {
        Self {
            base_url: endpoint.base_url.clone(),
            api_key: endpoint.api_key.clone(),
            model: endpoint.model.clone(),
            auth_type: endpoint.auth_type.clone(),
            image_input_mode: endpoint.image_input_mode.clone(),
            files_base_url: endpoint.files_base_url.clone(),
            stream_options: endpoint.openai_stream_options,
            vision_max_output_tokens: endpoint.vision_max_output_tokens,
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
        let projection = project_messages_for_llm(request.messages);
        let normalized_messages = projection.messages;
        log_projection_diagnostics(&self.model, &projection.diagnostics);
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

        let messages = self.build_messages(&normalized_request).await?;
        log_openai_message_shape(&self.model, &messages);

        let mut body = serde_json::json!({
            "model": self.model,
            "max_tokens": normalized_request.max_tokens,
            "messages": messages,
        });

        if let Some(tools) = normalized_request.tools {
            body["tools"] = serde_json::Value::Array(
                tools.iter().map(openai_tool_definition).collect::<Vec<_>>(),
            );
        }
        if streaming {
            body["stream"] = serde_json::Value::Bool(true);
            if self.stream_options {
                body["stream_options"] = serde_json::json!({"include_usage": true});
            }
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

fn openai_tool_definition(tool: &serde_json::Value) -> serde_json::Value {
    let name = tool
        .get("name")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let description = tool
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    let parameters = tool
        .get("input_schema")
        .cloned()
        .unwrap_or_else(|| serde_json::json!({"type": "object"}));

    serde_json::json!({
        "type": "function",
        "function": {
            "name": name,
            "description": description,
            "parameters": parameters,
        }
    })
}

fn append_tool_message(
    msg: &cortex_types::Message,
    role: &str,
    pending_tool_call_ids: &mut VecDeque<String>,
    messages: &mut Vec<serde_json::Value>,
) {
    let mut tool_calls_json = Vec::new();
    let mut text_parts = Vec::new();

    for block in &msg.content {
        match block {
            cortex_types::ContentBlock::Text { text } => {
                text_parts.push(text.clone());
            }
            cortex_types::ContentBlock::ToolUse { id, name, input } => {
                pending_tool_call_ids.push_back(id.clone());
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
            } => append_tool_result(
                tool_use_id,
                content,
                pending_tool_call_ids,
                messages,
                &mut text_parts,
            ),
            cortex_types::ContentBlock::Image { .. } => {}
        }
    }

    append_tool_text_or_calls(role, &tool_calls_json, &text_parts, messages);
}

fn append_tool_result(
    tool_use_id: &str,
    content: &str,
    pending_tool_call_ids: &mut VecDeque<String>,
    messages: &mut Vec<serde_json::Value>,
    text_parts: &mut Vec<String>,
) {
    if pending_tool_call_ids
        .front()
        .is_some_and(|id| id == tool_use_id)
    {
        let _ = pending_tool_call_ids.pop_front();
        messages.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_use_id,
            "content": normalize_tool_result_content(content)
        }));
    } else {
        text_parts.push(format!(
            "[orphan_tool_result:{tool_use_id}] {}",
            normalize_tool_result_content(content)
        ));
    }
}

fn append_tool_text_or_calls(
    role: &str,
    tool_calls_json: &[serde_json::Value],
    text_parts: &[String],
    messages: &mut Vec<serde_json::Value>,
) {
    if !tool_calls_json.is_empty() {
        let mut assistant_msg = serde_json::json!({
            "role": "assistant",
            "tool_calls": tool_calls_json
        });
        if !text_parts.is_empty() {
            assistant_msg["content"] = serde_json::Value::String(text_parts.join(""));
        }
        messages.push(assistant_msg);
    } else if !text_parts.is_empty() {
        messages.push(serde_json::json!({
            "role": role,
            "content": text_parts.join("\n")
        }));
    }
}

impl OpenAIClient {
    async fn build_messages(
        &self,
        request: &LlmRequest<'_>,
    ) -> Result<Vec<serde_json::Value>, LlmError> {
        let mut messages = Vec::new();

        if let Some(system) = request.system {
            messages.push(serde_json::json!({"role": "system", "content": system}));
        }

        let mut pending_tool_call_ids = VecDeque::new();
        for msg in request.messages {
            let role = match msg.role {
                cortex_types::Role::User => "user",
                cortex_types::Role::Assistant => "assistant",
            };

            if msg.has_tool_blocks() {
                append_tool_message(msg, role, &mut pending_tool_call_ids, &mut messages);
            } else if msg.has_images() {
                let content_blocks = self.build_image_content_blocks(msg).await?;
                messages.push(serde_json::json!({"role": role, "content": content_blocks}));
            } else {
                messages.push(serde_json::json!({"role": role, "content": msg.text_content()}));
            }
        }

        Ok(sanitize_openai_message_sequence(messages))
    }

    async fn build_image_content_blocks(
        &self,
        msg: &cortex_types::Message,
    ) -> Result<Vec<serde_json::Value>, LlmError> {
        let mut content_blocks = Vec::new();
        for block in &msg.content {
            match block {
                cortex_types::ContentBlock::Text { text } => {
                    content_blocks.push(serde_json::json!({"type": "text", "text": text}));
                }
                cortex_types::ContentBlock::Image { media_type, data } => {
                    let url = self.openai_image_url(media_type, data).await?;
                    content_blocks.push(serde_json::json!({
                        "type": "image_url",
                        "image_url": {"url": url}
                    }));
                }
                _ => {}
            }
        }
        Ok(content_blocks)
    }

    async fn openai_image_url(&self, media_type: &str, data: &str) -> Result<String, LlmError> {
        match self.image_input_mode {
            OpenAiImageInputMode::DataUrl => Ok(format!("data:{media_type};base64,{data}")),
            OpenAiImageInputMode::UploadThenUrl => {
                self.upload_image_and_get_content_url(media_type, data)
                    .await
            }
            OpenAiImageInputMode::RemoteUrlOnly => Err(LlmError::RequestFailed(
                "provider only accepts remote image URLs".to_string(),
            )),
        }
    }

    async fn upload_image_and_get_content_url(
        &self,
        media_type: &str,
        data: &str,
    ) -> Result<String, LlmError> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|e| LlmError::ParseError(format!("decode image base64: {e}")))?;
        let ext = match media_type {
            "image/png" => "png",
            "image/webp" => "webp",
            "image/gif" => "gif",
            _ => "jpg",
        };
        let part = reqwest::multipart::Part::bytes(bytes)
            .file_name(format!("upload.{ext}"))
            .mime_str(media_type)
            .map_err(|e| LlmError::RequestFailed(format!("image mime: {e}")))?;
        let form = reqwest::multipart::Form::new()
            .text("purpose", "agent")
            .part("file", part);

        let files_url = openai_compat_files_endpoint(self.files_base());
        let mut req = self.http.post(&files_url).multipart(form);
        if !self.api_key.is_empty() {
            req = match self.auth_type {
                AuthType::XApiKey => req.header("x-api-key", &self.api_key),
                _ => req.bearer_auth(&self.api_key),
            };
        }
        let resp = req
            .send()
            .await
            .map_err(|e| LlmError::RequestFailed(format!("image upload failed: {e}")))?;
        let status = resp.status();
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| LlmError::ParseError(format!("parse image upload response: {e}")))?;
        if !status.is_success() {
            return Err(LlmError::RequestFailed(format!("HTTP {status}: {json}")));
        }
        let Some(file_id) = json.get("id").and_then(serde_json::Value::as_str) else {
            return Err(LlmError::ParseError(format!(
                "missing file id in upload response: {json}"
            )));
        };
        Ok(openai_compat_file_content_url(self.files_base(), file_id))
    }

    fn files_base(&self) -> &str {
        if self.files_base_url.is_empty() {
            &self.base_url
        } else {
            &self.files_base_url
        }
    }
}

fn sanitize_openai_message_sequence(messages: Vec<serde_json::Value>) -> Vec<serde_json::Value> {
    let mut sanitized = Vec::with_capacity(messages.len());
    let mut expected_tool_ids = VecDeque::new();

    for mut message in messages {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();

        if role == "tool" {
            let tool_call_id = message
                .get("tool_call_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string();
            if expected_tool_ids
                .front()
                .is_some_and(|expected| expected == &tool_call_id)
            {
                let _ = expected_tool_ids.pop_front();
                sanitized.push(message);
            } else {
                sanitized.push(orphan_tool_message_to_user_context(&message));
            }
            continue;
        }

        if !expected_tool_ids.is_empty() {
            flush_unanswered_tool_calls(&mut sanitized, &mut expected_tool_ids);
        }

        if role == "assistant"
            && let Some(tool_calls) = message
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
        {
            for tool_call in tool_calls {
                if let Some(id) = tool_call.get("id").and_then(serde_json::Value::as_str) {
                    expected_tool_ids.push_back(id.to_string());
                }
            }
            if message
                .get("content")
                .is_some_and(serde_json::Value::is_null)
            {
                let _ = message.as_object_mut().map(|obj| obj.remove("content"));
            }
        }

        sanitized.push(message);
    }

    if !expected_tool_ids.is_empty() {
        flush_unanswered_tool_calls(&mut sanitized, &mut expected_tool_ids);
    }

    sanitized
}

fn flush_unanswered_tool_calls(
    sanitized: &mut Vec<serde_json::Value>,
    expected_tool_ids: &mut VecDeque<String>,
) {
    while let Some(tool_call_id) = expected_tool_ids.pop_front() {
        sanitized.push(serde_json::json!({
            "role": "tool",
            "tool_call_id": tool_call_id,
            "content": "(tool result unavailable)"
        }));
    }
}

fn orphan_tool_message_to_user_context(message: &serde_json::Value) -> serde_json::Value {
    let tool_call_id = message
        .get("tool_call_id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let content = message
        .get("content")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default();
    serde_json::json!({
        "role": "user",
        "content": format!(
            "[orphan_tool_result:{tool_call_id}] {}",
            normalize_tool_result_content(content)
        )
    })
}

fn normalize_tool_result_content(content: &str) -> String {
    if content.trim().is_empty() {
        "(empty tool result)".to_string()
    } else {
        content.to_string()
    }
}

fn log_openai_message_shape(model: &str, messages: &[serde_json::Value]) {
    if std::env::var_os("CORTEX_LOG_LLM_MESSAGE_SHAPE").is_none() {
        return;
    }

    let shape = messages
        .iter()
        .enumerate()
        .map(|(idx, message)| {
            let role = message
                .get("role")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("missing");
            let content_kind = match message.get("content") {
                Some(serde_json::Value::String(text)) if text.is_empty() => "empty_string",
                Some(serde_json::Value::String(_)) => "string",
                Some(serde_json::Value::Array(_)) => "array",
                Some(serde_json::Value::Null) => "null",
                Some(_) => "other",
                None => "missing",
            };
            let tool_calls = message
                .get("tool_calls")
                .and_then(serde_json::Value::as_array)
                .map_or(0, Vec::len);
            let tool_call_id = message
                .get("tool_call_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            format!(
                "#{idx}:role={role},content={content_kind},tool_calls={tool_calls},tool_call_id={tool_call_id}"
            )
        })
        .collect::<Vec<_>>()
        .join(" | ");
    tracing::info!(
        target: "cortex_turn::llm::openai",
        model = model,
        message_count = messages.len(),
        shape = shape,
        "OpenAI-compatible request message shape"
    );
}

fn log_projection_diagnostics(model: &str, diagnostics: &super::projection::ProjectionDiagnostics) {
    if std::env::var_os("CORTEX_LOG_LLM_MESSAGE_SHAPE").is_none() {
        return;
    }
    tracing::info!(
        target: "cortex_turn::llm::projection",
        model = model,
        inserted_user_anchor = diagnostics.inserted_user_anchor,
        synthetic_tool_results = diagnostics.synthetic_tool_results,
        orphan_tool_results = diagnostics.orphan_tool_results,
        duplicate_tool_uses = diagnostics.duplicate_tool_uses,
        empty_messages_removed = diagnostics.empty_messages_removed,
        "LLM projection diagnostics"
    );
}

fn openai_compat_files_endpoint(base_url: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.ends_with("/v4") {
        format!("{base}/files")
    } else {
        format!("{base}/v1/files")
    }
}

fn openai_compat_file_content_url(base_url: &str, file_id: &str) -> String {
    let base = base_url.trim_end_matches('/');
    if base.ends_with("/v1") || base.ends_with("/v4") {
        format!("{base}/files/{file_id}/content")
    } else {
        format!("{base}/v1/files/{file_id}/content")
    }
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

    #[test]
    fn wraps_tool_definition_for_openai() {
        let tool = serde_json::json!({
            "name": "read",
            "description": "Read a file",
            "input_schema": {"type": "object", "properties": {"path": {"type": "string"}}}
        });
        let wrapped = openai_tool_definition(&tool);
        assert_eq!(wrapped["type"], "function");
        assert_eq!(wrapped["function"]["name"], "read");
        assert_eq!(wrapped["function"]["description"], "Read a file");
        assert_eq!(wrapped["function"]["parameters"]["type"], "object");
    }

    #[test]
    fn zai_file_endpoints_reuse_v4_base() {
        let base = "https://api.z.ai/api/paas/v4/";
        assert_eq!(
            openai_compat_files_endpoint(base),
            "https://api.z.ai/api/paas/v4/files"
        );
        assert_eq!(
            openai_compat_file_content_url(base, "file-123"),
            "https://api.z.ai/api/paas/v4/files/file-123/content"
        );
    }
}
