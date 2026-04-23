use cortex_kernel::{
    EmbeddingClient, EmbeddingStore, Journal, MemoryGraph, MemoryStore, PromptManager,
};
use cortex_turn::context::{ContextBuilder, SituationalContext};
use cortex_turn::llm::LlmClient;
use cortex_turn::memory::{
    EmbeddingHealthStatus, EmbeddingRecaller, build_memory_context, mark_reconsolidation,
    rank_memories,
};
use cortex_turn::meta::{MetaAlert, MetaMonitor};
use cortex_turn::orchestrator::{TurnConfig, TurnContext, TurnStreamEvent, run_turn};
use cortex_turn::risk::PermissionGate;
use cortex_turn::tools::ToolRegistry;
use cortex_types::config::CortexConfig;
use cortex_types::{
    CorrelationId, Event, Message, Payload, PromptLayer, ResponsePart, ResumePacket, TextFormat,
    TurnId,
};

/// Resolves the appropriate LLM client for a named sub-endpoint.
pub trait EndpointLlmResolver: Send + Sync {
    /// Get the LLM for the given endpoint (e.g., `memory_extract`, `compress`).
    /// Returns `None` to fall back to the primary LLM.
    fn resolve(&self, endpoint_name: &str) -> Option<&dyn LlmClient>;
}

use std::path::{Path, PathBuf};

/// Output produced by a single Turn execution including post-processing results.
pub struct TurnOutput {
    /// The assistant's text response (if any).
    pub response_text: Option<String>,
    /// Structured response parts for transports that can render text and media.
    pub response_parts: Vec<ResponsePart>,
    /// Meta-cognitive alerts generated during this Turn.
    pub alerts: Vec<MetaAlert>,
    /// Number of entity relations persisted to the memory graph.
    pub entity_relations_count: usize,
    /// Number of memories extracted during this Turn (0 if none).
    pub extracted_memory_count: usize,
    /// Aggregate input tokens across all LLM calls in this Turn.
    pub total_input_tokens: usize,
    /// Aggregate output tokens across all LLM calls in this Turn.
    pub total_output_tokens: usize,
    /// Number of tool calls that completed successfully.
    pub tool_call_count: usize,
    /// Number of tool calls that errored.
    pub tool_error_count: usize,
}

/// Configuration for constructing a [`TurnExecutor`].
///
/// Groups the many subsystem references into a single struct to avoid
/// functions with excessive parameter counts.
pub struct TurnExecutorConfig<'a> {
    pub config: &'a CortexConfig,
    pub journal: &'a Journal,
    pub memory_store: &'a MemoryStore,
    pub llm: &'a dyn LlmClient,
    pub tools: &'a ToolRegistry,
    pub prompt_manager: &'a PromptManager,
    pub embedding_client: Option<&'a EmbeddingClient>,
    pub embedding_store: Option<&'a EmbeddingStore>,
    pub embedding_health: Option<&'a EmbeddingHealthStatus>,
    /// Skill summaries to inject into system prompt (pre-rendered).
    pub skill_summaries: Option<String>,
    /// Skill registry for Fork execution support.
    pub skill_registry: Option<&'a cortex_turn::skills::SkillRegistry>,
    pub data_dir: &'a Path,
    pub max_output_tokens: usize,
    pub resume: &'a ResumePacket,
    /// How many turns since last memory extraction (tracked by caller).
    pub turns_since_extract: usize,
    /// Endpoint-to-LLM resolver: returns the appropriate LLM client for a named
    /// sub-endpoint (e.g. `memory_extract` → light group). Falls back to primary `llm`.
    pub endpoint_llm: Option<&'a dyn EndpointLlmResolver>,
    /// Turn execution tracer for external observability (stderr / SSE).
    pub tracer: &'a dyn cortex_turn::orchestrator::TurnTracer,
    /// Vision-capable LLM to use when images are present.  `None` means the
    /// primary `llm` handles images directly (native multimodal model).
    pub vision_llm: Option<&'a dyn LlmClient>,
    /// Shared turn runtime control plane.
    pub control: Option<cortex_turn::orchestrator::TurnControl>,
    /// Called after TPN completes and before post-turn work starts.
    pub on_tpn_complete: Option<&'a (dyn Fn() + Send + Sync)>,
    /// Active session id for this turn execution.
    pub session_id: &'a str,
    /// Canonical actor identity for this turn execution.
    pub actor: &'a str,
    /// Transport or invocation source for this turn execution.
    pub source: &'a str,
    /// Foreground or background execution scope.
    pub execution_scope: cortex_sdk::ExecutionScope,
}

