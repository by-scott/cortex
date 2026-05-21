use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::Ordering;

use cortex_kernel::MemoryGraph;
use cortex_types::{CorrelationId, Event, Message, Payload, PromptLayer, TurnId};

use super::DaemonState;
use crate::turn_executor::EndpointLlmResolver;

pub(super) struct PostTurnJob {
    pub session_id: String,
    pub actor: String,
    pub source: String,
    pub input: String,
    pub final_text: Option<String>,
    pub events: Vec<Payload>,
    pub history: Vec<Message>,
    pub turns_since_extract: usize,
    pub should_extract: bool,
}

pub(super) fn channel() -> (
    tokio::sync::mpsc::UnboundedSender<PostTurnJob>,
    tokio::sync::mpsc::UnboundedReceiver<PostTurnJob>,
) {
    tokio::sync::mpsc::unbounded_channel()
}

pub(super) fn spawn(
    state: Arc<DaemonState>,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let Some(mut rx) = state.take_post_turn_receiver() else {
            tracing::warn!("Post-turn queue receiver already taken");
            return;
        };
        tracing::info!("Post-turn queue started");

        loop {
            tokio::select! {
                Some(job) = rx.recv() => process_job(&state, job).await,
                changed = shutdown_rx.changed() => {
                    if changed.is_ok() {
                        tracing::debug!("Post-turn queue received shutdown signal");
                    }
                    break;
                }
                else => break,
            }
        }
    })
}

async fn process_job(state: &Arc<DaemonState>, job: PostTurnJob) {
    tracing::debug!(
        session_id = %job.session_id,
        source = %job.source,
        "Post-turn job started"
    );

    let cfg = state.config().clone();
    let turn_config = build_turn_config(state, &cfg, &job);
    let dmn_llm = state
        .resolve("memory_extract")
        .or_else(|| state.resolve("compress"))
        .unwrap_or_else(|| state.llm.as_ref());
    let (prompt_updates, entity_relations, mut memories) =
        cortex_turn::orchestrator::post_turn::run_post_turn_batch(
            Some(&state.prompt_manager),
            &job.events,
            &job.input,
            job.final_text.as_ref(),
            dmn_llm,
            &job.history,
            &turn_config,
        )
        .await;

    if !turn_saved_memory(&job.events) {
        memories.extend(
            cortex_turn::orchestrator::post_turn::extract_explicit_user_memories(&job.input),
        );
    }

    let updated_prompts = apply_prompt_updates(state, &prompt_updates);
    let relation_count = persist_entity_relations(state, &entity_relations);
    let memory_count = save_extracted_memories(state, &memories).await;
    deprecate_expired_memories(state, &cfg);

    tracing::info!(
        session_id = %job.session_id,
        prompts = updated_prompts,
        relations = relation_count,
        memories = memory_count,
        "Post-turn job completed"
    );
}

fn build_turn_config(
    state: &DaemonState,
    cfg: &cortex_types::config::CortexConfig,
    job: &PostTurnJob,
) -> cortex_turn::orchestrator::TurnConfig {
    let mut turn_config = cortex_turn::orchestrator::TurnConfig {
        max_tokens: state.max_output_tokens,
        auto_extract: cfg.memory.auto_extract && job.should_extract,
        extract_min_turns: cfg.memory.extract_min_turns,
        turns_since_extract: job.turns_since_extract,
        reconsolidation_memories: active_reconsolidation_memories(state, &job.actor),
        evolution_weights: cfg.evolution.signal_weights(),
        protected_runtime_roots: protected_runtime_roots(&state.data_dir),
        ..cortex_turn::orchestrator::TurnConfig::default()
    };
    turn_config.trace = cfg.turn.trace.clone();
    turn_config.metacognition = cfg.metacognition.clone();
    turn_config.risk = cfg.risk.clone();
    turn_config.session_id = Some(job.session_id.clone());
    turn_config.actor = Some(job.actor.clone());
    turn_config.source = Some(job.source.clone());
    turn_config.execution_scope = cortex_sdk::ExecutionScope::Background;
    turn_config
}

