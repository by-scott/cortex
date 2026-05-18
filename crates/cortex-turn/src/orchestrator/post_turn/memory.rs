use cortex_types::Message;

use crate::llm::LlmClient;

/// Parse the LLM memory extraction response into `MemoryEntry` objects.
///
/// Expected JSON format: `[{"type": "User|Feedback|Project|Reference", "description": "...", "content": "..."}]`
#[must_use]
pub fn parse_memory_extract_response(response: &str) -> Vec<cortex_types::MemoryEntry> {
    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .and_then(|s| s.rsplit_once("```"))
            .map_or(trimmed, |(content, _)| content.trim())
    } else {
        trimmed
    };

    let parsed: Vec<serde_json::Value> = match serde_json::from_str::<serde_json::Value>(json_str) {
        Ok(serde_json::Value::Array(arr)) => arr,
        Ok(serde_json::Value::Object(map)) => [
            "memories",
            "memory_candidates",
            "candidates",
            "items",
            "results",
        ]
        .iter()
        .find_map(|key| match map.get(*key) {
            Some(serde_json::Value::Array(arr)) => Some(arr.clone()),
            _ => None,
        })
        .unwrap_or_else(|| vec![serde_json::Value::Object(map)]),
        Ok(_) | Err(_) => return Vec::new(),
    };

    parsed
        .iter()
        .filter_map(|v| {
            let desc = v.get("description")?.as_str()?;
            let content = v.get("content")?.as_str()?;
            if desc.is_empty() || content.is_empty() {
                return None;
            }
            let memory_type = match v.get("type").and_then(|t| t.as_str()).unwrap_or("Project") {
                "User" => cortex_types::MemoryType::User,
                "Feedback" => cortex_types::MemoryType::Feedback,
                "Reference" => cortex_types::MemoryType::Reference,
                _ => cortex_types::MemoryType::Project,
            };
            let kind = match v.get("kind").and_then(|k| k.as_str()).unwrap_or("Episodic") {
                "Semantic" => cortex_types::MemoryKind::Semantic,
                _ => cortex_types::MemoryKind::Episodic,
            };
            let source = match v
                .get("source")
                .and_then(|s| s.as_str())
                .unwrap_or("LlmGenerated")
            {
                "UserInput" => cortex_types::MemorySource::UserInput,
                "ToolOutput" => cortex_types::MemorySource::ToolOutput,
                "Network" => cortex_types::MemorySource::Network,
                _ => cortex_types::MemorySource::LlmGenerated,
            };
            let confidence = v
                .get("confidence")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0)
                .clamp(0.0, 1.0);
            let mut entry = cortex_types::MemoryEntry::new(
                content.to_string(),
                desc.to_string(),
                memory_type,
                kind,
            );
            entry.source = source;
            entry.strength = confidence;
            entry.add_evidence(cortex_types::MemoryEvidence::new(
                "post_turn_extract",
                source,
                confidence,
                desc,
            ));
            if source == cortex_types::MemorySource::UserInput {
                entry.confirm_by_user();
            }
            Some(entry)
        })
        .collect()
}

/// Capture explicit user memory directives without depending on an LLM extraction pass.
#[must_use]
pub fn extract_explicit_user_memories(input: &str) -> Vec<cortex_types::MemoryEntry> {
    let Some(content) = explicit_memory_content(input) else {
        return Vec::new();
    };
    let memory_type = if contains_any(&content, &["偏好", "prefer", "preference"]) {
        cortex_types::MemoryType::User
    } else {
        cortex_types::MemoryType::Project
    };
    let kind = if contains_any(input, &["长期", "durable", "always", "以后", "preference"]) {
        cortex_types::MemoryKind::Semantic
    } else {
        cortex_types::MemoryKind::Episodic
    };
    let description = summarize_explicit_memory(&content);
    let mut entry = cortex_types::MemoryEntry::new(content, description, memory_type, kind);
    entry.source = cortex_types::MemorySource::UserInput;
    entry.strength = 0.95;
    entry.confirm_by_user();
    entry.add_evidence(cortex_types::MemoryEvidence::new(
        "explicit_user_memory",
        cortex_types::MemorySource::UserInput,
        0.95,
        "explicit user memory directive",
    ));
    vec![entry]
}

fn explicit_memory_content(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if trimmed.starts_with('/') {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    let marker = if let Some(idx) = trimmed.find("记住") {
        idx + "记住".len()
    } else if let Some(idx) = lower.find("remember") {
        idx + "remember".len()
    } else {
        return None;
    };
    let content = trimmed[marker..]
        .trim_start_matches(|c: char| c.is_whitespace() || matches!(c, ':' | '：' | ',' | '，'))
        .trim();
    if content.chars().count() < 6 {
        None
    } else {
        Some(content.to_string())
    }
}

fn summarize_explicit_memory(content: &str) -> String {
    const MAX_DESCRIPTION_CHARS: usize = 80;
    let summary: String = content.chars().take(MAX_DESCRIPTION_CHARS).collect();
    if content.chars().count() > MAX_DESCRIPTION_CHARS {
        format!("{summary}...")
    } else {
        summary
    }
}

fn contains_any(haystack: &str, needles: &[&str]) -> bool {
    let lower = haystack.to_ascii_lowercase();
    needles
        .iter()
        .any(|needle| haystack.contains(needle) || lower.contains(needle))
}

/// Extract memories (`MemoryEntry` objects) from conversation using the memory-extract LLM template.
pub async fn run_memory_extraction(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    history: &[Message],
    llm: &dyn LlmClient,
    max_tokens: usize,
    reconsolidation_context: &str,
) -> Vec<cortex_types::MemoryEntry> {
    let template = prompt_manager
        .and_then(|p| p.get_system_template("memory-extract"))
        .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_MEMORY_EXTRACT.to_string());
    let prompt = crate::memory::extract::build_extract_prompt_with_reconsolidation(
        &template,
        history,
        reconsolidation_context,
    );
    let llm_messages = vec![cortex_types::Message {
        role: cortex_types::Role::User,
        content: vec![cortex_types::ContentBlock::Text { text: prompt }],
        attachments: Vec::new(),
    }];
    let request = crate::llm::types::LlmRequest {
        system: None,
        messages: &llm_messages,
        tools: None,
        max_tokens,
        thinking: false,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };
    match llm.complete(request).await {
        Ok(resp) => {
            let text = resp.text.unwrap_or_default();
            let memories = parse_memory_extract_response(&text);
            tracing::info!(
                memories = memories.len(),
                response_chars = text.chars().count(),
                "post-turn memory extraction completed"
            );
            memories
        }
        Err(error) => {
            tracing::warn!(error = %error, "post-turn memory extraction failed");
            vec![]
        }
    }
}
