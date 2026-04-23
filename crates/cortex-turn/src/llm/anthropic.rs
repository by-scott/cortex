use futures_util::StreamExt;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::client::LlmClient;
use super::types::{LlmError, LlmRequest, LlmResponse, LlmToolCall, Usage};
use super::{max_tokens_for_api, project_messages_for_llm};

pub struct AnthropicClient {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub vision_max_output_tokens: usize,
    capability_cache_ttl_hours: u64,
    learned_vision_max_output_tokens: AtomicUsize,
    capability_cache_path: Option<PathBuf>,
    http: Client,
}

impl AnthropicClient {
    #[must_use]
    pub fn new(
        base_url: &str,
        api_key: &str,
        model: &str,
        vision_max_output_tokens: usize,
        capability_cache_path: &str,
        capability_cache_ttl_hours: u64,
    ) -> Self {
        Self {
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            vision_max_output_tokens,
            capability_cache_ttl_hours,
            learned_vision_max_output_tokens: AtomicUsize::new(0),
            capability_cache_path: (!capability_cache_path.is_empty())
                .then(|| PathBuf::from(capability_cache_path)),
            http: Client::new(),
        }
    }

    fn log_image_request_failure(&self, body: &serde_json::Value, status: reqwest::StatusCode) {
        let mut snapshot = body.clone();
        redact_image_blocks(&mut snapshot);
        tracing::error!(
            model = self.model,
            status = %status,
            request = %snapshot,
            "Anthropic image request failed"
        );
    }

    fn build_message(msg: &cortex_types::Message, contains_images: bool) -> serde_json::Value {
        let role = match msg.role {
            cortex_types::Role::User => "user",
            cortex_types::Role::Assistant => "assistant",
        };
        if msg.has_tool_blocks() || msg.has_images() {
            let blocks: Vec<serde_json::Value> = msg
                .content
                .iter()
                .map(|block| anthropic_block(block, contains_images))
                .collect();
            serde_json::json!({"role": role, "content": blocks})
        } else {
            serde_json::json!({"role": role, "content": msg.text_content()})
        }
    }

    fn build_body(&self, request: &LlmRequest<'_>, contains_images: bool) -> serde_json::Value {
        let messages: Vec<serde_json::Value> = request
            .messages
            .iter()
            .map(|msg| Self::build_message(msg, contains_images))
            .collect();
        let messages = sanitize_anthropic_messages(messages, contains_images);
        log_anthropic_message_shape(&self.model, &messages);
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
        if request.on_text.is_some() {
            body["stream"] = serde_json::Value::Bool(true);
        }
        body
    }

    fn cached_vision_max_output_tokens(&self) -> usize {
        let learned = self
            .learned_vision_max_output_tokens
            .load(Ordering::Relaxed);
        if learned > 0 {
            learned
        } else if let Some(cached) = self.load_persisted_vision_max_output_tokens() {
            self.learned_vision_max_output_tokens
                .store(cached, Ordering::Relaxed);
            cached
        } else {
            self.vision_max_output_tokens
        }
    }

    fn cache_vision_max_output_tokens(&self, max_tokens: usize) {
        if max_tokens > 0 {
            self.learned_vision_max_output_tokens
                .store(max_tokens, Ordering::Relaxed);
            self.persist_vision_max_output_tokens(max_tokens);
        }
    }

    fn cache_key(&self) -> String {
        format!("{}:{}", self.base_url, self.model)
    }

    fn load_persisted_vision_max_output_tokens(&self) -> Option<usize> {
        let path = self.capability_cache_path.as_ref()?;
        let raw = std::fs::read_to_string(path).ok()?;
        let cache: HashMap<String, CachedModelInfo> = serde_json::from_str(&raw).ok()?;
        let info = cache.get(&self.cache_key())?;
        let ttl_hours = if self.capability_cache_ttl_hours == 0 {
            DEFAULT_CAPABILITY_CACHE_TTL_HOURS
        } else {
            self.capability_cache_ttl_hours
        };
        if info.is_expired_with_ttl(ttl_hours) {
            None
        } else {
            info.vision_max_output_tokens
        }
    }

