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
        BatchEntityInput, BatchTasks, execute_batch, format_conversation, to_memory_relations,
        to_prompt_updates,
    };

    let should_update_prompts =
        should_evolve_prompts(prompt_manager, events_log, input, final_text, history);
    let should_extract = prompt_manager.is_some()
        && config.auto_extract
        && crate::memory::should_extract(config.turns_since_extract, config.extract_min_turns);
    let reconsolidation_context = format_reconsolidation_context(&config.reconsolidation_memories);
    let mut batch_tasks = BatchTasks::default();
    if should_extract {
        batch_tasks.entity_extraction = Some(BatchEntityInput {
            conversation: format_conversation(history),
        });
    }
    if should_update_prompts && let Some(pm) = prompt_manager {
        batch_tasks.prompt_update = Some(build_batch_prompt_input(
            pm, history, events_log, input, final_text,
        ));
    }

    if batch_tasks.count() >= 2 {
        let result = execute_batch(&batch_tasks, llm, config.max_tokens).await;
        let memories = if should_extract {
            run_memory_extraction(
                prompt_manager,
                history,
                llm,
                config.max_tokens,
                &reconsolidation_context,
            )
            .await
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
        let memories = run_memory_extraction(
            prompt_manager,
            history,
            llm,
            config.max_tokens,
            &reconsolidation_context,
        )
        .await;
        (vec![], rels, memories)
    } else {
        (vec![], vec![], vec![])
    }
}

fn build_batch_prompt_input(
    prompt_manager: &cortex_kernel::PromptManager,
    history: &[Message],
    events_log: &[Payload],
    input: &str,
    final_text: Option<&String>,
) -> crate::memory::batch_post_turn::BatchPromptInput {
    let mut current_prompts = String::new();
    for layer in cortex_types::PromptLayer::all() {
        if let Some(content) = prompt_manager.get(layer) {
            let _ = write!(current_prompts, "[{layer}]\n{content}\n\n");
        }
    }
    let bootstrap = !prompt_manager.is_initialized();
    crate::memory::batch_post_turn::BatchPromptInput {
        current_prompts,
        evidence_context: build_prompt_update_evidence_context(
            history, events_log, input, bootstrap,
        ),
        delivery_context: build_prompt_update_delivery_context(final_text),
        bootstrap,
    }
}

