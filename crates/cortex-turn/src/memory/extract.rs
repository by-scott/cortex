use cortex_types::{MemoryRelation, Message};

const PH_CONVERSATION: &str = "{conversation}";

/// Build a prompt from a template by replacing `{conversation}` with formatted
/// messages. Works for both memory extraction and entity extraction.
#[must_use]
pub fn build_extract_prompt(template: &str, messages: &[Message]) -> String {
    let conversation = format_conversation(messages);
    template.replace(PH_CONVERSATION, &conversation)
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
            let relation = v.get("relation")?.as_str()?;
            if source.is_empty() || target.is_empty() || relation.is_empty() {
                return None;
            }
            Some(MemoryRelation::new(source, target, relation))
        })
        .collect()
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
        if graph.add_relation(rel).is_ok() {
            persisted += 1;
        }
    }
    persisted
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::{ContentBlock, Role};

    fn msg(role: Role, text: &str) -> Message {
        Message {
            role,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            attachments: Vec::new(),
        }
    }

    #[test]
    fn build_extract_prompt_fills_placeholder() {
        let template = "Extract from:\n{conversation}\nDone.";
        let messages = vec![msg(Role::User, "hello"), msg(Role::Assistant, "hi there")];
        let result = build_extract_prompt(template, &messages);
        assert!(result.contains("User: hello"));
        assert!(result.contains("Assistant: hi there"));
        assert!(!result.contains("{conversation}"));
    }

    #[test]
    fn build_extract_prompt_empty_messages() {
        let template = "Extract: {conversation}";
        let result = build_extract_prompt(template, &[]);
        assert_eq!(result, "Extract: ");
    }

    #[test]
    fn build_entity_extract_prompt_fills_placeholder() {
        let template = "Entities:\n{conversation}\nDone.";
        let messages = vec![
            msg(Role::User, "I use Rust for Cortex"),
            msg(Role::Assistant, "Noted"),
        ];
        let result = build_entity_extract_prompt(template, &messages);
        assert!(result.contains("User: I use Rust for Cortex"));
        assert!(result.contains("Assistant: Noted"));
        assert!(!result.contains("{conversation}"));
    }

    #[test]
    fn parse_entity_response_valid() {
        let json = r#"[{"source": "user", "target": "Rust", "relation": "uses"}, {"source": "Cortex", "target": "Rust", "relation": "built_with"}]"#;
        let result = parse_entity_response(json);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].source_id, "user");
        assert_eq!(result[0].target_id, "Rust");
        assert_eq!(result[0].relation_type, "uses");
        assert_eq!(result[1].source_id, "Cortex");
    }

    #[test]
    fn parse_entity_response_with_fences() {
        let json = "```json\n[{\"source\": \"a\", \"target\": \"b\", \"relation\": \"c\"}]\n```";
        let result = parse_entity_response(json);
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn parse_entity_response_invalid() {
        assert!(parse_entity_response("not json").is_empty());
        assert!(parse_entity_response("{}").is_empty());
    }

    #[test]
    fn parse_entity_response_empty_array() {
        let result = parse_entity_response("[]");
        assert!(result.is_empty());
    }

    #[test]
    fn parse_entity_response_skips_incomplete() {
        let json = r#"[{"source": "a", "target": "", "relation": "c"}, {"source": "x", "target": "y", "relation": "z"}]"#;
        let result = parse_entity_response(json);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].source_id, "x");
    }

    #[test]
    fn persist_relations_to_graph() {
        let g = cortex_kernel::MemoryGraph::in_memory().unwrap();
        let relations = vec![
            MemoryRelation::new("user", "Rust", "uses"),
            MemoryRelation::new("Cortex", "Rust", "built_with"),
        ];
        let count = persist_relations(&relations, &g);
        assert_eq!(count, 2);

        let neighbors = g.neighbors("user").unwrap();
        assert!(neighbors.contains(&"Rust".to_string()));
    }

    #[test]
    fn persist_relations_duplicates_ignored() {
        let g = cortex_kernel::MemoryGraph::in_memory().unwrap();
        let relations = vec![MemoryRelation::new("a", "b", "rel")];
        let _ = persist_relations(&relations, &g);
        let count = persist_relations(&relations, &g); // duplicate
        assert_eq!(count, 1); // INSERT OR REPLACE succeeds
    }
}