    fn persist_vision_max_output_tokens(&self, max_tokens: usize) {
        let Some(path) = self.capability_cache_path.as_ref() else {
            return;
        };
        let mut cache: HashMap<String, CachedModelInfo> = std::fs::read_to_string(path)
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        let entry = cache
            .entry(self.cache_key())
            .or_insert_with(CachedModelInfo::new);
        entry.vision_max_output_tokens = Some(max_tokens);
        entry.fetched_at = chrono::Utc::now();
        if let Ok(json) = serde_json::to_string_pretty(&cache) {
            let _ = std::fs::write(path, json);
        }
    }

    async fn send_body_with_transient_retries(
        &self,
        url: &str,
        body: &serde_json::Value,
        transient_retries: &mut usize,
        max_transient_retries: usize,
    ) -> Result<reqwest::Response, LlmError> {
        loop {
            match self
                .http
                .post(url)
                .header("x-api-key", &self.api_key)
                .header("anthropic-version", "2023-06-01")
                .header("anthropic-beta", "advanced-tool-use-2025-11-20")
                .header("content-type", "application/json")
                .json(body)
                .send()
                .await
            {
                Ok(resp) => return Ok(resp),
                Err(error)
                    if should_retry_transient_llm_error(
                        &error.to_string(),
                        *transient_retries,
                        max_transient_retries,
                    ) =>
                {
                    *transient_retries += 1;
                    trace_transient_llm_retry(&self.model, *transient_retries, &error.to_string());
                    tokio::time::sleep(transient_retry_delay(*transient_retries)).await;
                }
                Err(error) => return Err(LlmError::RequestFailed(error.to_string())),
            }
        }
    }