fn format_reconsolidation_context(memories: &[cortex_types::MemoryEntry]) -> String {
    if memories.is_empty() {
        return "None.".to_string();
    }
    memories
        .iter()
        .take(20)
        .enumerate()
        .map(|(idx, memory)| {
            format!(
                "{}. [{} {:?}/{:?} source={:?} strength={:.2}] {}\n{}",
                idx + 1,
                memory.id,
                memory.memory_type,
                memory.kind,
                memory.source,
                memory.strength,
                memory.description,
                memory.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
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
    let updates =
        analyze_prompt_updates(pm, llm, events_log, input, final_text, history, bootstrap).await;

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
    events_log: &[Payload],
    input: &str,
    response: Option<&String>,
    history: &[Message],
    bootstrap: bool,
) -> Vec<(cortex_types::PromptLayer, String)> {
    use cortex_types::PromptLayer;

    const PROMPTS_PLACEHOLDER: &str = "{current_prompts}";
    const EVIDENCE_PLACEHOLDER: &str = "{evidence_context}";
    const DELIVERY_PLACEHOLDER: &str = "{delivery_context}";
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

    let evidence_context =
        build_prompt_update_evidence_context(history, events_log, input, bootstrap);
    let delivery_context = build_prompt_update_delivery_context(response);

    let filled_template = template
        .replace(PROMPTS_PLACEHOLDER, &current_prompts)
        .replace(EVIDENCE_PLACEHOLDER, &evidence_context)
        .replace(DELIVERY_PLACEHOLDER, &delivery_context)
        .replace(
            CONVERSATION_PLACEHOLDER,
            &format!(
                "{evidence_context}\n\n## Delivery Draft (Do not copy directly)\n{delivery_context}"
            ),
        );
    let runtime_guidance = if bootstrap {
        "Runtime guidance:\n- Bootstrap may use the full evidence context: collaborator statements, assistant exploration, and tool results.\n- The delivery draft is not prompt content. Never copy it directly into any layer.\n- Only persist stable findings that remain valid after removing greetings, rapport, and user-facing scaffolding."
    } else {
        "Runtime guidance:\n- Use the evidence context as the source of truth for prompt evolution.\n- The delivery draft is user-facing prose and must not be copied directly into any layer."
    };
    let prompt = format!("{runtime_guidance}\n\n{filled_template}");

    // Call LLM for analysis -- use user message (not system-only) for provider compatibility
    let analysis_message = cortex_types::Message::user(prompt);
    let analysis_messages = [analysis_message];
    let req = LlmRequest {
        system: None,
        messages: &analysis_messages,
        tools: None,
        max_tokens: cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
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
        cortex_types::PromptLayer::Identity => {
            if bootstrap_identity_name(new_content).is_none() {
                return false;
            }
        }
        cortex_types::PromptLayer::Behavioral => {
            if contains_identity_claims(new_content) {
                return false;
            }
        }
        cortex_types::PromptLayer::User => {}
    }
    // No Jaccard check — bootstrap replaces template placeholders with real content.
    true
}

fn build_prompt_update_evidence_context(
    history: &[Message],
    events_log: &[Payload],
    input: &str,
    bootstrap: bool,
) -> String {
    let mut context = String::new();
    let recent_count = history.len().min(6);

    let stage = if bootstrap {
        "bootstrap initialization"
    } else {
        "incremental evolution"
    };
    let _ = write!(
        context,
        "## Evidence Scope\nThis is {stage}. Use the conversation and tool evidence below to infer durable findings.\nDo not treat the delivery draft as prompt content.\n\n"
    );
    let _ = writeln!(context, "## Conversation Evidence");
    for msg in history.iter().rev().take(recent_count).rev() {
        let role = match msg.role {
            Role::User => "Collaborator",
            Role::Assistant => "Assistant",
        };
        let _ = write!(context, "{role}: {}\n\n", msg.text_content());
    }
    let _ = write!(context, "Collaborator: {input}\n\n");

    let tool_evidence = summarize_tool_evidence(events_log);
    if !tool_evidence.is_empty() {
        let _ = write!(context, "## Tool Evidence\n{tool_evidence}\n");
    }

    context
}

fn build_prompt_update_delivery_context(response: Option<&String>) -> String {
    response.map_or_else(
        || "No final delivery draft was captured for this turn.".to_string(),
        |resp| format!("Assistant draft:\n{}", trim_for_prompt(resp, 4_000)),
    )
}

fn summarize_tool_evidence(events_log: &[Payload]) -> String {
    let mut lines = Vec::new();
    let mut pending_tool = None::<(&str, &str)>;

    for payload in events_log {
        match payload {
            Payload::ToolInvocationIntent { tool_name, input } => {
                pending_tool = Some((tool_name.as_str(), input.as_str()));
            }
            Payload::ToolInvocationResult {
                tool_name,
                output,
                is_error,
            } => {
                let status = if *is_error { "error" } else { "ok" };
                let input = pending_tool
                    .filter(|(pending_name, _)| *pending_name == tool_name)
                    .map_or("", |(_, tool_input)| tool_input);
                let line = if input.is_empty() {
                    format!(
                        "- `{tool_name}` [{status}] output: {}",
                        trim_for_prompt(output, 600)
                    )
                } else {
                    format!(
                        "- `{tool_name}` [{status}] input: {} | output: {}",
                        trim_for_prompt(input, 240),
                        trim_for_prompt(output, 600)
                    )
                };
                lines.push(line);
                pending_tool = None;
            }
            _ => {}
        }
    }

    lines.join("\n")
}

fn trim_for_prompt(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.to_string();
    }

    let trimmed: String = text.chars().take(max_chars).collect();
    format!("{trimmed}… [truncated {} chars]", char_count - max_chars)
}

#[must_use]
pub fn bootstrap_identity_name(content: &str) -> Option<&str> {
    content.lines().find_map(|line| {
        let trimmed = line.trim();
        trimmed
            .strip_prefix("**Name**:")
            .or_else(|| trimmed.strip_prefix("Name:"))
            .map(str::trim)
            .filter(|name| !name.is_empty())
    })
}

/// Count lines starting with `#` (markdown sections).
fn count_markdown_sections(text: &str) -> usize {
    text.lines().filter(|l| l.starts_with('#')).count()
}

/// Check for directive words that don't belong in `soul.md`.
///
/// Soul contains pure ontology (values, epistemology, autonomy).
/// Directives, tool references, and behavioral instructions are violations.
fn contains_directive_words(text: &str) -> bool {
    let lower = text.to_lowercase();
    [
        "you must ",
        "you should ",
        "always do ",
        "always use ",
        "never do ",
        "do not ",
        "when you ",
    ]
    .iter()
    .any(|d| lower.contains(d))
}

/// Check for identity claims that don't belong in `behavioral.md`.
///
/// Behavioral uses imperative protocol voice. First-person identity
/// statements and self-referential descriptions are violations.
fn contains_identity_claims(text: &str) -> bool {
    let lower = text.to_lowercase();
    lower.lines().any(|line| {
        let t = line.trim_start();
        t.starts_with("i am ")
            || t.starts_with("i believe ")
            || t.starts_with("my name is ")
            || t.starts_with("i exist as ")
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