/// Unified Turn execution with complete post-turn processing.
///
/// Encapsulates the full Turn lifecycle:
/// 1. Build system prompt (4-layer + bootstrap + resume + memory context)
/// 2. Execute Turn via `run_turn()`
/// 3. Post-turn: persist prompt updates, write entity relations, collect
///    meta alerts, apply memory decay
pub struct TurnExecutor<'a> {
    cfg: TurnExecutorConfig<'a>,
}

/// User input for a single Turn: prompt text plus optional media attachments.
pub struct TurnInput<'a> {
    pub text: &'a str,
    pub attachments: &'a [cortex_types::Attachment],
    /// Pre-encoded images as `(mime_type, base64_data)` pairs from the web API.
    pub inline_images: &'a [(String, String)],
}

/// Callbacks for Turn streaming events.
pub struct TurnCallbacks<'a> {
    pub on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
}

impl<'a> TurnExecutor<'a> {
    /// Create a new executor from a config bundle.
    #[must_use]
    pub const fn new(cfg: TurnExecutorConfig<'a>) -> Self {
        Self { cfg }
    }

    fn memory_graph_path(&self) -> PathBuf {
        let instance_home = self.cfg.data_dir.parent().unwrap_or(self.cfg.data_dir);
        cortex_kernel::CortexPaths::from_instance_home(instance_home).memory_graph_path()
    }

    /// Execute a Turn and perform all post-turn processing.
    ///
    /// # Errors
    ///
    /// Returns an error string if the Turn fails.
    ///
    /// # Panics
    ///
    /// Panics if the tokio runtime cannot be created (infallible in practice).
    pub fn execute(
        &self,
        input: &TurnInput<'_>,
        history: &mut Vec<Message>,
        gate: &dyn PermissionGate,
        meta: &mut MetaMonitor,
        summary_cache: &mut cortex_turn::context::SummaryCache,
        callbacks: &TurnCallbacks<'_>,
    ) -> Result<TurnOutput, String> {
        let system_prompt = self.build_system_prompt(input.text);
        let turn_config = self.build_turn_config(system_prompt);

        meta.start_turn();

        let result =
            self.run_turn_blocking(input, history, gate, &turn_config, callbacks, summary_cache);

        match result {
            Ok(turn_result) => {
                self.apply_prompt_updates(&turn_result);
                let entity_relations_count = self.persist_entity_relations(&turn_result);
                let extracted_memory_count = turn_result.extracted_memories.len();
                self.save_extracted_memories(&turn_result.extracted_memories);

                let alerts = meta.check();
                let event_count = u32::try_from(turn_result.events.len()).unwrap_or(u32::MAX);
                meta.end_turn(f64::from(event_count) / 50.0);

                let _deprecated = cortex_turn::memory::deprecate_expired(
                    self.cfg.memory_store,
                    self.cfg.config.memory.decay_rate,
                );

                let response_text = turn_result
                    .response_text
                    .filter(|text| !text.trim().is_empty())
                    .or_else(|| {
                        history
                            .iter()
                            .rev()
                            .find(|m| m.role == cortex_types::Role::Assistant)
                            .map(cortex_types::Message::text_content)
                            .filter(|t| !t.trim().is_empty())
                    });

                // Aggregate token and tool metrics from Turn events.
                let (mut total_in, mut total_out, mut tool_ok, mut tool_err) =
                    (0usize, 0usize, 0usize, 0usize);
                for ev in &turn_result.events {
                    match ev {
                        Payload::LlmCallCompleted {
                            input_tokens,
                            output_tokens,
                            ..
                        } => {
                            total_in += input_tokens;
                            total_out += output_tokens;
                        }
                        Payload::ToolInvocationResult { is_error, .. } => {
                            if *is_error {
                                tool_err += 1;
                            } else {
                                tool_ok += 1;
                            }
                        }
                        _ => {}
                    }
                }

                let response_parts =
                    build_response_parts(response_text.as_deref(), &turn_result.response_media);

                Ok(TurnOutput {
                    response_text,
                    response_parts,
                    alerts,
                    entity_relations_count,
                    extracted_memory_count,
                    total_input_tokens: total_in,
                    total_output_tokens: total_out,
                    tool_call_count: tool_ok,
                    tool_error_count: tool_err,
                })
            }
            Err(e) => {
                meta.end_turn(0.0);
                Err(e.to_string())
            }
        }
    }

