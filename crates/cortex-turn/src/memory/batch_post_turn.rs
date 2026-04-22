use cortex_types::{MemoryRelation, Message, PromptLayer};
use serde::{Deserialize, Serialize};

/// Which post-turn tasks to include in the batch.
#[derive(Debug, Default)]
pub struct BatchTasks {
    pub entity_extraction: Option<BatchEntityInput>,
    pub prompt_update: Option<BatchPromptInput>,
}

#[derive(Debug)]
pub struct BatchEntityInput {
    pub conversation: String,
}

#[derive(Debug)]
pub struct BatchPromptInput {
    pub current_prompts: String,
    pub evidence_context: String,
    pub delivery_context: String,
    pub bootstrap: bool,
}

/// Combined result from a batch post-turn LLM call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BatchPostTurnResult {
    #[serde(default)]
    pub entities: Vec<EntityRelation>,
    #[serde(default)]
    pub prompt_updates: Vec<PromptUpdate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityRelation {
    pub source: String,
    pub target: String,
    pub relation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptUpdate {
    pub action: String,
    pub layer: String,
    pub content: String,
}

impl BatchTasks {
    /// Count how many tasks are active.
    #[must_use]
    pub const fn count(&self) -> usize {
        let mut n = 0;
        if self.entity_extraction.is_some() {
            n += 1;
        }
        if self.prompt_update.is_some() {
            n += 1;
        }
        n
    }
}

/// Build a combined batch prompt from multiple tasks.
#[must_use]
pub fn build_batch_prompt(tasks: &BatchTasks) -> String {
    let mut sections = Vec::new();
    let mut task_num = 0;

    if let Some(ref entity) = tasks.entity_extraction {
        task_num += 1;
        sections.push(format!(
            "## Task {task_num}: Entity Extraction\n\
             Extract entity relationships from the following conversation.\n\
             Return as JSON array under key \"entities\".\n\
             Format: [{{\"source\": \"...\", \"target\": \"...\", \"relation\": \"...\"}}]\n\n\
             Conversation:\n{}\n",
            entity.conversation
        ));
    }

    if let Some(ref prompt) = tasks.prompt_update {
        task_num += 1;
        let mode = if prompt.bootstrap {
            "Bootstrap initialization"
        } else {
            "Incremental self-update"
        };
        sections.push(format!(
            "## Task {task_num}: Prompt Self-Update Analysis\n\
             Analyze whether any instance prompts should be updated based on this interaction.\n\
             Mode: {mode}.\n\
             Return as JSON array under key \"prompt_updates\".\n\
             Format: [{{\"action\": \"UPDATE\", \"layer\": \"soul|identity|user|agent\", \"content\": \"new content\"}}]\n\
             Return empty array if no updates needed.\n\n\
             Rules:\n\
             - Use the evidence context to infer durable findings.\n\
             - The delivery draft is user-facing prose, not prompt content. Never copy or lightly rewrite it into prompt files.\n\
             - If a finding only appears in delivery phrasing and is unsupported by evidence, ignore it.\n\n\
             Current prompts:\n{}\n\n\
             Evidence context:\n{}\n\n\
             Delivery draft (do not copy directly):\n{}\n",
            prompt.current_prompts, prompt.evidence_context, prompt.delivery_context
        ));
    }

    {
        const TASK_NUM_PLACEHOLDER: &str = "{task_num}";
        let header = cortex_kernel::prompt_manager::DEFAULT_BATCH_ANALYSIS
            .replace(TASK_NUM_PLACEHOLDER, &task_num.to_string());
        format!("{header}\n{}", sections.join("\n"))
    }
}

/// Parse the combined batch response.
#[must_use]
pub fn parse_batch_response(response: &str) -> BatchPostTurnResult {
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

    serde_json::from_str(json_str).unwrap_or_default()
}

/// Convert batch entity relations to `MemoryRelation`.
#[must_use]
pub fn to_memory_relations(entities: &[EntityRelation]) -> Vec<MemoryRelation> {
    entities
        .iter()
        .filter(|e| !e.source.is_empty() && !e.target.is_empty() && !e.relation.is_empty())
        .map(|e| MemoryRelation::new(&e.source, &e.target, &e.relation))
        .collect()
}

/// Convert batch prompt updates to the format used by the orchestrator.
#[must_use]
pub fn to_prompt_updates(updates: &[PromptUpdate]) -> Vec<(PromptLayer, String)> {
    updates
        .iter()
        .filter(|u| u.action == "UPDATE" && !u.content.is_empty())
        .filter_map(|u| {
            let layer = match u.layer.as_str() {
                "soul" => PromptLayer::Soul,
                "identity" => PromptLayer::Identity,
                "user" => PromptLayer::User,
                "agent" | "behavioral" => PromptLayer::Behavioral,
                _ => return None,
            };
            Some((layer, u.content.clone()))
        })
        .collect()
}

/// Format messages into a conversation string for batch input.
#[must_use]
pub fn format_conversation(messages: &[Message]) -> String {
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

/// Execute the batch post-turn LLM call.
pub async fn execute_batch(
    tasks: &BatchTasks,
    llm: &dyn crate::llm::client::LlmClient,
    max_tokens: usize,
) -> BatchPostTurnResult {
    let prompt = build_batch_prompt(tasks);
    let messages = vec![cortex_types::Message::user(prompt)];

    let request = crate::llm::types::LlmRequest {
        system: None,
        messages: &messages,
        tools: None,
        max_tokens,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    match llm.complete(request).await {
        Ok(resp) => {
            let text = resp.text.unwrap_or_default();
            parse_batch_response(&text)
        }
        Err(_) => BatchPostTurnResult::default(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_post_turn_result_json_roundtrip() {
        let result = BatchPostTurnResult {
            entities: vec![EntityRelation {
                source: "user".into(),
                target: "project".into(),
                relation: "works_on".into(),
            }],
            prompt_updates: vec![PromptUpdate {
                action: "UPDATE".into(),
                layer: "user".into(),
                content: "new content".into(),
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        let back: BatchPostTurnResult = serde_json::from_str(&json).unwrap();
        assert_eq!(back.entities.len(), 1);
        assert_eq!(back.prompt_updates.len(), 1);
    }

    #[test]
    fn build_batch_prompt_two_tasks() {
        let tasks = BatchTasks {
            entity_extraction: Some(BatchEntityInput {
                conversation: "User: hello\nAssistant: hi".into(),
            }),
            prompt_update: Some(BatchPromptInput {
                current_prompts: "soul: be helpful".into(),
                evidence_context: "User asked about Rust".into(),
                delivery_context: "Assistant answered about ownership.".into(),
                bootstrap: false,
            }),
        };
        let prompt = build_batch_prompt(&tasks);
        assert!(prompt.contains("Task 1: Entity Extraction"));
        assert!(prompt.contains("Task 2: Prompt Self-Update"));
        assert!(prompt.contains("2 independent analysis tasks"));
    }

    #[test]
    fn build_batch_prompt_single_task() {
        let tasks = BatchTasks {
            entity_extraction: Some(BatchEntityInput {
                conversation: "test".into(),
            }),
            prompt_update: None,
        };
        let prompt = build_batch_prompt(&tasks);
        assert!(prompt.contains("1 independent analysis tasks"));
        assert!(prompt.contains("Task 1: Entity Extraction"));
        assert!(!prompt.contains("Task 2"));
    }

    #[test]
    fn parse_batch_response_valid() {
        let json =
            r#"{"entities":[{"source":"A","target":"B","relation":"knows"}],"prompt_updates":[]}"#;
        let result = parse_batch_response(json);
        assert_eq!(result.entities.len(), 1);
        assert!(result.prompt_updates.is_empty());
    }

    #[test]
    fn parse_batch_response_with_fences() {
        let json = "```json\n{\"entities\":[],\"prompt_updates\":[]}\n```";
        let result = parse_batch_response(json);
        assert!(result.entities.is_empty());
    }

    #[test]
    fn parse_batch_response_invalid() {
        let result = parse_batch_response("not json");
        assert!(result.entities.is_empty());
        assert!(result.prompt_updates.is_empty());
    }

    #[test]
    fn to_memory_relations_filters_empty() {
        let entities = vec![
            EntityRelation {
                source: "A".into(),
                target: "B".into(),
                relation: "knows".into(),
            },
            EntityRelation {
                source: String::new(),
                target: "B".into(),
                relation: String::new(),
            },
        ];
        let rels = to_memory_relations(&entities);
        assert_eq!(rels.len(), 1);
    }

    #[test]
    fn to_prompt_updates_filters_non_update() {
        let updates = vec![
            PromptUpdate {
                action: "UPDATE".into(),
                layer: "user".into(),
                content: "new".into(),
            },
            PromptUpdate {
                action: "DELETE".into(),
                layer: "soul".into(),
                content: "x".into(),
            },
            PromptUpdate {
                action: "UPDATE".into(),
                layer: "invalid".into(),
                content: "x".into(),
            },
        ];
        let result = to_prompt_updates(&updates);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].0, PromptLayer::User);
    }

    #[test]
    fn task_count() {
        let empty = BatchTasks::default();
        assert_eq!(empty.count(), 0);

        let one = BatchTasks {
            entity_extraction: Some(BatchEntityInput {
                conversation: "x".into(),
            }),
            prompt_update: None,
        };
        assert_eq!(one.count(), 1);

        let two = BatchTasks {
            entity_extraction: Some(BatchEntityInput {
                conversation: "x".into(),
            }),
            prompt_update: Some(BatchPromptInput {
                current_prompts: "x".into(),
                evidence_context: "x".into(),
                delivery_context: "y".into(),
                bootstrap: false,
            }),
        };
        assert_eq!(two.count(), 2);
    }
}
