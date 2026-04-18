use std::fmt::Write as _;

use cortex_types::{Message, Payload, Role};

use crate::llm::{LlmClient, LlmRequest};

use super::TurnConfig;

// ── Evolution signal ────────────────────────────────────────

/// Signal-driven evolution trigger replacing hardcoded thresholds.
///
/// Six weighted signals determine whether prompt self-update should run:
/// - `correction_detected` (1.0): system response contains self-correction markers
/// - `explicit_preference` (0.8): user input contains preference expressions
/// - `new_domain` (0.6): user mentions domains absent from user profile
/// - `first_session_turn` (0.5): first turn in this session's history
/// - `tool_intensive` (0.4): 3+ tool calls this turn
/// - `long_input` (0.3): input > 500 chars
///
/// Threshold: 0.5 (any single high-weight signal suffices).
#[derive(Clone, Copy)]
pub struct EvolutionSignal {
    /// Bitfield: bit 0 = `correction_detected`, 1 = `explicit_preference`,
    /// 2 = `new_domain_detected`, 3 = `first_session_turn`, 4 = `tool_intensive`,
    /// 5 = `long_input`.
    flags: u8,
}

impl EvolutionSignal {
    const CORRECTION_DETECTED: u8 = 1 << 0;
    const EXPLICIT_PREFERENCE: u8 = 1 << 1;
    const NEW_DOMAIN_DETECTED: u8 = 1 << 2;
    const FIRST_SESSION_TURN: u8 = 1 << 3;
    const TOOL_INTENSIVE: u8 = 1 << 4;
    const LONG_INPUT: u8 = 1 << 5;

    const fn new() -> Self {
        Self { flags: 0 }
    }

    const fn set_if(&mut self, flag: u8, condition: bool) {
        if condition {
            self.flags |= flag;
        }
    }