    /// Build the `TurnConfig` from executor configuration.
    fn build_turn_config(&self, system_prompt: Option<String>) -> TurnConfig {
        TurnConfig {
            system_prompt,
            max_tokens: self.cfg.max_output_tokens,
            agent_depth: 0,
            working_memory_capacity: 5,
            max_tool_iterations: self.cfg.config.turn.max_tool_iterations,
            auto_extract: self.cfg.config.memory.auto_extract,
            extract_min_turns: self.cfg.config.memory.extract_min_turns,
            reconsolidation_memories: self.active_reconsolidation_memories(),
            turns_since_extract: self.cfg.turns_since_extract,
            tool_timeout_secs: self.cfg.config.turn.tool_timeout_secs,
            llm_transient_retries: self.cfg.config.turn.llm_transient_retries,
            strip_think_tags: self.cfg.config.turn.strip_think_tags,
            evolution_weights: self.cfg.config.evolution.signal_weights(),
            pressure_thresholds: {
                let v = &self.cfg.config.context.pressure_thresholds;
                if v.len() == 4 {
                    [v[0], v[1], v[2], v[3]]
                } else {
                    [0.60, 0.75, 0.85, 0.95]
                }
            },
            metacognition: self.cfg.config.metacognition.clone(),
            risk: self.cfg.config.risk.clone(),
            trace: self.cfg.config.turn.trace.clone(),
            session_id: Some(self.cfg.session_id.to_string()),
            actor: Some(self.cfg.actor.to_string()),
            source: Some(self.cfg.source.to_string()),
            execution_scope: self.cfg.execution_scope,
        }
    }

    fn active_reconsolidation_memories(&self) -> Vec<cortex_types::MemoryEntry> {
        let now = chrono::Utc::now();
        self.cfg
            .memory_store
            .list_for_actor(self.cfg.actor)
            .unwrap_or_default()
            .into_iter()
            .filter(|memory| {
                memory
                    .reconsolidation_until
                    .is_some_and(|until| until > now)
            })
            .collect()
    }

