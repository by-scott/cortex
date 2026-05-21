use std::sync::atomic::Ordering;

use cortex_turn::context::SummaryCache;
use cortex_turn::meta::MetaMonitor;

use super::foreground::ForegroundExecution;
use super::permissions::RuntimePermissionGate;
use super::session_state::{DaemonSession, restore_failed_turn_history};
use super::{
    BroadcastEvent, BroadcastMessage, DaemonState, TracingTurnTracer, post_turn_queue,
    transport_payloads,
};
use crate::turn_executor::{TurnCallbacks, TurnExecutor, TurnExecutorConfig};

struct BuildExecutorInput<'a> {
    cfg: &'a cortex_types::config::CortexConfig,
    resume: &'a cortex_types::ResumePacket,
    session_id: &'a str,
    actor: &'a str,
    source: &'a str,
    execution_scope: cortex_sdk::ExecutionScope,
    turns_since_extract: usize,
    skill_summaries: Option<String>,
    tracer: &'a dyn cortex_turn::orchestrator::TurnTracer,
    control: Option<cortex_turn::orchestrator::TurnControl>,
    on_tpn_complete: Option<&'a (dyn Fn() + Send + Sync)>,
}

#[derive(Clone, Copy)]
struct ExecuteTurnInput<'a> {
    session_id: &'a str,
    prompt: &'a str,
    source: &'a str,
    attachments: &'a [cortex_types::Attachment],
    inline_images: &'a [(String, String)],
    execution_scope: cortex_sdk::ExecutionScope,
    actor_override: Option<&'a str>,
}

impl DaemonState {
    /// Execute a Turn in the given session.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limit
    /// is exceeded, or the LLM turn fails.
    fn execute_turn_inner(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        attachments: &[cortex_types::Attachment],
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner_with_scope(
            session_id,
            prompt,
            source,
            attachments,
            inline_images,
            cortex_sdk::ExecutionScope::Foreground,
        )
    }

    fn execute_turn_inner_with_scope(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        attachments: &[cortex_types::Attachment],
        inline_images: &[(String, String)],
        execution_scope: cortex_sdk::ExecutionScope,
    ) -> Result<String, String> {
        self.execute_turn_inner_with_context(ExecuteTurnInput {
            session_id,
            prompt,
            source,
            attachments,
            inline_images,
            execution_scope,
            actor_override: None,
        })
    }

