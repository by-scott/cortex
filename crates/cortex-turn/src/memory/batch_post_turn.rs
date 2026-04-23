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
    #[serde(default = "default_confidence")]
    pub confidence: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptUpdate {
    pub action: String,
    pub layer: String,
    pub content: String,
}

const fn default_confidence() -> f64 {
    1.0
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
             Use only relation types: works_on, created_by, depends_on, part_of, corrected_by, prefers, located_at, occurred_before, caused, uses, created, modified, reviewed, replaced_by.\n\
             Do not emit generic relations such as relates_to or associated_with.\n\
             Format: [{{\"source\": \"...\", \"target\": \"...\", \"relation\": \"...\", \"confidence\": 0.0}}]\n\n\
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
        .filter(|e| e.confidence >= 0.70)
        .filter_map(|e| {
            crate::memory::extract::normalize_relation_type(&e.relation).map(|relation| {
                MemoryRelation::new(&e.source, &e.target, relation)
                    .with_metadata(serde_json::json!({ "confidence": e.confidence }).to_string())
            })
        })
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