    /// Execute the turn in a blocking context.
    fn run_turn_blocking(
        &self,
        input: &TurnInput<'_>,
        history: &mut Vec<Message>,
        gate: &dyn PermissionGate,
        turn_config: &TurnConfig,
        callbacks: &TurnCallbacks<'_>,
        summary_cache: &mut cortex_turn::context::SummaryCache,
    ) -> Result<cortex_turn::orchestrator::TurnResult, cortex_turn::orchestrator::TurnError> {
        let compress_template = self
            .cfg
            .prompt_manager
            .get_system_template("context-compress");

        // Convert image attachments to (media_type, base64) pairs for
        // the TurnContext; non-image attachments are mentioned in the
        // prompt text instead (handled by the caller).
        let mut images = attachments_to_images(input.attachments);
        // Append pre-encoded inline images from the web API.
        images.extend_from_slice(input.inline_images);

        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                run_turn(TurnContext {
                    input: input.text,
                    history,
                    llm: self.cfg.llm,
                    vision_llm: self.cfg.vision_llm,
                    tools: self.cfg.tools,
                    journal: self.cfg.journal,
                    gate,
                    config: turn_config,
                    on_event: callbacks.on_event,
                    images,
                    compress_template,
                    summary_cache: Some(summary_cache),
                    prompt_manager: Some(self.cfg.prompt_manager),
                    skill_registry: self.cfg.skill_registry,
                    post_turn_llm: self.cfg.endpoint_llm.and_then(|r| {
                        r.resolve("memory_extract")
                            .or_else(|| r.resolve("compress"))
                    }),
                    tracer: self.cfg.tracer,
                    control: self.cfg.control.clone(),
                    on_tpn_complete: self.cfg.on_tpn_complete,
                })
                .await
            })
        })
    }

    /// Apply prompt layer updates from the turn result.
    ///
    /// During bootstrap (uninitialized), the instance stays in bootstrap mode
    /// until the **identity** layer is updated — meaning the instance has
    /// received a name, the minimum requirement for identity formation.
    /// Until then, every turn continues using the bootstrap template and
    /// bootstrap-init evolution (no Jaccard check).
    fn apply_prompt_updates(&self, turn_result: &cortex_turn::orchestrator::TurnResult) {
        let pm = self.cfg.prompt_manager;
        let was_bootstrap = !pm.is_initialized();
        let mut updated_count = 0usize;
        let mut identity_updated = false;
        for (layer, content) in &turn_result.prompt_updates {
            if pm.update(*layer, content).is_ok() {
                updated_count += 1;
                if *layer == PromptLayer::Identity
                    && cortex_turn::orchestrator::post_turn::bootstrap_identity_name(content)
                        .is_some()
                {
                    identity_updated = true;
                }
                let ev = Event::new(
                    TurnId::new(),
                    CorrelationId::new(),
                    Payload::PromptUpdated {
                        layer: if was_bootstrap {
                            format!("bootstrap:{layer}")
                        } else {
                            layer.to_string()
                        },
                    },
                );
                let _ = self.cfg.journal.append(&ev);
            }
        }
        // Only graduate from bootstrap when identity is updated (instance got a name).
        if was_bootstrap && identity_updated {
            let _ = pm.mark_initialized();
            tracing::info!("Bootstrap: initialized {updated_count} prompt layers via evolution");
        }
    }

    /// Persist entity relations to the memory graph.
    fn persist_entity_relations(
        &self,
        turn_result: &cortex_turn::orchestrator::TurnResult,
    ) -> usize {
        if turn_result.entity_relations.is_empty() {
            return 0;
        }
        if let Ok(graph) = MemoryGraph::open(&self.memory_graph_path()) {
            let _ = cortex_turn::memory::extract::persist_relations(
                &turn_result.entity_relations,
                &graph,
            );
        }
        turn_result.entity_relations.len()
    }

    /// Save extracted memories to the store and eagerly generate embeddings.
    fn save_extracted_memories(&self, memories: &[cortex_types::MemoryEntry]) {
        for mem in memories {
            if self.cfg.memory_store.save(mem).is_ok() {
                if let (Some(ec), Some(cache)) =
                    (self.cfg.embedding_client, self.cfg.embedding_store)
                {
                    let text = format!("{} {}", mem.description, mem.content);
                    let hash = cortex_kernel::embedding_store::content_hash(&text);
                    if cache.get(&hash).is_none() {
                        let embed_result = tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(ec.embed(&text))
                        });
                        if let Ok(emb) = embed_result {
                            let _ = cache.put(&hash, "default", &emb);
                            let _ = cache.ensure_vector_table(emb.len());
                            let _ = cache.upsert_vector(&mem.id, &emb);
                        }
                    }
                }
                let ev = Event::new(
                    TurnId::new(),
                    CorrelationId::new(),
                    Payload::MemoryCaptured {
                        memory_id: mem.id.clone(),
                        memory_type: format!("{:?}", mem.memory_type),
                    },
                );
                let _ = self.cfg.journal.append(&ev);
            }
        }
    }

    /// Build the system prompt by assembling all context layers.
    fn build_system_prompt(&self, input: &str) -> Option<String> {
        let pm = self.cfg.prompt_manager;
        let mut builder = ContextBuilder::new();

        if let Some(s) = pm.get(PromptLayer::Soul) {
            builder.set_soul(s);
        }
        if let Some(s) = pm.get(PromptLayer::Identity) {
            builder.set_identity(s);
        }
        if let Some(s) = pm.get(PromptLayer::User) {
            builder.set_user(s);
        }
        if let Some(s) = pm.get(PromptLayer::Behavioral) {
            builder.set_behavioral(s);
        }

        // R6 Situational: Bootstrap (first interaction) or Active (normal operation)
        // These are mutually exclusive by construction — SituationalContext is an enum.
        if !pm.is_initialized() {
            let bootstrap_content = pm
                .get_system_template("bootstrap")
                .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_BOOTSTRAP.to_string());
            builder.set_situational(SituationalContext::Bootstrap(bootstrap_content));
        } else if !self.cfg.resume.is_empty() {
            builder.set_situational(SituationalContext::Active {
                phase: String::new(),
                goals: self.cfg.resume.goals.join("; "),
                resume: self.cfg.resume.format_prompt(),
            });
        }

        // Skill summaries injection
        if let Some(ref summaries) = self.cfg.skill_summaries {
            builder.set_skills(summaries.clone());
        }

        // Memory context with embedding recall when available
        let all_memories = self
            .cfg
            .memory_store
            .list_for_actor(self.cfg.actor)
            .unwrap_or_default();
        let relevant = match (self.cfg.embedding_client, self.cfg.embedding_store) {
            (Some(ec), Some(cache)) => {
                let recaller = self.cfg.embedding_health.map_or_else(
                    || EmbeddingRecaller::new(ec, cache),
                    |health| EmbeddingRecaller::with_health(ec, cache, health),
                );
                let graph_scores = MemoryGraph::open(&self.memory_graph_path()).ok().map(|g| {
                    let seeds: Vec<String> =
                        cortex_turn::memory::rank_memories(input, &all_memories, 10)
                            .iter()
                            .map(|m| m.id.clone())
                            .collect();
                    cortex_turn::memory::graph_reasoning_scores(&seeds, &g, 2)
                });
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(recaller.recall(
                        input,
                        &all_memories,
                        self.cfg.config.memory.max_recall,
                        graph_scores,
                    ))
                })
            }
            _ => rank_memories(input, &all_memories, self.cfg.config.memory.max_recall),
        };
        mark_reconsolidation(&relevant, self.cfg.memory_store, 30);
        let memory_ctx = build_memory_context(&relevant);
        if !memory_ctx.is_empty() {
            builder.set_memory(memory_ctx);
        }

        builder.build()
    }
}