    fn execute_turn_inner_with_context(
        &self,
        input: ExecuteTurnInput<'_>,
    ) -> Result<String, String> {
        let ExecuteTurnInput {
            session_id,
            prompt,
            source,
            attachments,
            inline_images,
            execution_scope,
            actor_override,
        } = input;
        if Self::tracks_client_session(source) {
            self.set_client_session(source, session_id);
        }

        // Reject early if API key is not configured
        if self.config().api.api_key.is_empty() {
            return Err(
                "API key not configured. Edit config.toml [api].api_key or reinstall with CORTEX_API_KEY".into(),
            );
        }

        // Rate limit check
        if let crate::rate_limiter::RateLimitResult::SessionLimited
        | crate::rate_limiter::RateLimitResult::GlobalLimited =
            self.rate_limiter.check(session_id)
        {
            return Err("rate limit exceeded".into());
        }

        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let tracer = TracingTurnTracer {
            config: cfg.turn.trace.clone(),
        };
        let actor = actor_override.map_or_else(|| self.transport_actor(source), str::to_string);
        let mut session = self.take_or_create_session(session_id);
        let resume = self.resume_for_actor(&actor);
        let history_len_before_turn = session.history.len();
        let result = self.with_registered_turn_control(session_id, |control, on_tpn_complete| {
            let executor = self.build_executor(BuildExecutorInput {
                cfg: &cfg,
                resume: &resume,
                session_id,
                actor: &actor,
                source,
                execution_scope,
                turns_since_extract: session.turns_since_extract,
                skill_summaries,
                tracer: &tracer,
                control: Some(control.clone()),
                on_tpn_complete: Some(on_tpn_complete),
            });

            let callbacks = TurnCallbacks { on_event: None };

            let turn_input = crate::turn_executor::TurnInput {
                text: prompt,
                attachments,
                inline_images,
            };
            let gate = RuntimePermissionGate {
                state: self,
                session_id,
                actor: &actor,
                source,
                auto_approve_up_to: cfg.risk.auto_approve_up_to,
                control: Some(&control),
                on_event: None,
            };
            executor.execute(
                &turn_input,
                &mut session.history,
                &gate,
                &mut session.monitor,
                &mut session.summary_cache,
                &callbacks,
            )
        });

        if let Err(error) = &result {
            restore_failed_turn_history(
                &mut session.history,
                history_len_before_turn,
                &crate::turn_executor::TurnInput {
                    text: prompt,
                    attachments,
                    inline_images,
                },
                error,
            );
        }
        let output = self.process_turn_result(&result, &mut session, session_id, &actor, source);
        if let (Ok(text), Ok(turn_output)) = (&output, &result) {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::done(text.clone(), turn_output.response_parts.clone()),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
    }

    /// Execute a turn in the given session.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limiting
    /// blocks the turn, or the underlying turn execution fails.
    pub fn execute_turn(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner(session_id, prompt, source, &[], inline_images)
    }

    /// Execute a background turn that should not consume foreground queue
    /// ownership or mark the foreground runtime as busy.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limiting
    /// blocks the turn, or the underlying turn execution fails.
    pub(crate) fn execute_background_turn(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner_with_scope(
            session_id,
            prompt,
            source,
            &[],
            inline_images,
            cortex_sdk::ExecutionScope::Background,
        )
    }

    pub(crate) fn execute_background_turn_for_actor(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        actor: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner_with_context(ExecuteTurnInput {
            session_id,
            prompt,
            source,
            attachments: &[],
            inline_images,
            execution_scope: cortex_sdk::ExecutionScope::Background,
            actor_override: Some(actor),
        })
    }

    /// Execute a Turn with streaming callbacks for SSE delivery.
    ///
    /// Similar to `execute_turn` but wires up a unified event callback so
    /// callers can stream partial user-visible text, observer text, and tool progress.
    fn execute_turn_streaming_inner(
        &self,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        if Self::tracks_client_session(source) {
            self.set_client_session(source, session_id);
        }

        // Reject early if API key is not configured
        if self.config().api.api_key.is_empty() {
            return Err(
                "API key not configured. Edit config.toml [api].api_key or reinstall with CORTEX_API_KEY".into(),
            );
        }

        // Rate limit check
        if let crate::rate_limiter::RateLimitResult::SessionLimited
        | crate::rate_limiter::RateLimitResult::GlobalLimited =
            self.rate_limiter.check(session_id)
        {
            return Err("rate limit exceeded".into());
        }

        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let actor = self.transport_actor(source);
        let mut session = self.take_or_create_session(session_id);
        let resume = self.resume_for_actor(&actor);
        let history_len_before_turn = session.history.len();
        let result = self.with_registered_turn_control(session_id, |control, on_tpn_complete| {
            let executor = self.build_executor(BuildExecutorInput {
                cfg: &cfg,
                resume: &resume,
                session_id,
                actor: &actor,
                source,
                execution_scope: cortex_sdk::ExecutionScope::Foreground,
                turns_since_extract: session.turns_since_extract,
                skill_summaries,
                tracer,
                control: Some(control.clone()),
                on_tpn_complete: Some(on_tpn_complete),
            });

            // Wrap callbacks to also broadcast events on the session channel
            let bc_tx = self.session_broadcast(session_id);
            let bc_sid = session_id.to_string();
            let bc_src = source.to_string();
            let wrapped_on_event = move |event: &cortex_turn::orchestrator::TurnStreamEvent| {
                on_event(event);
                if let Some(broadcast_event) = BroadcastEvent::from_turn_stream_event(event) {
                    let _ = bc_tx.send(BroadcastMessage {
                        session_id: bc_sid.clone(),
                        source: bc_src.clone(),
                        event: broadcast_event,
                    });
                }
            };

            let callbacks = TurnCallbacks {
                on_event: Some(&wrapped_on_event),
            };

            let gate = RuntimePermissionGate {
                state: self,
                session_id,
                actor: &actor,
                source,
                auto_approve_up_to: cfg.risk.auto_approve_up_to,
                control: Some(&control),
                on_event: Some(&wrapped_on_event),
            };
            executor.execute(
                input,
                &mut session.history,
                &gate,
                &mut session.monitor,
                &mut session.summary_cache,
                &callbacks,
            )
        });
        if let Err(error) = &result {
            restore_failed_turn_history(
                &mut session.history,
                history_len_before_turn,
                input,
                error,
            );
        }
        let output = self.process_turn_output_result_streaming(
            result,
            &mut session,
            session_id,
            &actor,
            source,
        );
        if let Ok(turn_output) = &output {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::done(
                    turn_output.response_text.clone().unwrap_or_default(),
                    turn_output.response_parts.clone(),
                ),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
    }

    pub(crate) fn execute_turn_streaming(
        &self,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        self.execute_turn_streaming_inner(session_id, input, source, on_event, tracer)
    }

    pub(crate) fn execute_foreground_turn_streaming(
        &self,
        foreground: &ForegroundExecution,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        let release = foreground.release_handle();
        self.execute_turn_streaming_inner(
            session_id,
            input,
            source,
            move |event| {
                on_event(event);
                if matches!(
                    event,
                    cortex_turn::orchestrator::TurnStreamEvent::Boundary(
                        cortex_turn::orchestrator::TurnStreamBoundary::TpnComplete
                    )
                ) {
                    release.finish_visible();
                }
            },
            tracer,
        )
    }

    /// Build skill summaries for system prompt injection.
    fn build_skill_summaries(&self, cfg: &cortex_types::config::CortexConfig) -> Option<String> {
        use std::fmt::Write as _;
        if !cfg.skills.inject_summaries {
            return None;
        }
        let sums = self
            .skill_registry
            .summaries(cfg.skills.max_active_summaries);
        if sums.is_empty() {
            return None;
        }
        let mut text = String::from("## Skills\n\nReusable procedures available this turn:\n");
        for s in &sums {
            let _ = writeln!(text, "- {}: {}", s.name, s.description);
        }
        Some(text)
    }

    fn resume_for_actor(&self, actor: &str) -> cortex_types::ResumePacket {
        let goals = self
            .goal_store
            .list_open_for_actor(actor)
            .unwrap_or_default()
            .into_iter()
            .take(8)
            .map(|goal| goal.context_line())
            .collect();
        cortex_types::ResumePacket {
            goals,
            ..cortex_types::ResumePacket::default()
        }
    }

    /// Build a `TurnExecutor` with the standard subsystem references.
    fn build_executor<'a>(&'a self, input: BuildExecutorInput<'a>) -> TurnExecutor<'a> {
        let BuildExecutorInput {
            cfg,
            resume,
            session_id,
            actor,
            source,
            execution_scope,
            turns_since_extract,
            skill_summaries,
            tracer,
            control,
            on_tpn_complete,
        } = input;
        TurnExecutor::new(TurnExecutorConfig {
            config: cfg,
            journal: &self.journal,
            memory_store: &self.memory_store,
            llm: self.llm.as_ref(),
            tools: &self.tools,
            prompt_manager: &self.prompt_manager,
            embedding_client: self.embedding_client.as_deref(),
            embedding_store: self.embedding_store.as_deref(),
            embedding_health: Some(&*self.embedding_health),
            skill_summaries,
            skill_registry: Some(&self.skill_registry),
            data_dir: &self.data_dir,
            max_output_tokens: self.max_output_tokens,
            resume,
            turns_since_extract,
            tracer,
            vision_llm: self.vision_llm.as_deref(),
            control,
            on_tpn_complete,
            session_id,
            actor,
            source,
            execution_scope,
        })
    }

    /// Take a session from the in-memory map or restore/create it.
    fn take_or_create_session(&self, session_id: &str) -> DaemonSession {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions
            .remove(session_id)
            .unwrap_or_else(|| self.restore_or_create_session(session_id))
    }

    /// Process a Turn result: update counters, record metrics, extract text.
    fn process_turn_result(
        &self,
        result: &Result<crate::turn_executor::TurnOutput, String>,
        session: &mut DaemonSession,
        session_id: &str,
        actor: &str,
        source: &str,
    ) -> Result<String, String> {
        match result {
            Ok(output) => {
                self.record_turn_metrics(output);
                self.update_session_after_turn(output, session);
                self.enqueue_deferred_post_turn(session_id, actor, source, output);
                transport_payloads::extract_final_response_text(output)
            }
            Err(e) => {
                self.metrics.record_turn_error();
                Err(e.clone())
            }
        }
    }

    fn process_turn_output_result_streaming(
        &self,
        result: Result<crate::turn_executor::TurnOutput, String>,
        session: &mut DaemonSession,
        session_id: &str,
        actor: &str,
        source: &str,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        match result {
            Ok(output) => {
                self.record_turn_metrics(&output);
                self.update_session_after_turn(&output, session);
                self.enqueue_deferred_post_turn(session_id, actor, source, &output);
                if output
                    .response_text
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || !output.response_parts.is_empty()
                {
                    Ok(output)
                } else {
                    Ok(transport_payloads::synthesize_empty_turn_output(output))
                }
            }
            Err(e) => {
                self.metrics.record_turn_error();
                Err(e)
            }
        }
    }

    fn enqueue_deferred_post_turn(
        &self,
        session_id: &str,
        actor: &str,
        source: &str,
        output: &crate::turn_executor::TurnOutput,
    ) {
        let Some(deferred) = output.deferred_post_turn.clone() else {
            return;
        };
        self.enqueue_post_turn(post_turn_queue::PostTurnJob {
            session_id: session_id.to_string(),
            actor: actor.to_string(),
            source: source.to_string(),
            input: deferred.input,
            final_text: deferred.final_text,
            events: deferred.events,
            history: deferred.history,
            turns_since_extract: deferred.turns_since_extract,
            should_extract: deferred.should_extract,
        });
    }

    fn record_turn_metrics(&self, output: &crate::turn_executor::TurnOutput) {
        self.metrics.record_turn();
        self.metrics.record_tokens(
            output.total_input_tokens as u64,
            output.total_output_tokens as u64,
            output.total_cache_read_input_tokens as u64,
            output.total_cache_creation_input_tokens as u64,
        );
        if output.last_call_input_tokens > 0
            || output.last_call_output_tokens > 0
            || output.last_call_cache_read_input_tokens > 0
            || output.last_call_cache_creation_input_tokens > 0
        {
            self.metrics.record_last_call_tokens(
                output.last_call_input_tokens as u64,
                output.last_call_output_tokens as u64,
                output.last_call_cache_read_input_tokens as u64,
                output.last_call_cache_creation_input_tokens as u64,
            );
        }
        for _ in 0..output.tool_call_count {
            self.metrics.record_tool_call(false);
        }
        for _ in 0..output.tool_error_count {
            self.metrics.record_tool_call(true);
        }
        for _ in 0..output.extracted_memory_count {
            self.metrics.record_memory_capture();
        }
        for _ in 0..output.recalled_memory_count {
            self.metrics.record_memory_recall();
        }
        for _ in &output.alerts {
            self.metrics.record_alert();
        }
    }

    /// Update session counters and heartbeat state after a successful Turn.
    fn update_session_after_turn(
        &self,
        output: &crate::turn_executor::TurnOutput,
        session: &mut DaemonSession,
    ) {
        session.turn_count += 1;
        session.meta.total_input_tokens = session
            .meta
            .total_input_tokens
            .saturating_add(output.total_input_tokens as u64);
        session.meta.total_output_tokens = session
            .meta
            .total_output_tokens
            .saturating_add(output.total_output_tokens as u64);
        session.turns_since_extract += 1;
        // Reset extract counter: after successful extraction, or if we've
        // overshot the threshold (extraction tried but produced nothing).
        let threshold = self.config().memory.extract_min_turns;
        let scheduled_extract = output
            .deferred_post_turn
            .as_ref()
            .is_some_and(|job| job.should_extract);
        if scheduled_extract || session.turns_since_extract > threshold {
            session.turns_since_extract = 0;
        }
        if output.extracted_memory_count > 0 {
            let count = u32::try_from(output.extracted_memory_count).unwrap_or(u32::MAX);
            self.heartbeat_state
                .pending_consolidation
                .fetch_add(count, Ordering::Relaxed);
            self.heartbeat_state
                .pending_embeddings
                .fetch_add(count, Ordering::Relaxed);
        }
    }

    /// Persist session to disk and reinsert into the in-memory map.
    fn persist_and_reinsert(&self, session_id: &str, mut session: DaemonSession) {
        session.meta.turn_count = session.turn_count;
        let _ = self
            .session_store
            .save_history(&session.meta.id, &session.history);
        let _ = self.session_store.save(&session.meta);
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string(), session);
    }

    /// Try to restore a session from disk (preserving history and turn count),
    /// or create a fresh one if the `session_id` doesn't exist on disk.
    /// Ended sessions (with `ended_at` set) are not restored -- a new session
    /// is created instead.
    fn restore_or_create_session(&self, session_id: &str) -> DaemonSession {
        // Try to restore from SessionStore
        if let Some(meta) = self
            .session_store
            .list()
            .into_iter()
            .find(|m| m.id.to_string() == session_id)
        {
            // Do not restore already-ended sessions.
            if meta.ended_at.is_some() {
                return self.new_daemon_session();
            }
            let history = self.session_store.load_history(&meta.id);
            let turn_count = meta.turn_count;
            let cfg = self.config();
            return DaemonSession {
                meta,
                turn_count,
                turns_since_extract: turn_count, // resume from persisted count
                history,
                monitor: MetaMonitor::new(
                    cfg.metacognition.doom_loop_threshold,
                    cfg.metacognition.fatigue_threshold,
                    cfg.metacognition.duration_limit_secs,
                    cfg.metacognition.frame_anchoring_threshold,
                    cfg.metacognition.frame_audit.clone(),
                ),
                summary_cache: SummaryCache::new(),
            };
        }
        self.new_daemon_session()
    }

    fn new_daemon_session(&self) -> DaemonSession {
        let (_, meta) = self.session_manager().create_session();
        let cfg = self.config();
        DaemonSession {
            meta,
            history: Vec::new(),
            turn_count: 0,
            turns_since_extract: 0,
            monitor: MetaMonitor::new(
                cfg.metacognition.doom_loop_threshold,
                cfg.metacognition.fatigue_threshold,
                cfg.metacognition.duration_limit_secs,
                cfg.metacognition.frame_anchoring_threshold,
                cfg.metacognition.frame_audit.clone(),
            ),
            summary_cache: SummaryCache::new(),
        }
    }

    pub(crate) fn end_session(&self, session_id: &str) {
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session_id);
        if let Some(mut session) = removed {
            self.session_manager()
                .end_session(&mut session.meta, session.turn_count);
        } else {
            let sm = self.session_manager();
            if let Some(mut meta) = sm
                .list_sessions()
                .into_iter()
                .find(|s| s.id.to_string() == session_id || s.name.as_deref() == Some(session_id))
                && meta.ended_at.is_none()
            {
                let tc = meta.turn_count;
                sm.end_session(&mut meta, tc);
            }
        }
        // Remove the per-session broadcast channel so it can be collected.
        self.session_channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session_id);
    }
}
