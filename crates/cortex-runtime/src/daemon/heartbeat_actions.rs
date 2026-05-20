use super::DaemonState;

pub(super) fn execute(
    action: &crate::heartbeat::HeartbeatAction,
    state: &DaemonState,
    hb: &crate::heartbeat::HeartbeatState,
    stability: &mut crate::stability::StabilityMonitor,
) {
    match action {
        crate::heartbeat::HeartbeatAction::DeprecateExpired => {
            deprecate_expired(state);
        }
        crate::heartbeat::HeartbeatAction::EmbedPending => {
            embed_pending(state, hb);
        }
        crate::heartbeat::HeartbeatAction::ConsolidateMemories => {
            consolidate(state, hb);
        }
        crate::heartbeat::HeartbeatAction::EvolveSkills => {
            evolve_skills(state, hb);
        }
        crate::heartbeat::HeartbeatAction::Checkpoint => {
            checkpoint(state, stability);
        }
        crate::heartbeat::HeartbeatAction::SelfUpdate => {
            autonomous_turn(
                state,
                hb,
                "self-update",
                "Analyze recent interactions and determine if any prompts \
                 (soul/identity/behavioral/user) should be updated based on \
                 accumulated corrections and feedback.",
                |hb_inner| {
                    hb_inner
                        .correction_count
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                },
            );
        }
        crate::heartbeat::HeartbeatAction::DeepReflection => {
            autonomous_turn(
                state,
                hb,
                "reflection",
                "Reflect on recent work. What patterns have emerged? \
                 What could be improved? Are there any unresolved issues \
                 or insights worth remembering?",
                crate::heartbeat::HeartbeatState::touch,
            );
        }
        crate::heartbeat::HeartbeatAction::CronDue(prompt) => {
            autonomous_turn(state, hb, "cron", prompt, |_| {});
        }
    }
}

fn deprecate_expired(state: &DaemonState) {
    let n = cortex_turn::memory::deprecate_expired(state.memory_store(), 0.05).unwrap_or(0);
    if n > 0 {
        tracing::debug!(deprecated = n, "Heartbeat: deprecate");
    }
}

fn embed_pending(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;

    hb.pending_embeddings.store(0, Relaxed);
    let (Some(client), Some(cache)) = (
        state.embedding_client.as_ref(),
        state.embedding_store.as_ref(),
    ) else {
        return;
    };
    let memories = state.memory_store().list_all().unwrap_or_default();
    let mut embedded = 0usize;
    let mut vec_table_ready = false;
    for m in &memories {
        let hash = cortex_kernel::embedding_store::content_hash(&m.content);
        if cache.get(&hash).is_none()
            && let Ok(vec) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(client.embed(&m.content))
            })
            && !vec.is_empty()
        {
            // Lazily create the vec0 table on first embedding.
            if !vec_table_ready {
                let _ = cache.ensure_vector_table(vec.len());
                vec_table_ready = true;
            }
            let _ = cache.put(&hash, client.model_name(), &vec);
            let _ = cache.upsert_vector(&m.id, &vec);
            embedded += 1;
        } else if !vec_table_ready {
            // If the embedding was already cached, ensure the vec0 table
            // exists using its dimension, then backfill the vector index.
            if let Some(cached) = cache.get(&hash)
                && !cached.is_empty()
            {
                let _ = cache.ensure_vector_table(cached.len());
                vec_table_ready = true;
                let _ = cache.upsert_vector(&m.id, &cached);
            }
        } else {
            // Vec table is ready; backfill from cache if needed.
            if let Some(cached) = cache.get(&hash) {
                let _ = cache.upsert_vector(&m.id, &cached);
            }
        }
    }
    if embedded > 0 {
        tracing::info!(count = embedded, "Heartbeat: embedded pending memories");
    }
}

fn consolidate(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;

    let store = state.memory_store();
    let mut mem = store.list_all().unwrap_or_default();
    let r = cortex_turn::memory::consolidate::consolidate_memories(&mut mem);
    cortex_turn::memory::consolidate::upgrade_episodic_to_semantic(
        &mut mem,
        &[],
        state.config().memory.semantic_upgrade_similarity_threshold,
    );
    cortex_turn::memory::consolidate::apply_decay(&mut mem, 0.05, chrono::Utc::now());
    for m in &mem {
        let _ = store.save(m);
    }
    hb.pending_consolidation.store(0, Relaxed);
    if r.upgraded > 0 {
        tracing::debug!(upgraded = r.upgraded, "Heartbeat: consolidate");
    }
}

fn evolve_skills(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;

    if let Some(evo) = state.skill_registry().evolve() {
        for name in &evo.created {
            tracing::info!(skill = %name, "Heartbeat: new skill");
        }
        for (name, score) in &evo.flagged_weak {
            tracing::warn!(skill = %name, score = *score, "Heartbeat: weak skill flagged");
        }
        for proposal in &evo.proposals {
            tracing::info!(
                proposal = %proposal.id,
                candidate = %proposal.candidate_skill,
                target = ?proposal.target_skill,
                relation = %proposal.relation.as_str(),
                "Heartbeat: skill evolution proposal"
            );
        }
    }
    for (name, score) in state.skill_registry().utility_snapshot() {
        let _ = state.journal().save_skill_utility(&name, score);
    }
    for health in state.skill_registry().health_snapshot() {
        let _ = state.journal().save_skill_health(&health);
    }
    for proposal in state.skill_registry().proposal_snapshot() {
        let _ = state.journal().save_skill_proposal(&proposal);
    }
    for payload in state.skill_registry().drain_pending_events() {
        let event = cortex_types::Event::new(
            cortex_types::TurnId::new(),
            cortex_types::CorrelationId::new(),
            payload,
        );
        let _ = state.journal().append(&event);
    }
    hb.tool_calls_since_evolve.store(0, Relaxed);
}

fn checkpoint(state: &DaemonState, stability: &mut crate::stability::StabilityMonitor) {
    let _ = state.journal().gc_unreferenced_blobs();
    let _ = state.journal().create_checkpoint();
    let count = state.journal().event_count().unwrap_or(0);
    stability.record_snapshot(0, count, 0);
    if stability.sample_count() >= 3 {
        let report = stability.generate_report();
        if !report.is_stable {
            tracing::warn!("Stability: {:?}", report.growth_rates);
        }
    }
}

fn autonomous_turn(
    state: &DaemonState,
    hb: &crate::heartbeat::HeartbeatState,
    label: &str,
    prompt: &str,
    on_success: impl FnOnce(&crate::heartbeat::HeartbeatState),
) {
    tracing::info!("Heartbeat: {label} triggered");
    let session_id = format!("autonomous-{label}-{}", chrono::Utc::now().timestamp());
    match state.execute_background_turn(&session_id, prompt, "heartbeat", &[]) {
        Ok(_) => {
            on_success(hb);
            hb.record_llm_call();
            tracing::info!("Heartbeat: {label} completed");
        }
        Err(error) => tracing::warn!("Heartbeat: {label} failed: {error}"),
    }
}
