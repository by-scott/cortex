use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Message, Payload, TurnId, TurnState};

use super::strip_think_tags;
use crate::causal::CausalAnalyzer;
use crate::confidence::ConfidenceTracker;
use crate::context::pressure::{PressureLevel, compute_occupancy, estimate_tokens};
use crate::context::pressure_response::{self, PressureAction};
use crate::llm::LlmClient;
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::working_memory::WorkingMemoryManager;

use super::journal_append;
use super::post_turn::run_post_turn_batch;
use super::{TraceCategory, TurnConfig, TurnError, TurnResult, TurnTracer};

// ── DMN Phase ───────────────────────────────────────────────

pub struct DmnPhaseContext<'a> {
    pub confidence: &'a mut ConfidenceTracker,
    pub meta_monitor: &'a MetaMonitor,
    pub working_mem: &'a mut WorkingMemoryManager,
    pub reasoning_engine: &'a ReasoningEngine,
    pub journal: &'a Journal,
    pub turn_id: TurnId,
    pub corr_id: CorrelationId,
    pub events_log: &'a mut Vec<Payload>,
    pub tracer: &'a dyn TurnTracer,
}

pub fn complete_dmn_phase(ctx: &mut DmnPhaseContext<'_>) {
    // Step 1: Decision confidence -- assess and emit events
    let conf_events = ctx.confidence.assess();
    for ev in conf_events {
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }

    ctx.tracer.trace_at(
        TraceCategory::Meta,
        cortex_types::TraceLevel::Summary,
        &format!("Confidence: {:.2}", ctx.confidence.score()),
    );

    // Step 2: MetaMonitor -- unified five-dimension metacognitive check
    let alerts = ctx
        .meta_monitor
        .check_with_confidence(ctx.confidence.score());
    for alert in alerts {
        let kind = &alert.kind;
        let ev = Payload::ImpasseDetected {
            detector: format!("{kind:?}"),
            details: alert.message,
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }

    // Step 3: Reasoning reflection -- evaluate reasoning chain quality if active
    if let Some(chain) = ctx.reasoning_engine.chain() {
        let ev = reflect_on_reasoning(chain);
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }

    // Step 4: Causal retrospect -- heuristic causal analysis on Turn events
    if ctx.events_log.len() >= 3 {
        let stored = events_to_stored(ctx.events_log, ctx.turn_id, ctx.corr_id);
        let analyzer = CausalAnalyzer::new();
        let links = analyzer.analyze_heuristic(&stored);
        let chains = analyzer.build_chains(&links);
        if !chains.is_empty() {
            let longest = chains
                .iter()
                .max_by_key(|c| c.link_count())
                .map(cortex_types::CausalChain::format)
                .unwrap_or_default();
            let root_causes: Vec<String> = chains.iter().map(|c| c.root_cause.clone()).collect();
            let ev = Payload::CausalRetrospect {
                chain_count: chains.len(),
                longest_chain_summary: longest,
                root_causes,
            };
            journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
            ctx.events_log.push(ev);
        }
    }

    // Step 5: Working memory -- decay and evict stale items before completing
    let wm_events = ctx.working_mem.decay_and_evict(chrono::Utc::now());
    if !wm_events.is_empty() {
        ctx.tracer.trace_at(
            TraceCategory::Memory,
            cortex_types::TraceLevel::Summary,
            &format!("Working memory decay: {} events", wm_events.len()),
        );
    }
    for ev in wm_events {
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }

    // Step 6: Usage pattern feedback -- generate config suggestions from RPE stats
    let suggestions = ctx.meta_monitor.rpe.usage_suggestions();
    for suggestion in suggestions {
        let ev = Payload::MetaControlApplied {
            action: format!("config_suggestion: {}", suggestion.message),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }

    // Step 7: Drift detection -- warn about heavily imbalanced tool usage
    let drift_warnings = ctx.meta_monitor.rpe.detect_drift();
    for warning in drift_warnings {
        let ev = Payload::MetaControlApplied {
            action: format!("drift_detected: {warning}"),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }
}

/// Evaluate the quality of a completed reasoning chain.
///
/// Three dimensions: step coherence (0.3), confidence trend (0.4),
/// conclusion reliability (0.3).
pub fn reflect_on_reasoning(chain: &cortex_types::ReasoningChain) -> Payload {
    let mut weaknesses = Vec::new();

    // Dimension 1: Step coherence (0.3 weight)
    // Check for consecutive duplicate step types (indicates stuck reasoning)
    let coherence_score = {
        let steps = &chain.steps;
        if steps.len() < 2 {
            1.0
        } else {
            let consecutive_dups: u32 = steps
                .windows(2)
                .filter(|w| w[0].step_type == w[1].step_type)
                .count()
                .try_into()
                .unwrap_or(u32::MAX);
            let pairs: u32 = (steps.len() - 1).try_into().unwrap_or(u32::MAX);
            let ratio = f64::from(consecutive_dups) / f64::from(pairs);
            (1.0 - ratio).max(0.0)
        }
    };
    if coherence_score < 0.5 {
        weaknesses.push("incoherent_steps".to_string());
    }

    // Dimension 2: Confidence trend (0.4 weight)
    // Compute first-order differences; sustained decline is negative
    let trend_score = {
        let steps = &chain.steps;
        if steps.len() < 2 {
            0.5
        } else {
            let diffs: Vec<f64> = steps
                .windows(2)
                .map(|w| w[1].confidence - w[0].confidence)
                .collect();
            let diffs_len: u32 = diffs.len().try_into().unwrap_or(u32::MAX);
            let avg_diff = diffs.iter().sum::<f64>() / f64::from(diffs_len);
            // Map avg_diff from [-1,1] to [0,1]: avg_diff * 0.5 + 0.5
            avg_diff.mul_add(0.5, 0.5).clamp(0.0, 1.0)
        }
    };
    if trend_score < 0.3 {
        weaknesses.push("declining_confidence".to_string());
    }

    // Dimension 3: Conclusion reliability (0.3 weight)
    let conclusion_score = chain
        .steps
        .iter()
        .rev()
        .find(|s| s.step_type == cortex_types::ReasoningStepType::Conclusion)
        .map_or(0.0, |s| s.confidence);
    if conclusion_score < 0.3 && chain.conclusion.is_some() {
        weaknesses.push("weak_conclusion".to_string());
    }
    if chain.conclusion.is_none() {
        weaknesses.push("no_conclusion".to_string());
    }

    let quality_score =
        coherence_score.mul_add(0.3, trend_score.mul_add(0.4, conclusion_score * 0.3));

    Payload::ReasoningReflection {
        chain_id: chain.id.clone(),
        quality_score,
        weaknesses,
    }
}

// ── Post-TPN phase ──────────────────────────────────────────

pub struct PostTpnContext<'a> {
    pub state: TurnState,
    pub final_text: Option<String>,
    pub response_media: Vec<cortex_types::Attachment>,
    pub reasoning_engine: ReasoningEngine,
    pub confidence: ConfidenceTracker,
    pub meta_monitor: MetaMonitor,
    pub working_mem: WorkingMemoryManager,
    pub events_log: Vec<Payload>,
    pub prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    pub skill_registry: Option<&'a crate::skills::SkillRegistry>,
    pub input: &'a str,
    pub llm: &'a dyn LlmClient,
    pub post_turn_llm: Option<&'a dyn LlmClient>,
    pub history: &'a mut Vec<Message>,
    pub config: &'a TurnConfig,
    pub journal: &'a Journal,
    pub turn_id: TurnId,
    pub corr_id: CorrelationId,
    pub tracer: &'a dyn TurnTracer,
}

pub async fn run_post_tpn_phase(mut ctx: PostTpnContext<'_>) -> Result<TurnResult, TurnError> {
    finalize_reasoning(
        &mut ctx.reasoning_engine,
        ctx.final_text.as_ref(),
        ctx.journal,
        ctx.turn_id,
        ctx.corr_id,
        &mut ctx.events_log,
    );
    complete_dmn_phase(&mut DmnPhaseContext {
        confidence: &mut ctx.confidence,
        meta_monitor: &ctx.meta_monitor,
        working_mem: &mut ctx.working_mem,
        reasoning_engine: &ctx.reasoning_engine,
        journal: ctx.journal,
        turn_id: ctx.turn_id,
        corr_id: ctx.corr_id,
        events_log: &mut ctx.events_log,
        tracer: ctx.tracer,
    });
    ctx.meta_monitor
        .end_turn(f64::from(u32::try_from(ctx.events_log.len()).unwrap_or(u32::MAX)) / 50.0);

    let dmn_llm = ctx.post_turn_llm.unwrap_or(ctx.llm);
    let (prompt_updates, entity_relations, mut extracted_memories) = run_post_turn_batch(
        ctx.prompt_manager,
        &ctx.events_log,
        ctx.input,
        ctx.final_text.as_ref(),
        dmn_llm,
        ctx.history,
        ctx.config,
    )
    .await;
    if !turn_saved_memory(&ctx.events_log) {
        extracted_memories.extend(super::post_turn::extract_explicit_user_memories(ctx.input));
    }

    if !extracted_memories.is_empty() {
        ctx.tracer.trace_at(
            TraceCategory::Memory,
            cortex_types::TraceLevel::Basic,
            &format!("Extracted {} memories", extracted_memories.len()),
        );
    }
    if !prompt_updates.is_empty() {
        ctx.tracer.trace_at(
            TraceCategory::Memory,
            cortex_types::TraceLevel::Summary,
            "Prompt evolution triggered",
        );
    }

    emit_quality_signals(&mut ctx);

    ctx.tracer.trace_at(
        TraceCategory::Phase,
        cortex_types::TraceLevel::Minimal,
        "Turn completed",
    );

    finish_turn(
        FinishTurnInput {
            state: ctx.state,
            final_text: ctx.final_text,
            response_media: ctx.response_media,
            prompt_updates,
            entity_relations,
            extracted_memories,
        },
        ctx.journal,
        ctx.turn_id,
        ctx.corr_id,
        &mut ctx.events_log,
        ctx.config.strip_think_tags,
    )
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

fn emit_quality_signals(ctx: &mut PostTpnContext<'_>) {
    let modifying_tools: Vec<String> = ctx
        .events_log
        .iter()
        .filter_map(|ev| {
            if let Payload::ToolInvocationIntent { tool_name, .. } = ev
                && matches!(tool_name.as_str(), "write" | "edit" | "bash")
            {
                return Some(tool_name.clone());
            }
            None
        })
        .collect();
    if modifying_tools.is_empty() {
        return;
    }

    let qc = Payload::QualityCheckSuggested { modifying_tools };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &qc);
    ctx.events_log.push(qc);

    if let Some(registry) = ctx.skill_registry {
        let event_kinds = vec!["QualityCheckSuggested".to_string()];
        let activated = registry.activated_skills_for_events(&event_kinds);
        for summary in activated {
            let ev = Payload::SkillInvoked {
                name: summary.name,
                trigger: cortex_types::InvocationTrigger::SignalDriven(String::from(
                    "QualityCheckSuggested",
                ))
                .to_string(),
                execution_mode: "post_turn".to_string(),
            };
            journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
            ctx.events_log.push(ev);
        }
    }
}

pub fn finalize_reasoning(
    engine: &mut ReasoningEngine,
    final_text: Option<&String>,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) {
    if engine.is_active() {
        let conclusion = final_text.map_or("No explicit conclusion reached", String::as_str);
        if let Some(ev) = engine.complete(conclusion) {
            journal_append(journal, turn_id, corr_id, &ev);
            events_log.push(ev);
        }
    }
}

pub struct FinishTurnInput {
    pub state: TurnState,
    pub final_text: Option<String>,
    pub response_media: Vec<cortex_types::Attachment>,
    pub prompt_updates: Vec<(cortex_types::PromptLayer, String)>,
    pub entity_relations: Vec<cortex_types::MemoryRelation>,
    pub extracted_memories: Vec<cortex_types::MemoryEntry>,
}

pub fn finish_turn(
    input: FinishTurnInput,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
    strip_think: bool,
) -> Result<TurnResult, TurnError> {
    let state = input
        .state
        .try_transition(TurnState::Completed)
        .map_err(|e| TurnError::StateTransition(e.to_string()))?;

    let payload = Payload::TurnCompleted;
    journal_append(journal, turn_id, corr_id, &payload);
    events_log.push(payload);

    let response_text = if strip_think {
        input.final_text.map(|t| strip_think_tags(&t))
    } else {
        input.final_text
    };

    Ok(TurnResult {
        response_text,
        response_media: input.response_media,
        state,
        events: std::mem::take(events_log),
        prompt_updates: input.prompt_updates,
        entity_relations: input.entity_relations,
        extracted_memories: input.extracted_memories,
    })
}

// ── Pressure handling ───────────────────────────────────────

pub struct PressureContext<'a> {
    pub history: &'a mut Vec<Message>,
    pub working_mem: &'a mut WorkingMemoryManager,
    pub compress_template: Option<&'a String>,
    pub summary_cache: &'a mut crate::context::SummaryCache,
    pub journal: &'a Journal,
    pub turn_id: TurnId,
    pub corr_id: CorrelationId,
    pub events_log: &'a mut Vec<Payload>,
    pub llm: &'a dyn LlmClient,
    pub max_tokens: usize,
    pub pressure_thresholds: [f64; 4],
}

fn apply_working_memory_pressure(ctx: &mut PressureContext<'_>, action: &PressureAction) {
    match action {
        PressureAction::AccelerateDecay => {
            let decay_events = ctx.working_mem.decay_and_evict(chrono::Utc::now());
            for ev in decay_events {
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
                ctx.events_log.push(ev);
            }
        }
        PressureAction::TrimWorkingMemory { keep } => {
            while ctx.working_mem.active_count() > *keep {
                let evict_events = ctx.working_mem.decay_and_evict(chrono::Utc::now());
                for ev in evict_events {
                    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
                    ctx.events_log.push(ev);
                }
                if ctx.working_mem.active_count() > *keep {
                    break;
                }
            }
        }
        PressureAction::ClearWorkingMemory => {
            while ctx.working_mem.active_count() > 0 {
                let evict_events = ctx.working_mem.decay_and_evict(chrono::Utc::now());
                for ev in evict_events {
                    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
                    ctx.events_log.push(ev);
                }
                if ctx.working_mem.active_count() > 0 {
                    break;
                }
            }
        }
        PressureAction::CompressHistory => {}
    }
}

pub async fn apply_compress_history(ctx: &mut PressureContext<'_>) {
    if let Some(tpl) = ctx.compress_template {
        let summarize_max = ctx.max_tokens / 4;
        let result = crate::context::summarize_and_compress(
            ctx.history,
            ctx.llm,
            tpl,
            summarize_max,
            ctx.summary_cache,
        )
        .await;
        match result {
            crate::context::SummarizeResult::Summarized {
                original_tokens,
                compressed_tokens,
                preserved_user_messages,
                suffix_messages,
                summary,
                replacement_messages,
            } => {
                let compact_ev = Payload::ContextCompacted {
                    original_tokens,
                    compressed_tokens,
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &compact_ev);
                ctx.events_log.push(compact_ev);
                let boundary_ev = Payload::ContextCompactBoundary {
                    original_tokens,
                    compressed_tokens,
                    preserved_user_messages,
                    suffix_messages,
                    summary,
                    replacement_messages,
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &boundary_ev);
                ctx.events_log.push(boundary_ev);
            }
            crate::context::SummarizeResult::Fallback { removed } => {
                if removed > 0 {
                    let compressed_tokens: usize = ctx
                        .history
                        .iter()
                        .map(|m| estimate_tokens(&m.text_content()))
                        .sum();
                    let compact_ev = Payload::ContextCompacted {
                        original_tokens: compressed_tokens + removed * 100,
                        compressed_tokens,
                    };
                    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &compact_ev);
                    ctx.events_log.push(compact_ev);
                }
            }
            crate::context::SummarizeResult::NoCompression => {}
        }
    }
}