fn active_reconsolidation_memories(
    state: &DaemonState,
    actor: &str,
) -> Vec<cortex_types::MemoryEntry> {
    let now = chrono::Utc::now();
    state
        .memory_store
        .list_for_actor(actor)
        .unwrap_or_default()
        .into_iter()
        .filter(|memory| {
            memory
                .reconsolidation_until
                .is_some_and(|until| until > now)
        })
        .collect()
}

fn turn_saved_memory(events: &[Payload]) -> bool {
    events.iter().any(|event| match event {
        Payload::ToolInvocationResult {
            tool_name,
            is_error,
            ..
        } => !*is_error && tool_name == "memory_save",
        _ => false,
    })
}

fn apply_prompt_updates(
    state: &DaemonState,
    updates: &[(cortex_types::PromptLayer, String)],
) -> usize {
    let was_bootstrap = !state.prompt_manager.is_initialized();
    let mut updated_count = 0usize;
    let mut identity_updated = false;

    for (layer, content) in updates {
        if state.prompt_manager.update(*layer, content).is_err() {
            continue;
        }
        updated_count += 1;
        if *layer == PromptLayer::Identity
            && cortex_turn::orchestrator::post_turn::bootstrap_identity_name(content).is_some()
        {
            identity_updated = true;
        }
        let event = Event::new(
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
        let _ = state.journal.append(&event);
    }

    if was_bootstrap && identity_updated {
        let _ = state.prompt_manager.mark_initialized();
        tracing::info!("Bootstrap: initialized {updated_count} prompt layers via post-turn queue");
    }
    updated_count
}

fn persist_entity_relations(
    state: &DaemonState,
    relations: &[cortex_types::MemoryRelation],
) -> usize {
    if relations.is_empty() {
        return 0;
    }
    if let Ok(graph) = MemoryGraph::open(&memory_graph_path(&state.data_dir)) {
        let _ = cortex_turn::memory::extract::persist_relations(relations, &graph);
    }
    relations.len()
}

async fn save_extracted_memories(
    state: &DaemonState,
    memories: &[cortex_types::MemoryEntry],
) -> usize {
    let mut saved = 0usize;
    for memory in memories {
        if state.memory_store.save(memory).is_err() {
            continue;
        }
        saved += 1;
        embed_memory(state, memory).await;
        let event = Event::new(
            TurnId::new(),
            CorrelationId::new(),
            Payload::MemoryCaptured {
                memory_id: memory.id.clone(),
                memory_type: format!("{:?}", memory.memory_type),
            },
        );
        let _ = state.journal.append(&event);
        state.metrics.record_memory_capture();
    }

    if saved > 0 {
        let count = u32::try_from(saved).unwrap_or(u32::MAX);
        state
            .heartbeat_state
            .pending_consolidation
            .fetch_add(count, Ordering::Relaxed);
        state
            .heartbeat_state
            .pending_embeddings
            .fetch_add(count, Ordering::Relaxed);
    }
    saved
}

async fn embed_memory(state: &DaemonState, memory: &cortex_types::MemoryEntry) {
    let (Some(client), Some(cache)) = (&state.embedding_client, &state.embedding_store) else {
        return;
    };
    let text = format!("{} {}", memory.description, memory.content);
    let hash = cortex_kernel::embedding_store::content_hash(&text);
    if cache.get(&hash).is_some() {
        return;
    }
    let Ok(embedding) = client.embed(&text).await else {
        return;
    };
    let _ = cache.put(&hash, "default", &embedding);
    let _ = cache.ensure_vector_table(embedding.len());
    let _ = cache.upsert_vector(&memory.id, &embedding);
}

fn deprecate_expired_memories(state: &DaemonState, cfg: &cortex_types::config::CortexConfig) {
    let _ = cortex_turn::memory::deprecate_expired(&state.memory_store, cfg.memory.decay_rate);
}

fn memory_graph_path(data_dir: &Path) -> PathBuf {
    let instance_home = data_dir.parent().unwrap_or(data_dir);
    cortex_kernel::CortexPaths::from_instance_home(instance_home).memory_graph_path()
}

fn protected_runtime_roots(data_dir: &Path) -> Vec<PathBuf> {
    let Some(instance_home) = data_dir.parent() else {
        return vec![data_dir.to_path_buf()];
    };
    [
        "data", "prompts", "sessions", "memory", "skills", "channels",
    ]
    .into_iter()
    .map(|name| instance_home.join(name))
    .collect()
}