    const fn has(self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn score(self) -> f64 {
        const WEIGHTS: [(u8, f64); 6] = [
            (EvolutionSignal::CORRECTION_DETECTED, 1.0),
            (EvolutionSignal::EXPLICIT_PREFERENCE, 0.8),
            (EvolutionSignal::NEW_DOMAIN_DETECTED, 0.6),
            (EvolutionSignal::FIRST_SESSION_TURN, 0.5),
            (EvolutionSignal::TOOL_INTENSIVE, 0.4),
            (EvolutionSignal::LONG_INPUT, 0.3),
        ];
        WEIGHTS
            .iter()
            .filter(|(flag, _)| self.has(*flag))
            .map(|(_, weight)| weight)
            .sum()
    }

    /// Compute score using provided weights (ordered same as signal constants).
    fn score_with_weights(self, weights: &[f64; 6]) -> f64 {
        const FLAGS: [u8; 6] = [
            EvolutionSignal::CORRECTION_DETECTED,
            EvolutionSignal::EXPLICIT_PREFERENCE,
            EvolutionSignal::NEW_DOMAIN_DETECTED,
            EvolutionSignal::FIRST_SESSION_TURN,
            EvolutionSignal::TOOL_INTENSIVE,
            EvolutionSignal::LONG_INPUT,
        ];
        FLAGS
            .iter()
            .zip(weights.iter())
            .filter(|(flag, _)| self.has(**flag))
            .map(|(_, weight)| weight)
            .sum()
    }

    fn should_trigger(self) -> bool {
        self.score() >= 0.5
    }

    fn should_trigger_with_weights(self, weights: &[f64; 6]) -> bool {
        self.score_with_weights(weights) >= 0.5
    }
}

// ── Should evolve prompts ───────────────────────────────────

/// Check whether the evolution signal warrants prompt self-update.
#[must_use]
pub fn should_evolve_prompts(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    events_log: &[Payload],
    input: &str,
    final_text: Option<&String>,
    history: &[Message],
) -> bool {
    prompt_manager.is_some_and(|pm| {
        // Bootstrap (uninitialized) always triggers evolution — prompts must be populated.
        if !pm.is_initialized() {
            return true;
        }
        let tool_call_count = events_log
            .iter()
            .filter(|e| matches!(e, Payload::ToolInvocationResult { .. }))
            .count();
        let response_text = final_text.map_or("", String::as_str);
        let user_profile = pm.get(cortex_types::PromptLayer::User).unwrap_or_default();
        let mut signal = EvolutionSignal::new();
        signal.set_if(
            EvolutionSignal::CORRECTION_DETECTED,
            crate::memory::user_signal::detect_correction(response_text),
        );
        signal.set_if(
            EvolutionSignal::EXPLICIT_PREFERENCE,
            crate::memory::user_signal::detect_preference(input),
        );
        signal.set_if(
            EvolutionSignal::NEW_DOMAIN_DETECTED,
            crate::memory::user_signal::detect_new_domain(input, &user_profile),
        );
        signal.set_if(EvolutionSignal::FIRST_SESSION_TURN, history.is_empty());
        signal.set_if(EvolutionSignal::TOOL_INTENSIVE, tool_call_count >= 3);
        signal.set_if(EvolutionSignal::LONG_INPUT, input.len() > 500);
        signal.should_trigger()
    })
}

// ── Post-turn batch ─────────────────────────────────────────

/// Post-turn batch: entity extraction, memory extraction, and prompt self-update.
pub async fn run_post_turn_batch(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    events_log: &[Payload],
    input: &str,
    final_text: Option<&String>,
    llm: &dyn LlmClient,
    history: &[Message],
    config: &TurnConfig,
) -> (
    Vec<(cortex_types::PromptLayer, String)>,
    Vec<cortex_types::MemoryRelation>,
    Vec<cortex_types::MemoryEntry>,
) {
    use crate::memory::batch_post_turn::{
        BatchEntityInput, BatchPromptInput, BatchTasks, execute_batch, format_conversation,
        to_memory_relations, to_prompt_updates,
    };

    let should_update_prompts =
        should_evolve_prompts(prompt_manager, events_log, input, final_text, history);
    let should_extract = prompt_manager.is_some()
        && config.auto_extract
        && crate::memory::should_extract(config.turns_since_extract, config.extract_min_turns);
    let mut batch_tasks = BatchTasks::default();
    if should_extract {
        batch_tasks.entity_extraction = Some(BatchEntityInput {
            conversation: format_conversation(history),
        });
    }
    if should_update_prompts && let Some(pm) = prompt_manager {
        let mut current_prompts = String::new();
        for layer in cortex_types::PromptLayer::all() {
            if let Some(content) = pm.get(layer) {
                let _ = write!(current_prompts, "[{layer}]\n{content}\n\n");
            }
        }
        let recent = history
            .iter()
            .rev()
            .take(6)
            .map(cortex_types::Message::text_content)
            .collect::<Vec<_>>()
            .join("\n");
        batch_tasks.prompt_update = Some(BatchPromptInput {
            current_prompts,
            recent_context: recent,
        });
    }

    if batch_tasks.count() >= 2 {
        let result = execute_batch(&batch_tasks, llm, config.max_tokens).await;
        let memories = if should_extract {
            run_memory_extraction(prompt_manager, history, llm, config.max_tokens).await
        } else {
            vec![]
        };
        // Apply quality validation to batch prompt updates (parity with non-batch path).
        // During bootstrap, skip Jaccard similarity check.
        let raw_updates = to_prompt_updates(&result.prompt_updates);
        let bootstrap = prompt_manager.is_some_and(|pm| !pm.is_initialized());
        let validated_updates = if let Some(pm) = prompt_manager {
            raw_updates
                .into_iter()
                .filter(|(layer, new_content)| {
                    let old_content = pm.get(*layer).unwrap_or_default();
                    if bootstrap {
                        validate_prompt_update_bootstrap(*layer, &old_content, new_content)
                    } else {
                        validate_prompt_update(*layer, &old_content, new_content)
                    }
                })
                .collect()
        } else {
            raw_updates
        };
        (
            validated_updates,
            to_memory_relations(&result.entities),
            memories,
        )
    } else if should_update_prompts {
        let updates = maybe_prompt_self_update(
            prompt_manager,
            events_log,
            input,
            final_text,
            llm,
            history,
            &config.evolution_weights,
        )
        .await;
        (updates, vec![], vec![])
    } else if should_extract {
        let template = prompt_manager
            .and_then(|pm| pm.get_system_template("entity-extract"))
            .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_ENTITY_EXTRACT.to_string());
        let rels =
            crate::memory::extract::extract_entities(history, &template, llm, config.max_tokens)
                .await;
        let memories = run_memory_extraction(prompt_manager, history, llm, config.max_tokens).await;
        (vec![], rels, memories)
    } else {
        (vec![], vec![], vec![])
    }
}

// ── Prompt self-update ──────────────────────────────────────