fn build_response_parts(
    response_text: Option<&str>,
    media: &[cortex_types::Attachment],
) -> Vec<ResponsePart> {
    let mut parts = Vec::new();
    if let Some(text) = response_text
        && !text.trim().is_empty()
    {
        parts.push(ResponsePart::Text {
            text: text.to_string(),
            format: TextFormat::Markdown,
        });
    }
    parts.extend(
        media
            .iter()
            .cloned()
            .map(|attachment| ResponsePart::Media { attachment }),
    );
    parts
}

/// Convert image attachments to `(mime_type, base64_data)` pairs that the
/// LLM orchestrator can include as `ContentBlock::Image` entries.
///
/// Non-image attachments and files exceeding 10 MB are silently skipped.
fn attachments_to_images(attachments: &[cortex_types::Attachment]) -> Vec<(String, String)> {
    use base64::Engine;

    const MAX_IMAGE_BYTES: u64 = 10 * 1024 * 1024;

    let mut images = Vec::new();
    for att in attachments {
        if att.media_type != "image" {
            continue;
        }
        // Skip overly large images based on declared size
        if let Some(sz) = att.size
            && sz > MAX_IMAGE_BYTES
        {
            continue;
        }
        let Ok(data) = std::fs::read(&att.url) else {
            tracing::warn!("Failed to read attachment file: {}", att.url);
            continue;
        };
        if u64::try_from(data.len()).unwrap_or(u64::MAX) > MAX_IMAGE_BYTES {
            continue;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&data);
        images.push((att.mime_type.clone(), b64));
    }
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turn_output_default_values() {
        let output = TurnOutput {
            response_text: Some("hello".into()),
            response_parts: vec![ResponsePart::Text {
                text: "hello".into(),
                format: TextFormat::Markdown,
            }],
            alerts: vec![],
            entity_relations_count: 0,
            extracted_memory_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
            tool_call_count: 0,
            tool_error_count: 0,
        };
        assert_eq!(output.response_text.as_deref(), Some("hello"));
        assert!(output.alerts.is_empty());
        assert_eq!(output.entity_relations_count, 0);
        assert_eq!(output.extracted_memory_count, 0);
    }
}