    async fn handle_unsuccessful_response(
        &self,
        resp: reqwest::Response,
        body: &serde_json::Value,
        contains_images: bool,
        attempt_max_tokens: usize,
    ) -> Result<usize, LlmError> {
        let status = resp.status();
        if contains_images {
            self.log_image_request_failure(body, status);
        }
        let body_text = resp
            .text()
            .await
            .unwrap_or_else(|_| String::from("<no body>"));
        if contains_images
            && is_invalid_parameter_error(&body_text)
            && let Some(next_max_tokens) = next_vision_probe_max_tokens(attempt_max_tokens)
        {
            tracing::warn!(
                model = self.model,
                max_tokens = attempt_max_tokens,
                retry_max_tokens = next_max_tokens,
                "Anthropic image request rejected max_tokens; retrying with lower cap"
            );
            return Ok(next_max_tokens);
        }
        Err(LlmError::RequestFailed(format!(
            "HTTP {status}: {body_text}"
        )))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedModelInfo {
    #[serde(default)]
    context_window: usize,
    #[serde(default)]
    max_output_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    vision_max_output_tokens: Option<usize>,
    fetched_at: chrono::DateTime<chrono::Utc>,
}

const DEFAULT_CAPABILITY_CACHE_TTL_HOURS: u64 = 168;

impl CachedModelInfo {
    fn new() -> Self {
        Self {
            context_window: 0,
            max_output_tokens: 0,
            vision_max_output_tokens: None,
            fetched_at: chrono::Utc::now(),
        }
    }

    fn is_expired_with_ttl(&self, ttl_hours: u64) -> bool {
        chrono::Utc::now()
            .signed_duration_since(self.fetched_at)
            .num_hours()
            >= i64::try_from(ttl_hours).unwrap_or(i64::MAX)
    }
}

#[async_trait::async_trait]
impl LlmClient for AnthropicClient {
    fn supports_tools_with_images(&self) -> bool {
        false
    }

    async fn complete(&self, request: LlmRequest<'_>) -> Result<LlmResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url);
        let streaming = request.on_text.is_some();
        let projection = project_messages_for_llm(request.messages);
        let normalized_messages = projection.messages;
        log_projection_diagnostics(&self.model, &projection.diagnostics);
        let contains_images = normalized_messages
            .iter()
            .any(cortex_types::Message::has_images);
        let max_tokens = max_tokens_for_api(
            request.max_tokens,
            &normalized_messages,
            self.cached_vision_max_output_tokens(),
        );

        let mut attempt_max_tokens = max_tokens;
        let mut transient_retries = 0usize;
        let max_transient_retries = request.transient_retries;
        loop {
            let normalized_request = LlmRequest {
                system: request.system,
                messages: &normalized_messages,
                tools: request.tools,
                max_tokens: attempt_max_tokens,
                transient_retries: request.transient_retries,
                on_text: request.on_text,
            };
            let body = self.build_body(&normalized_request, contains_images);

            let resp = self
                .send_body_with_transient_retries(
                    &url,
                    &body,
                    &mut transient_retries,
                    max_transient_retries,
                )
                .await?;

            let status = resp.status();
            let result = if status.is_success() && streaming {
                parse_stream(resp, request.on_text, contains_images.then_some(body)).await
            } else if status.is_success() {
                match resp.json::<serde_json::Value>().await {
                    Ok(json) => ensure_non_empty_response(parse_response(&json)),
                    Err(error) => Err(StreamFailure {
                        error: LlmError::ParseError(error.to_string()).to_string(),
                        emitted_text: false,
                    }),
                }
            } else {
                let next_max_tokens = self
                    .handle_unsuccessful_response(resp, &body, contains_images, attempt_max_tokens)
                    .await?;
                attempt_max_tokens = next_max_tokens;
                continue;
            };

            match result {
                Ok(response) => {
                    if contains_images {
                        self.cache_vision_max_output_tokens(attempt_max_tokens);
                    }
                    return Ok(response);
                }
                Err(StreamFailure {
                    error,
                    emitted_text: false,
                }) if contains_images
                    && is_invalid_parameter_error(&error)
                    && next_vision_probe_max_tokens(attempt_max_tokens).is_some() =>
                {
                    let next_max_tokens = next_vision_probe_max_tokens(attempt_max_tokens)
                        .unwrap_or(attempt_max_tokens);
                    tracing::warn!(
                        model = self.model,
                        max_tokens = attempt_max_tokens,
                        retry_max_tokens = next_max_tokens,
                        "Anthropic image stream rejected max_tokens; retrying with lower cap"
                    );
                    attempt_max_tokens = next_max_tokens;
                }
                Err(failure)
                    if !failure.emitted_text
                        && should_retry_transient_llm_error(
                            &failure.error,
                            transient_retries,
                            max_transient_retries,
                        ) =>
                {
                    transient_retries += 1;
                    trace_transient_llm_retry(&self.model, transient_retries, &failure.error);
                    tokio::time::sleep(transient_retry_delay(transient_retries)).await;
                }
                Err(failure) => return Err(LlmError::RequestFailed(failure.error)),
            }
        }
    }
}

fn should_retry_transient_llm_error(error: &str, retries: usize, max_retries: usize) -> bool {
    retries < max_retries && is_transient_llm_error(error)
}

fn is_transient_llm_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    [
        "error sending request",
        "empty anthropic response",
        "empty anthropic stream response",
        "network error",
        "try again later",
        "connection reset",
        "connection closed",
        "connection refused",
        "connection aborted",
        "connection timed out",
        "operation timed out",
        "unexpected eof",
        "dns error",
        "tcp connect error",
        "tls handshake",
        "temporary failure",
        "timed out",
        "timeout",
        "502",
        "503",
        "504",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
}

fn transient_retry_delay(retry: usize) -> std::time::Duration {
    std::time::Duration::from_millis(250 * u64::try_from(retry).unwrap_or(1))
}

fn ensure_non_empty_response(response: LlmResponse) -> Result<LlmResponse, StreamFailure> {
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

fn trace_transient_llm_retry(model: &str, retry: usize, error: &str) {
    tracing::warn!(
        model,
        retry,
        error,
        "Anthropic-compatible request failed transiently; retrying"
    );
}

fn next_vision_probe_max_tokens(current: usize) -> Option<usize> {
    const MIN_VISION_MAX_OUTPUT_TOKENS: usize = 1024;
    if current <= MIN_VISION_MAX_OUTPUT_TOKENS {
        None
    } else {
        Some((current / 2).max(MIN_VISION_MAX_OUTPUT_TOKENS))
    }
}

fn is_invalid_parameter_error(error: &str) -> bool {
    let lower = error.to_ascii_lowercase();
    lower.contains("invalid api parameter")
        || lower.contains("max_tokens")
        || lower.contains("invalid parameter")
}

fn anthropic_block(block: &cortex_types::ContentBlock, contains_images: bool) -> serde_json::Value {
    match block {
        cortex_types::ContentBlock::Text { text } => {
            serde_json::json!({"type": "text", "text": text})
        }
        cortex_types::ContentBlock::ToolUse { id, name, input } => {
            if contains_images {
                serde_json::json!({
                    "type": "text",
                    "text": format!("[tool_use:{}:{} {}]", id, name, input)
                })
            } else {
                serde_json::json!({"type": "tool_use", "id": id, "name": name, "input": input})
            }
        }
        cortex_types::ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => {
            if contains_images {
                serde_json::json!({
                    "type": "text",
                    "text": format!("[tool_result:{} error={} {}]", tool_use_id, is_error, content)
                })
            } else {
                serde_json::json!({"type": "tool_result", "tool_use_id": tool_use_id, "content": content, "is_error": is_error})
            }
        }
        cortex_types::ContentBlock::Image { media_type, data } => {
            serde_json::json!({"type": "image", "source": {"type": "base64", "media_type": media_type, "data": data}})
        }
    }
}

fn sanitize_anthropic_messages(
    messages: Vec<serde_json::Value>,
    contains_images: bool,
) -> Vec<serde_json::Value> {
    let mut normalized = Vec::with_capacity(messages.len());
    let mut pending_tool_ids: Vec<String> = Vec::new();

    for message in messages {
        let role = message
            .get("role")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("user");
        if role == "assistant" && !pending_tool_ids.is_empty() {
            let missing = drain_missing_tool_results(&mut pending_tool_ids);
            push_anthropic_message(&mut normalized, "user", missing);
        }

        let blocks = anthropic_message_blocks(&message);
        let mut sanitized_blocks = Vec::with_capacity(blocks.len());
        let mut saw_valid_tool_result = false;

        for block in blocks {
            let block_type = block
                .get("type")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("text");
            match (role, block_type) {
                ("assistant", "tool_use") if !contains_images => {
                    if let Some(id) = block.get("id").and_then(serde_json::Value::as_str) {
                        pending_tool_ids.push(id.to_string());
                    }
                    sanitized_blocks.push(block);
                }
                ("user", "tool_result") if !contains_images => {
                    let tool_use_id = block
                        .get("tool_use_id")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    if let Some(pos) = pending_tool_ids.iter().position(|id| id == tool_use_id) {
                        pending_tool_ids.remove(pos);
                        saw_valid_tool_result = true;
                        sanitized_blocks.push(block);
                    } else {
                        sanitized_blocks.push(tool_result_as_text(&block));
                    }
                }
                (_, "tool_use" | "tool_result") => {
                    sanitized_blocks.push(tool_result_as_text(&block));
                }
                _ => sanitized_blocks.push(block),
            }
        }

        if role == "user" && !pending_tool_ids.is_empty() && !saw_valid_tool_result {
            let mut with_missing = drain_missing_tool_results(&mut pending_tool_ids);
            with_missing.extend(sanitized_blocks);
            sanitized_blocks = with_missing;
        }

        if sanitized_blocks.is_empty() {
            continue;
        }

        push_anthropic_message(&mut normalized, role, sanitized_blocks);
    }

    if !pending_tool_ids.is_empty() {
        let missing = drain_missing_tool_results(&mut pending_tool_ids);
        push_anthropic_message(&mut normalized, "user", missing);
    }

    ensure_anthropic_starts_with_user(&mut normalized);
    normalized
}

fn ensure_anthropic_starts_with_user(messages: &mut Vec<serde_json::Value>) {
    if messages
        .first()
        .and_then(|message| message.get("role"))
        .and_then(serde_json::Value::as_str)
        == Some("user")
    {
        return;
    }

    messages.insert(
        0,
        serde_json::json!({
            "role": "user",
            "content": [{"type": "text", "text": "(Earlier conversation omitted.)"}]
        }),
    );
}

fn drain_missing_tool_results(pending_tool_ids: &mut Vec<String>) -> Vec<serde_json::Value> {
    pending_tool_ids
        .drain(..)
        .map(|tool_use_id| {
            serde_json::json!({
                "type": "tool_result",
                "tool_use_id": tool_use_id,
                "content": "(tool result unavailable)",
                "is_error": true
            })
        })
        .collect()
}

fn anthropic_message_blocks(message: &serde_json::Value) -> Vec<serde_json::Value> {
    match message.get("content") {
        Some(serde_json::Value::Array(blocks)) => blocks.clone(),
        Some(serde_json::Value::String(text)) if !text.trim().is_empty() => {
            vec![serde_json::json!({"type": "text", "text": text})]
        }
        Some(serde_json::Value::String(_)) | None => Vec::new(),
        Some(other) => vec![serde_json::json!({"type": "text", "text": other.to_string()})],
    }
}

fn push_anthropic_message(
    messages: &mut Vec<serde_json::Value>,
    role: &str,
    blocks: Vec<serde_json::Value>,
) {
    if let Some(last) = messages.last_mut()
        && last.get("role").and_then(serde_json::Value::as_str) == Some(role)
    {
        if let Some(content) = last
            .get_mut("content")
            .and_then(serde_json::Value::as_array_mut)
        {
            content.extend(blocks);
        }
        return;
    }

    messages.push(serde_json::json!({"role": role, "content": blocks}));
}

fn tool_result_as_text(block: &serde_json::Value) -> serde_json::Value {
    let block_type = block
        .get("type")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("tool");
    let id = block
        .get("tool_use_id")
        .or_else(|| block.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("unknown");
    let content = block
        .get("content")
        .or_else(|| block.get("input"))
        .map_or_else(String::new, serde_json::Value::to_string);
    serde_json::json!({
        "type": "text",
        "text": format!("[{block_type}:{id}] {content}")
    })
}

fn log_anthropic_message_shape(model: &str, messages: &[serde_json::Value]) {
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
            let block_shape = message
                .get("content")
                .and_then(serde_json::Value::as_array)
                .map_or_else(
                    || "non_array".to_string(),
                    |blocks| {
                        blocks
                            .iter()
                            .map(|block| {
                                block
                                    .get("type")
                                    .and_then(serde_json::Value::as_str)
                                    .unwrap_or("unknown")
                            })
                            .collect::<Vec<_>>()
                            .join("+")
                    },
                );
            format!("#{idx}:role={role},blocks={block_shape}")
        })
        .collect::<Vec<_>>()
        .join(" | ");
    tracing::info!(
        target: "cortex_turn::llm::anthropic",
        model = model,
        message_count = messages.len(),
        shape = shape,
        "Anthropic-compatible request message shape"
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

fn redact_image_blocks(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            if let Some(source) = map.get_mut("source")
                && let Some(src_obj) = source.as_object_mut()
                && let Some(data) = src_obj.get_mut("data")
                && let Some(raw) = data.as_str()
            {
                *data = serde_json::Value::String(format!("<base64:{} chars>", raw.len()));
            }
            for child in map.values_mut() {
                redact_image_blocks(child);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                redact_image_blocks(item);
            }
        }
        _ => {}
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
            self.usage.input_tokens = u
                .get("input_tokens")
                .and_then(serde_json::Value::as_u64)
                .map_or(0, |v| usize::try_from(v).unwrap_or(0));
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
        // Some providers (e.g. ZAI) send input_tokens in message_delta instead
        // of message_start.
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

struct StreamFailure {
    error: String,
    emitted_text: bool,
}

async fn parse_stream(
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

    #[test]
    fn stream_accumulator_accepts_text_on_block_start() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(
            &serde_json::json!({
                "type": "content_block_start",
                "content_block": {"type": "text", "text": "final answer"}
            }),
            None,
        );

        let response = acc.into_response();
        assert_eq!(response.text.as_deref(), Some("final answer"));
        assert!(response.tool_calls.is_empty());
    }

    #[test]
    fn stream_accumulator_accepts_tool_input_on_block_start() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(
            &serde_json::json!({
                "type": "content_block_start",
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "bash",
                    "input": {"cmd": "pwd"}
                }
            }),
            None,
        );
        acc.process_event(&serde_json::json!({"type": "content_block_stop"}), None);

        let response = acc.into_response();
        assert!(response.text.is_none());
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(response.tool_calls[0].id, "toolu_1");
        assert_eq!(response.tool_calls[0].name, "bash");
        assert_eq!(
            response.tool_calls[0].input,
            serde_json::json!({"cmd": "pwd"})
        );
    }

    #[test]
    fn stream_accumulator_prefers_tool_input_delta_over_start_placeholder() {
        let mut acc = StreamAccumulator::new();
        acc.process_event(
            &serde_json::json!({
                "type": "content_block_start",
                "content_block": {
                    "type": "tool_use",
                    "id": "toolu_1",
                    "name": "bash",
                    "input": {"placeholder": true}
                }
            }),
            None,
        );
        acc.process_event(
            &serde_json::json!({
                "type": "content_block_delta",
                "delta": {"type": "input_json_delta", "partial_json": "{\"cmd\":\"pwd\"}"}
            }),
            None,
        );
        acc.process_event(&serde_json::json!({"type": "content_block_stop"}), None);

        let response = acc.into_response();
        assert_eq!(response.tool_calls.len(), 1);
        assert_eq!(
            response.tool_calls[0].input,
            serde_json::json!({"cmd": "pwd"})
        );
    }

    #[test]
    fn rejects_empty_anthropic_response() {
        let response = LlmResponse {
            text: Some("   ".into()),
            tool_calls: Vec::new(),
            usage: Usage::default(),
            model: "mock".into(),
        };

        let err = ensure_non_empty_response(response).unwrap_err();
        assert_eq!(err.error, "empty Anthropic response");
    }

    #[test]
    fn classifies_provider_network_errors_as_transient() {
        assert!(is_transient_llm_error(
            "Stream error: Network error, please try again later"
        ));
        assert!(is_transient_llm_error("empty Anthropic stream response"));
        assert!(is_transient_llm_error(
            "error sending request for url (https://api.z.ai/api/anthropic/v1/messages)"
        ));
        assert!(!is_transient_llm_error("messages parameter is illegal"));
        assert!(should_retry_transient_llm_error("network error", 0, 1));
        assert!(should_retry_transient_llm_error(
            "error sending request for url (https://api.z.ai/api/anthropic/v1/messages)",
            0,
            5
        ));
        assert!(!should_retry_transient_llm_error(
            "error sending request for url (https://api.z.ai/api/anthropic/v1/messages)",
            5,
            5
        ));
        assert!(!should_retry_transient_llm_error("network error", 0, 0));
    }

    #[test]
    fn sanitize_inserts_user_anchor_before_leading_assistant() {
        let messages = vec![
            serde_json::json!({
                "role": "assistant",
                "content": [{"type": "tool_use", "id": "t1", "name": "bash", "input": {}}]
            }),
            serde_json::json!({
                "role": "user",
                "content": [{"type": "tool_result", "tool_use_id": "t1", "content": "ok"}]
            }),
        ];

        let sanitized = sanitize_anthropic_messages(messages, false);

        assert_eq!(
            sanitized[0].get("role").and_then(serde_json::Value::as_str),
            Some("user")
        );
        assert_eq!(
            sanitized[1].get("role").and_then(serde_json::Value::as_str),
            Some("assistant")
        );
    }
}