pub async fn maybe_prompt_self_update(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    events_log: &[Payload],
    input: &str,
    final_text: Option<&String>,
    llm: &dyn LlmClient,
    history: &[Message],
    evolution_weights: &[f64; 6],
) -> Vec<(cortex_types::PromptLayer, String)> {
    let Some(pm) = prompt_manager else {
        return vec![];
    };

    let tool_call_count = events_log
        .iter()
        .filter(|e| matches!(e, Payload::ToolInvocationResult { .. }))
        .count();

    let response_text = final_text.map_or("", String::as_str);
    let user_profile = pm.get(cortex_types::PromptLayer::User).unwrap_or_default();

    let mut signal = EvolutionSignal::new();
    signal.set_if(
        EvolutionSignal::CORRECTION_DETECTED,
        crate::memory::user_signal::detect_correction(response_text),
    );
    signal.set_if(
        EvolutionSignal::EXPLICIT_PREFERENCE,
        crate::memory::user_signal::detect_preference(input),
    );
    signal.set_if(
        EvolutionSignal::NEW_DOMAIN_DETECTED,
        crate::memory::user_signal::detect_new_domain(input, &user_profile),
    );
    signal.set_if(EvolutionSignal::FIRST_SESSION_TURN, history.is_empty());
    signal.set_if(EvolutionSignal::TOOL_INTENSIVE, tool_call_count >= 3);
    signal.set_if(EvolutionSignal::LONG_INPUT, input.len() > 500);

    if !signal.should_trigger_with_weights(evolution_weights) {
        return vec![];
    }

    let bootstrap = !pm.is_initialized();
    let updates = analyze_prompt_updates(pm, llm, input, final_text, history, bootstrap).await;

    // Quality validation: filter out updates that fail quality rules.
    // During bootstrap, skip Jaccard similarity (template → real content diverges widely).
    updates
        .into_iter()
        .filter(|(layer, new_content)| {
            let old_content = pm.get(*layer).unwrap_or_default();
            if bootstrap {
                validate_prompt_update_bootstrap(*layer, &old_content, new_content)
            } else {
                validate_prompt_update(*layer, &old_content, new_content)
            }
        })
        .collect()
}

/// Analyze whether any instance prompts should be updated based on this turn's interaction.
///
/// When `bootstrap` is true, uses the `bootstrap-init` template (designed for first-time
/// initialization from template placeholders). Otherwise uses the `self-update` template
/// for incremental evolution.
pub async fn analyze_prompt_updates(
    pm: &cortex_kernel::PromptManager,
    llm: &dyn LlmClient,
    input: &str,
    response: Option<&String>,
    history: &[Message],
    bootstrap: bool,
) -> Vec<(cortex_types::PromptLayer, String)> {
    use cortex_types::PromptLayer;

    const PROMPTS_PLACEHOLDER: &str = "{current_prompts}";
    const CONVERSATION_PLACEHOLDER: &str = "{conversation}";

    // Bootstrap uses the dedicated bootstrap-init template; normal uses self-update.
    let template = if bootstrap {
        pm.get_system_template("bootstrap-init")
            .or_else(|| pm.get_system_template("self-update"))
    } else {
        pm.get_system_template("self-update")
    };
    let Some(template) = template else {
        return vec![];
    };

    // Build current prompts context
    let mut current_prompts = String::new();
    for layer in PromptLayer::all() {
        if let Some(content) = pm.get(layer) {
            let _ = write!(current_prompts, "--- {layer} ---\n{content}\n\n");
        }
    }

    // Build conversation summary (last few messages + current input/response)
    let mut conversation = String::new();
    let recent_count = history.len().min(6);
    for msg in history.iter().rev().take(recent_count).rev() {
        let role = match msg.role {
            Role::User => "User",
            Role::Assistant => "Assistant",
        };
        let _ = write!(conversation, "{}: {}\n\n", role, msg.text_content());
    }
    let _ = write!(conversation, "User: {input}\n\n");
    if let Some(resp) = response {
        let _ = write!(conversation, "Assistant: {resp}\n\n");
    }

    // Fill template
    let prompt = template
        .replace(PROMPTS_PLACEHOLDER, &current_prompts)
        .replace(CONVERSATION_PLACEHOLDER, &conversation);

    // Call LLM for analysis -- use user message (not system-only) for provider compatibility
    let analysis_message = cortex_types::Message::user(prompt);
    let analysis_messages = [analysis_message];
    let req = LlmRequest {
        system: None,
        messages: &analysis_messages,
        tools: None,
        max_tokens: cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK,
        on_text: None,
    };

    let Ok(resp) = llm.complete(req).await else {
        return vec![];
    };

    let Some(text) = resp.text else {
        return vec![];
    };

    // Parse JSON response
    let updates: Vec<serde_json::Value> = if let Ok(v) = serde_json::from_str(&text) {
        v
    } else {
        // Try to extract JSON from markdown code block
        let trimmed = text
            .trim()
            .trim_start_matches("```json")
            .trim_start_matches("```")
            .trim_end_matches("```")
            .trim();
        match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => return vec![],
        }
    };

    let mut result = vec![];
    for update in &updates {
        let action = update.get("action").and_then(|a| a.as_str()).unwrap_or("");
        if action != "UPDATE" {
            continue;
        }
        let layer_str = update.get("layer").and_then(|l| l.as_str()).unwrap_or("");
        let content = update.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if content.is_empty() {
            continue;
        }
        let layer = match layer_str {
            "soul" => PromptLayer::Soul,
            "identity" => PromptLayer::Identity,
            "user" => PromptLayer::User,
            "agent" | "behavioral" => PromptLayer::Behavioral,
            _ => continue,
        };
        result.push((layer, content.to_string()));
    }
    result
}