pub async fn handle_context_pressure(ctx: &mut PressureContext<'_>) {
    let used = ctx
        .history
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum::<usize>();
    let occupancy = compute_occupancy(used, ctx.max_tokens);
    let level = PressureLevel::from_occupancy(occupancy, &ctx.pressure_thresholds);

    if level <= PressureLevel::Normal {
        return;
    }

    let pressure_obs = Payload::ContextPressureObserved {
        level: level.name().to_string(),
        occupancy,
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &pressure_obs);
    ctx.events_log.push(pressure_obs);

    let resp = pressure_response::respond(level);
    for action in &resp.actions {
        match action {
            PressureAction::AccelerateDecay
            | PressureAction::TrimWorkingMemory { .. }
            | PressureAction::ClearWorkingMemory => {
                apply_working_memory_pressure(ctx, action);
            }
            PressureAction::CompressHistory => {
                apply_compress_history(ctx).await;
            }
        }
    }
    for ev in resp.events {
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
        ctx.events_log.push(ev);
    }
}

// ── Helpers ─────────────────────────────────────────────────

/// Map a `Payload` variant to its type name string.
const fn event_payload_type_name(payload: &Payload) -> &'static str {
    match payload {
        Payload::TurnStarted => "TurnStarted",
        Payload::TurnCompleted => "TurnCompleted",
        Payload::TurnInterrupted => "TurnInterrupted",
        Payload::SessionStarted { .. } => "SessionStarted",
        Payload::SessionEnded { .. } => "SessionEnded",
        Payload::UserMessage { .. } => "UserMessage",
        Payload::AssistantMessage { .. } => "AssistantMessage",
        Payload::ToolInvocationIntent { .. } => "ToolInvocationIntent",
        Payload::ToolInvocationResult { .. } => "ToolInvocationResult",
        Payload::PermissionRequested { .. } => "PermissionRequested",
        Payload::PermissionGranted { .. } => "PermissionGranted",
        Payload::PermissionDenied { .. } => "PermissionDenied",
        Payload::ContextPressureObserved { .. } => "ContextPressureObserved",
        Payload::ContextCompacted { .. } => "ContextCompacted",
        Payload::ContextCompactBoundary { .. } => "ContextCompactBoundary",
        Payload::ImpasseDetected { .. } => "ImpasseDetected",
        Payload::ConflictDetected { .. } => "ConflictDetected",
        Payload::MetaControlApplied { .. } => "MetaControlApplied",
        Payload::FrameCheckResult { .. } => "FrameCheckResult",
        Payload::GoalSet { .. } => "GoalSet",
        Payload::GoalShifted { .. } => "GoalShifted",
        Payload::GoalCompleted { .. } => "GoalCompleted",
        Payload::MemoryCaptured { .. } => "MemoryCaptured",
        Payload::MemoryMaterialized { .. } => "MemoryMaterialized",
        Payload::MemoryStabilized { .. } => "MemoryStabilized",
        Payload::LlmCallCompleted { .. } => "LlmCallCompleted",
        Payload::WorkingMemoryItemActivated { .. } => "WorkingMemoryItemActivated",
        Payload::WorkingMemoryItemRehearsed { .. } => "WorkingMemoryItemRehearsed",
        Payload::WorkingMemoryItemEvicted { .. } => "WorkingMemoryItemEvicted",
        Payload::WorkingMemoryCapacityExceeded { .. } => "WorkingMemoryCapacityExceeded",
        Payload::ChannelScheduled { .. } => "ChannelScheduled",
        Payload::MaintenanceExecuted { .. } => "MaintenanceExecuted",
        Payload::EmergencyTriggered { .. } => "EmergencyTriggered",
        Payload::GuardrailTriggered { .. } => "GuardrailTriggered",
        Payload::ExternalInputObserved { .. } => "ExternalInputObserved",
        Payload::ConfidenceAssessed { .. } => "ConfidenceAssessed",
        Payload::ConfidenceLow { .. } => "ConfidenceLow",
        Payload::PressureResponseApplied { .. } => "PressureResponseApplied",
        Payload::AcpClientSpawned { .. } => "AcpClientSpawned",
        Payload::AcpClientResponse { .. } => "AcpClientResponse",
        Payload::AgentWorkerSpawned { .. } => "AgentWorkerSpawned",
        Payload::AgentWorkerCompleted { .. } => "AgentWorkerCompleted",
        Payload::DelegationCompleted { .. } => "DelegationCompleted",
        Payload::PromptUpdated { .. } => "PromptUpdated",
        Payload::ReasoningStarted { .. } => "ReasoningStarted",
        Payload::ReasoningStepCompleted { .. } => "ReasoningStepCompleted",
        Payload::ReasoningBranchEvaluated { .. } => "ReasoningBranchEvaluated",
        Payload::ReasoningChainCompleted { .. } => "ReasoningChainCompleted",
        Payload::TaskDecomposed { .. } => "TaskDecomposed",
        Payload::TaskAggregated { .. } => "TaskAggregated",
        Payload::TaskClaimed { .. } => "TaskClaimed",
        Payload::WorkflowSpecLoaded { .. } => "WorkflowSpecLoaded",
        Payload::CausalAnalysisCompleted { .. } => "CausalAnalysisCompleted",
        Payload::EmbeddingModelSwitched { .. } => "EmbeddingModelSwitched",
        Payload::EmbeddingDegraded { .. } => "EmbeddingDegraded",
        Payload::SkillInvoked { .. } => "SkillInvoked",
        Payload::SkillCompleted { .. } => "SkillCompleted",
        Payload::PluginLoaded { .. } => "PluginLoaded",
        Payload::AuditQueryExecuted { .. } => "AuditQueryExecuted",
        Payload::HealthAutoRecoveryTriggered { .. } => "HealthAutoRecoveryTriggered",
        Payload::AlertFired { .. } => "AlertFired",
        Payload::SecuritySanitized { .. } => "SecuritySanitized",
        Payload::ConfigValidated { .. } => "ConfigValidated",
        Payload::PluginDiscovered { .. } => "PluginDiscovered",
        Payload::MemorySplit { .. } => "MemorySplit",
        Payload::MemoryGraphHealthAssessed { .. } => "MemoryGraphHealthAssessed",
        Payload::MemoryRelationReorganized { .. } => "MemoryRelationReorganized",
        Payload::ReasoningReflection { .. } => "ReasoningReflection",
        Payload::CausalRetrospect { .. } => "CausalRetrospect",
        Payload::SnapshotCreated { .. } => "SnapshotCreated",
        Payload::ProjectionCheckpoint { .. } => "ProjectionCheckpoint",
        Payload::SelfModification { .. } => "SelfModification",
        Payload::SideEffectRecorded { .. } => "SideEffectRecorded",
        Payload::ExternalizedPayload { .. } => "ExternalizedPayload",
        Payload::QualityCheckSuggested { .. } => "QualityCheckSuggested",
        Payload::ExplorationTriggered { .. } => "ExplorationTriggered",
        Payload::MaintenanceCycleCompleted { .. } => "MaintenanceCycleCompleted",
    }
}

/// Convert `Payload` list to `StoredEvent` list for `CausalAnalyzer` consumption.
fn events_to_stored(
    payloads: &[Payload],
    turn_id: TurnId,
    corr_id: CorrelationId,
) -> Vec<cortex_kernel::StoredEvent> {
    use cortex_kernel::StoredEvent;

    let turn_str = turn_id.to_string();
    let corr_str = corr_id.to_string();
    let base_time = chrono::Utc::now();

    payloads
        .iter()
        .enumerate()
        .map(|(i, payload)| {
            let offset = u64::try_from(i).unwrap_or(u64::MAX);
            let millis = i64::try_from(i).unwrap_or(i64::MAX);
            StoredEvent {
                offset,
                event_id: format!("evt-{i}"),
                turn_id: turn_str.clone(),
                correlation_id: corr_str.clone(),
                timestamp: base_time + chrono::Duration::milliseconds(millis),
                event_type: event_payload_type_name(payload).to_string(),
                payload: payload.clone(),
                execution_version: String::new(),
            }
        })
        .collect()
}
