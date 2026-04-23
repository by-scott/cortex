use cortex_types::{MemoryRelation, Message};

const PH_CONVERSATION: &str = "{conversation}";
const PH_RECONSOLIDATION: &str = "{reconsolidation}";
const RELATION_CONFIDENCE_MIN: f64 = 0.70;
const ALLOWED_RELATION_TYPES: &[&str] = &[
    "works_on",
    "created_by",
    "depends_on",
    "part_of",
    "corrected_by",
    "prefers",
    "located_at",
    "occurred_before",
    "caused",
    "uses",
    "created",
    "modified",
    "reviewed",
    "replaced_by",
];

/// Build a prompt from a template by replacing `{conversation}` with formatted
/// messages. Works for both memory extraction and entity extraction.
#[must_use]
pub fn build_extract_prompt(template: &str, messages: &[Message]) -> String {
    build_extract_prompt_with_reconsolidation(template, messages, "")
}

/// Build a memory extraction prompt with active reconsolidation context.
#[must_use]
pub fn build_extract_prompt_with_reconsolidation(
    template: &str,
    messages: &[Message],
    reconsolidation_context: &str,
) -> String {
    let conversation = format_conversation(messages);
    template
        .replace(PH_CONVERSATION, &conversation)
        .replace(PH_RECONSOLIDATION, reconsolidation_context)
}

/// Build the entity extraction prompt from conversation history.
///
/// The template must contain a `{conversation}` placeholder.
#[must_use]
pub fn build_entity_extract_prompt(template: &str, messages: &[Message]) -> String {
    let conversation = format_conversation(messages);
    template.replace(PH_CONVERSATION, &conversation)
}

/// Format messages into a conversation string.
fn format_conversation(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| {
            let role = match m.role {
                cortex_types::Role::User => "User",
                cortex_types::Role::Assistant => "Assistant",
            };
            format!("{role}: {}", m.text_content())
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse LLM entity extraction response into a list of `MemoryRelation`.
///
/// Expected JSON format: `[{"source": "...", "target": "...", "relation": "..."}]`
/// Returns empty vec on parse failure.
#[must_use]
pub fn parse_entity_response(response: &str) -> Vec<MemoryRelation> {
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

    let parsed: Vec<serde_json::Value> = match serde_json::from_str(json_str) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    parsed
        .iter()
        .filter_map(|v| {
            let source = v.get("source")?.as_str()?;
            let target = v.get("target")?.as_str()?;
            let relation = normalize_relation_type(v.get("relation")?.as_str()?)?;
            let confidence = v
                .get("confidence")
                .and_then(serde_json::Value::as_f64)
                .unwrap_or(1.0);
            if source.is_empty() || target.is_empty() || relation.is_empty() {
                return None;
            }
            if confidence < RELATION_CONFIDENCE_MIN {
                return None;
            }
            let metadata = serde_json::json!({ "confidence": confidence }).to_string();
            Some(MemoryRelation::new(source, target, relation).with_metadata(metadata))
        })
        .collect()
}

/// Normalize and validate a graph relation type.
#[must_use]
pub fn normalize_relation_type(relation: &str) -> Option<&'static str> {
    let normalized = relation.trim().to_ascii_lowercase();
    ALLOWED_RELATION_TYPES
        .iter()
        .copied()
        .find(|allowed| *allowed == normalized)
}

/// Extract entities from a conversation using LLM.
///
/// Returns the list of extracted relations. Returns empty list on LLM failure.
pub async fn extract_entities(
    messages: &[Message],
    template: &str,
    llm: &dyn crate::llm::client::LlmClient,
    max_tokens: usize,
) -> Vec<MemoryRelation> {
    let prompt = build_entity_extract_prompt(template, messages);

    let llm_messages = vec![cortex_types::Message::user(prompt)];

    let request = crate::llm::types::LlmRequest {
        system: None,
        messages: &llm_messages,
        tools: None,
        max_tokens,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    match llm.complete(request).await {
        Ok(resp) => {
            let text = resp.text.unwrap_or_default();
            parse_entity_response(&text)
        }
        Err(_) => Vec::new(),
    }
}

/// Persist extracted relations to the memory graph.
///
/// Silently ignores duplicates (`INSERT OR REPLACE` in `MemoryGraph`).
/// Returns the number of relations successfully persisted.
#[must_use]
pub fn persist_relations(
    relations: &[MemoryRelation],
    graph: &cortex_kernel::MemoryGraph,
) -> usize {
    let mut persisted = 0;
    for rel in relations {
        if normalize_relation_type(&rel.relation_type).is_some() && graph.add_relation(rel).is_ok()
        {
            persisted += 1;
        }
    }
    persisted
}