// ── Prompt validation ───────────────────────────────────────

/// Validate a proposed prompt update before writing to disk.
///
/// Three quality rules:
/// 1. Section preservation: new content must not have fewer markdown sections.
/// 2. Layer boundary: `soul` should not contain directive words; `behavioral` should
///    not contain identity claims.
/// 3. Incremental change: `Jaccard` word similarity must be >= 0.3.
#[must_use]
pub fn validate_prompt_update(
    layer: cortex_types::PromptLayer,
    old_content: &str,
    new_content: &str,
) -> bool {
    // Rule 1: don't lose sections
    let old_sections = count_markdown_sections(old_content);
    let new_sections = count_markdown_sections(new_content);
    if new_sections < old_sections {
        return false;
    }

    // Rule 2: layer boundary compliance
    match layer {
        cortex_types::PromptLayer::Soul => {
            if contains_directive_words(new_content) {
                return false;
            }
        }
        cortex_types::PromptLayer::Behavioral => {
            if contains_identity_claims(new_content) {
                return false;
            }
        }
        cortex_types::PromptLayer::Identity | cortex_types::PromptLayer::User => {}
    }

    // Rule 3: incremental change (not a complete rewrite)
    if jaccard_word_similarity(old_content, new_content) < 0.3 {
        return false;
    }

    true
}

/// Bootstrap-mode validation: section preservation + layer boundary, but NO Jaccard check.
///
/// During bootstrap, prompts go from templates to real content — a complete rewrite is expected.
#[must_use]
pub fn validate_prompt_update_bootstrap(
    layer: cortex_types::PromptLayer,
    old_content: &str,
    new_content: &str,
) -> bool {
    // Rule 1: don't lose sections
    let old_sections = count_markdown_sections(old_content);
    let new_sections = count_markdown_sections(new_content);
    if new_sections < old_sections {
        return false;
    }
    // Rule 2: layer boundary compliance
    match layer {
        cortex_types::PromptLayer::Soul => {
            if contains_directive_words(new_content) {
                return false;
            }
        }
        cortex_types::PromptLayer::Behavioral => {
            if contains_identity_claims(new_content) {
                return false;
            }
        }
        cortex_types::PromptLayer::Identity | cortex_types::PromptLayer::User => {}
    }
    // No Jaccard check — bootstrap replaces template placeholders with real content.
    true
}

/// Count lines starting with `#` (markdown sections).
fn count_markdown_sections(text: &str) -> usize {
    text.lines().filter(|l| l.starts_with('#')).count()
}

/// Check for directive words that don't belong in `soul.md`.
fn contains_directive_words(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Only flag strong directives -- "must"/"should" in imperative context
    // Avoid false positives: check for "you must"/"you should" patterns
    [
        "you must ",
        "you should ",
        "always do ",
        "never do ",
        "do not ",
    ]
    .iter()
    .any(|d| lower.contains(d))
}

/// Check for identity claims that don't belong in `behavioral.md`.
fn contains_identity_claims(text: &str) -> bool {
    let lower = text.to_lowercase();
    // Only flag strong identity statements at line starts
    lower.lines().any(|line| {
        line.trim_start().starts_with("i am ") || line.trim_start().starts_with("i believe ")
    })
}

/// `Jaccard` similarity on word sets (intersection / union).
fn jaccard_word_similarity(a: &str, b: &str) -> f64 {
    let left_words: std::collections::HashSet<&str> = a
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .collect();
    let right_words: std::collections::HashSet<&str> = b
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 3)
        .collect();
    let intersection: u32 = left_words
        .intersection(&right_words)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    let union: u32 = left_words
        .union(&right_words)
        .count()
        .try_into()
        .unwrap_or(u32::MAX);
    if union == 0 {
        return 1.0;
    }
    f64::from(intersection) / f64::from(union)
}

// ── Memory extraction ───────────────────────────────────────

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
        Ok(serde_json::Value::Object(_)) => {
            // Single object: wrap in array
            vec![serde_json::from_str(json_str).unwrap_or_default()]
        }
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
            let kind = cortex_types::MemoryKind::Episodic;
            Some(cortex_types::MemoryEntry::new(
                content.to_string(),
                desc.to_string(),
                memory_type,
                kind,
            ))
        })
        .collect()
}

/// Extract memories (`MemoryEntry` objects) from conversation using the memory-extract LLM template.
pub async fn run_memory_extraction(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    history: &[Message],
    llm: &dyn LlmClient,
    max_tokens: usize,
) -> Vec<cortex_types::MemoryEntry> {
    let template = prompt_manager
        .and_then(|p| p.get_system_template("memory-extract"))
        .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_MEMORY_EXTRACT.to_string());
    let prompt = crate::memory::extract::build_extract_prompt(&template, history);
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
        on_text: None,
    };
    match llm.complete(request).await {
        Ok(resp) => parse_memory_extract_response(&resp.text.unwrap_or_default()),
        Err(_) => vec![],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Prompt validation tests ─────────────────────────────────

    #[test]
    fn validate_prompt_update_rejects_section_loss() {
        let old = "# Title\n\n## Section A\nContent A\n\n## Section B\nContent B\n";
        let new_fewer = "# Title\nAll content merged\n";
        assert!(!validate_prompt_update(
            cortex_types::PromptLayer::User,
            old,
            new_fewer
        ));
    }

    #[test]
    fn validate_prompt_update_accepts_section_preserved() {
        let old = "# User\n\n## Role\nDeveloper\n\n## Expertise\nRust\n";
        let new_same = "# User\n\n## Role\nSenior developer\n\n## Expertise\nRust, Python\n";
        assert!(validate_prompt_update(
            cortex_types::PromptLayer::User,
            old,
            new_same
        ));
    }

    #[test]
    fn validate_prompt_update_rejects_soul_with_directives() {
        let old = "# Soul\n\nI believe in depth over breadth.\n";
        let new_directive = "# Soul\n\nI believe in depth over breadth.\nYou must always verify.\n";
        assert!(!validate_prompt_update(
            cortex_types::PromptLayer::Soul,
            old,
            new_directive
        ));
    }

    #[test]
    fn validate_prompt_update_rejects_behavioral_with_identity() {
        let old = "# Behavioral\n\nPerceive before acting.\n";
        let new_identity = "# Behavioral\n\nI am a helpful assistant.\nPerceive before acting.\n";
        assert!(!validate_prompt_update(
            cortex_types::PromptLayer::Behavioral,
            old,
            new_identity
        ));
    }

    #[test]
    fn validate_prompt_update_rejects_excessive_rewrite() {
        let old = "# Soul\n\nI believe depth defeats breadth. Focus is how understanding forms.\n";
        let new_totally_different =
            "# Soul\n\nCompletely unrelated content about a different topic entirely.\n";
        assert!(!validate_prompt_update(
            cortex_types::PromptLayer::Soul,
            old,
            new_totally_different
        ));
    }

    #[test]
    fn validate_prompt_update_accepts_incremental_change() {
        let old = "# User\n\n## Role\nDeveloper working on backend systems.\n\n## Expertise\nRust, Python, databases.\n";
        let new_incremental = "# User\n\n## Role\nSenior developer working on backend systems.\n\n## Expertise\nRust, Python, databases, Kubernetes.\n";
        assert!(validate_prompt_update(
            cortex_types::PromptLayer::User,
            old,
            new_incremental
        ));
    }

    // ── Evolution signal tests ──────────────────────────────────

    #[test]
    fn evolution_signal_correction_triggers() {
        let mut signal = EvolutionSignal::new();
        signal.set_if(EvolutionSignal::CORRECTION_DETECTED, true);
        assert!(signal.should_trigger(), "correction (1.0) >= 0.5 threshold");
    }

    #[test]
    fn evolution_signal_long_input_alone_insufficient() {
        let mut signal = EvolutionSignal::new();
        signal.set_if(EvolutionSignal::LONG_INPUT, true);
        assert!(!signal.should_trigger(), "long_input (0.3) < 0.5 threshold");
    }

    #[test]
    fn evolution_signal_combined_reaches_threshold() {
        let mut signal = EvolutionSignal::new();
        signal.set_if(EvolutionSignal::TOOL_INTENSIVE, true); // 0.4
        signal.set_if(EvolutionSignal::LONG_INPUT, true); // 0.3
        assert!(
            signal.should_trigger(),
            "tool_intensive(0.4) + long_input(0.3) = 0.7 >= 0.5"
        );
    }

    #[test]
    fn evolution_signal_empty_does_not_trigger() {
        let signal = EvolutionSignal::new();
        assert!(!signal.should_trigger(), "no signals = score 0.0");
    }

    #[test]
    fn evolution_signal_all_signals_score() {
        let mut signal = EvolutionSignal::new();
        signal.set_if(EvolutionSignal::CORRECTION_DETECTED, true);
        signal.set_if(EvolutionSignal::EXPLICIT_PREFERENCE, true);
        signal.set_if(EvolutionSignal::NEW_DOMAIN_DETECTED, true);
        signal.set_if(EvolutionSignal::FIRST_SESSION_TURN, true);
        signal.set_if(EvolutionSignal::TOOL_INTENSIVE, true);
        signal.set_if(EvolutionSignal::LONG_INPUT, true);
        let expected = 1.0 + 0.8 + 0.6 + 0.5 + 0.4 + 0.3;
        assert!((signal.score() - expected).abs() < f64::EPSILON);
    }

    // ── Memory extraction tests ─────────────────────────────────

    #[test]
    fn parse_memory_extract_valid_json() {
        let json = r#"[{"type":"User","description":"prefers concise replies","content":"User said they want short answers"}]"#;
        let memories = parse_memory_extract_response(json);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].description, "prefers concise replies");
        assert_eq!(memories[0].content, "User said they want short answers");
        assert_eq!(memories[0].memory_type, cortex_types::MemoryType::User);
        assert_eq!(memories[0].status, cortex_types::MemoryStatus::Captured);
    }

    #[test]
    fn parse_memory_extract_with_fences() {
        let json = "```json\n[{\"type\":\"Feedback\",\"description\":\"no mocks\",\"content\":\"Use real DB in tests\"}]\n```";
        let memories = parse_memory_extract_response(json);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].memory_type, cortex_types::MemoryType::Feedback);
    }

    #[test]
    fn parse_memory_extract_invalid() {
        assert!(parse_memory_extract_response("not json").is_empty());
        assert!(parse_memory_extract_response("{}").is_empty());
    }

    #[test]
    fn parse_memory_extract_skips_empty_fields() {
        let json = r#"[{"type":"User","description":"","content":"something"},{"type":"Project","description":"valid","content":"data"}]"#;
        let memories = parse_memory_extract_response(json);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].description, "valid");
    }

    #[test]
    fn parse_memory_extract_defaults_to_project() {
        let json = r#"[{"description":"note","content":"data"}]"#;
        let memories = parse_memory_extract_response(json);
        assert_eq!(memories.len(), 1);
        assert_eq!(memories[0].memory_type, cortex_types::MemoryType::Project);
    }

    #[test]
    fn extract_config_respected() {
        // auto_extract=false should prevent extraction
        let config = TurnConfig {
            auto_extract: false,
            extract_min_turns: 5,
            turns_since_extract: 10,
            ..TurnConfig::default()
        };
        assert!(!config.auto_extract);

        // auto_extract=true but turns < threshold
        let config2 = TurnConfig {
            auto_extract: true,
            extract_min_turns: 5,
            turns_since_extract: 3,
            ..TurnConfig::default()
        };
        assert!(!crate::memory::should_extract(
            config2.turns_since_extract,
            config2.extract_min_turns
        ));

        // auto_extract=true and turns >= threshold
        let config3 = TurnConfig {
            auto_extract: true,
            extract_min_turns: 5,
            turns_since_extract: 5,
            ..TurnConfig::default()
        };
        assert!(crate::memory::should_extract(
            config3.turns_since_extract,
            config3.extract_min_turns
        ));
    }
}
