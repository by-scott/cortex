use std::fmt::Write as _;

use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Message, Payload, PermissionDecision, Role, TurnId};

use crate::attention::ChannelScheduler;
use crate::confidence::ConfidenceTracker;
use crate::context::pressure::{PressureLevel, compute_occupancy, estimate_tokens};
use crate::llm::{LlmClient, LlmRequest};
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::risk::{DenialTracker, PermissionGate, RiskAssessor};
use crate::tools::{ToolRegistry, ToolResult};
use crate::working_memory::WorkingMemoryManager;

use super::dmn::{PressureContext, apply_compress_history};
use super::journal_append;
use super::{
    MAX_AGENT_DEPTH, NullTracer, TraceCategory, TurnConfig, TurnContext, TurnError, TurnTracer,
};

// ── Tool progress reporting ─────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProgressStatus {
    Started,
    Completed,
    Error,
}

#[derive(Debug, Clone)]
pub struct ToolProgress {
    pub tool_name: String,
    pub status: ToolProgressStatus,
    pub message: Option<String>,
}

// ── TPN loop context ────────────────────────────────────────

pub struct TpnLoopContext<'a> {
    pub history: &'a mut Vec<Message>,
    pub llm: &'a dyn LlmClient,
    pub tools: &'a ToolRegistry,
    pub journal: &'a Journal,
    pub gate: &'a dyn PermissionGate,
    pub config: &'a TurnConfig,
    pub on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    pub on_tool_progress: Option<&'a (dyn Fn(&ToolProgress) + Send + Sync)>,
    pub compress_template: Option<&'a String>,
    pub summary_cache: &'a mut crate::context::SummaryCache,
    pub system_prompt: Option<&'a String>,
    pub tool_defs: &'a [serde_json::Value],
    pub working_mem: &'a mut WorkingMemoryManager,
    pub scheduler: &'a mut ChannelScheduler,
    pub confidence: &'a mut ConfidenceTracker,
    pub meta_monitor: &'a mut MetaMonitor,
    pub denial_tracker: &'a mut DenialTracker,
    pub risk_assessor: &'a RiskAssessor,
    pub reasoning_engine: &'a mut ReasoningEngine,
    pub prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    pub skill_registry: Option<&'a crate::skills::SkillRegistry>,
    pub turn_id: TurnId,
    pub corr_id: CorrelationId,
    pub events_log: &'a mut Vec<Payload>,
    pub tracer: &'a dyn TurnTracer,
    /// External cancellation flag — checked at each loop iteration.
    pub cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Shared inbox for mid-turn message injection from external callers.
    pub message_inbox: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
}

// ── Trace helpers ──────────────────────────────────────────

fn trace_llm_result(tracer: &dyn TurnTracer, response: &crate::llm::LlmResponse) {
    tracer.trace_at(
        TraceCategory::Llm,
        cortex_types::TraceLevel::Basic,
        &format!(
            "LLM complete: {}in/{}out tokens, est ${:.4}",
            response.usage.input_tokens,
            response.usage.output_tokens,
            crate::llm::cost::estimate_cost(
                &response.model,
                response.usage.input_tokens,
                response.usage.output_tokens,
            ),
        ),
    );
    tracer.trace_at(
        TraceCategory::Llm,
        cortex_types::TraceLevel::Full,
        &format!(
            "model={}, in={}, out={}, tools={}",
            response.model,
            response.usage.input_tokens,
            response.usage.output_tokens,
            response.tool_calls.len(),
        ),
    );
}

fn trace_tool_start(tracer: &dyn TurnTracer, tool_name: &str, tc_input: &serde_json::Value) {
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Minimal,
        &format!("Tool: {tool_name} (started)"),
    );
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Summary,
        &format!("Tool: {tool_name} args={}", truncate_json(tc_input, 200)),
    );
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Full,
        &format!("Tool: {tool_name} args={tc_input}"),
    );
}

fn trace_tool_finish(tracer: &dyn TurnTracer, tool_name: &str, result: &ToolResult) {
    let status = if result.is_error { "error" } else { "ok" };
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Basic,
        &format!("Tool: {tool_name} ({status})"),
    );
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Debug,
        &format!(
            "Tool: {tool_name} result={}",
            truncate_json_str(&result.output, 1000)
        ),
    );
}

// ── Main loop ───────────────────────────────────────────────

pub async fn run_tpn_loop(ctx: &mut TpnLoopContext<'_>) -> Result<Option<String>, TurnError> {
    let mut final_text: Option<String> = None;
    // Metacognition strategy hint -- injected into system prompt when alerts fire
    let mut meta_hint: Option<String> = None;
    let mut tool_iteration: usize = 0;

    for iteration in 0..ctx.config.max_tool_iterations {
        // Check external cancellation flag before each iteration.
        if ctx
            .cancel_flag
            .as_ref()
            .is_some_and(|f| f.load(std::sync::atomic::Ordering::Relaxed))
        {
            ctx.tracer.trace_at(
                TraceCategory::Phase,
                cortex_types::TraceLevel::Minimal,
                "Turn cancelled by user (/stop)",
            );
            break;
        }

        drain_injected_messages(ctx);
        flush_scheduler_events(
            ctx.scheduler,
            ctx.journal,
            ctx.turn_id,
            ctx.corr_id,
            ctx.events_log,
        );

        super::dmn::handle_context_pressure(&mut PressureContext {
            history: ctx.history,
            working_mem: ctx.working_mem,
            compress_template: ctx.compress_template,
            summary_cache: ctx.summary_cache,
            journal: ctx.journal,
            turn_id: ctx.turn_id,
            corr_id: ctx.corr_id,
            events_log: ctx.events_log,
            llm: ctx.llm,
            max_tokens: ctx.config.max_tokens,
        })
        .await;

        let system_with_extras = build_system_prompt_with_extras(
            ctx.system_prompt,
            ctx.reasoning_engine,
            &mut meta_hint,
        );

        ctx.tracer.trace_at(
            TraceCategory::Llm,
            cortex_types::TraceLevel::Basic,
            &format!("LLM call #{}", iteration + 1),
        );

        let request = LlmRequest {
            system: system_with_extras.as_deref(),
            messages: ctx.history,
            tools: (!ctx.tool_defs.is_empty()).then_some(ctx.tool_defs),
            max_tokens: ctx.config.max_tokens,
            on_text: ctx.on_text,
        };

        let response = ctx
            .llm
            .complete(request)
            .await
            .map_err(|e| TurnError::LlmError(e.to_string()))?;

        trace_llm_result(ctx.tracer, &response);

        record_llm_cost(
            &response,
            ctx.journal,
            ctx.turn_id,
            ctx.corr_id,
            ctx.events_log,
        );
        record_response_events(ctx, &response);

        if response.tool_calls.is_empty() {
            if let Some(text) = &response.text {
                let payload = Payload::AssistantMessage {
                    content: text.clone(),
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
                ctx.events_log.push(payload);
                ctx.history.push(Message::assistant(text));
            }
            final_text = response.text;
            break;
        }

        if process_tool_calls_batch(ctx, &response).await {
            final_text = Some("Multiple tool calls were denied. Please confirm direction.".into());
            break;
        }

        tool_iteration += 1;
        if let Some(early_exit) =
            post_tool_iteration(ctx, &response, tool_iteration, &mut meta_hint).await
        {
            return Ok(early_exit);
        }
    }

    Ok(final_text)
}

// ── Tool call processing ────────────────────────────────────

pub async fn process_tool_calls_batch(
    ctx: &mut TpnLoopContext<'_>,
    response: &crate::llm::LlmResponse,
) -> bool {
    let mut tool_results_for_history: Vec<cortex_types::ContentBlock> = Vec::new();
    let mut assistant_blocks: Vec<cortex_types::ContentBlock> = Vec::new();

    for tc in &response.tool_calls {
        let tool_name = tc.name.clone();
        assistant_blocks.push(cortex_types::ContentBlock::ToolUse {
            id: tc.id.clone(),
            name: tool_name.clone(),
            input: tc.input.clone(),
        });

        let risk_level = ctx.risk_assessor.assess_level_with_depth(
            &tool_name,
            &tc.input,
            ctx.config.agent_depth,
        );
        let perm_payload = Payload::PermissionRequested {
            tool_name: tool_name.clone(),
            risk_level: format!("{risk_level:?}"),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &perm_payload);
        ctx.events_log.push(perm_payload);

        let decision = ctx.gate.check(&tool_name, risk_level);

        let (tool_output, is_error) = match decision {
            PermissionDecision::Approved => {
                let mut tc_ctx = ToolCallContext {
                    journal: ctx.journal,
                    turn_id: ctx.turn_id,
                    corr_id: ctx.corr_id,
                    events_log: ctx.events_log,
                    confidence: ctx.confidence,
                    meta_monitor: ctx.meta_monitor,
                    working_mem: ctx.working_mem,
                    denial_tracker: ctx.denial_tracker,
                    tools: ctx.tools,
                    config: ctx.config,
                    llm: ctx.llm,
                    gate: ctx.gate,
                    history: ctx.history,
                    on_text: ctx.on_text,
                    prompt_manager: ctx.prompt_manager,
                    on_tool_progress: ctx.on_tool_progress,
                    skill_registry: ctx.skill_registry,
                    tracer: ctx.tracer,
                };
                process_approved_tool_call(&mut tc_ctx, &tool_name, &tc.input).await
            }
            PermissionDecision::Denied => handle_denied_tool(
                &tool_name,
                ctx.journal,
                ctx.turn_id,
                ctx.corr_id,
                ctx.events_log,
                ctx.denial_tracker,
                ctx.confidence,
            ),
            PermissionDecision::Pending | PermissionDecision::TimedOut => {
                // Fallback: if we reach here the gate did not resolve interactively.
                // Treat as denied — safe default.
                handle_denied_tool(
                    &tool_name,
                    ctx.journal,
                    ctx.turn_id,
                    ctx.corr_id,
                    ctx.events_log,
                    ctx.denial_tracker,
                    ctx.confidence,
                )
            }
        };

        tool_results_for_history.push(cortex_types::ContentBlock::ToolResult {
            tool_use_id: tc.id.clone(),
            content: tool_output,
            is_error,
        });
    }

    ctx.history.push(Message {
        role: Role::Assistant,
        content: assistant_blocks,
        attachments: Vec::new(),
    });
    ctx.history.push(Message {
        role: Role::User,
        content: tool_results_for_history,
        attachments: Vec::new(),
    });
    ctx.denial_tracker.should_pause()
}

// ── Scheduler events ────────────────────────────────────────

pub fn flush_scheduler_events(
    scheduler: &mut ChannelScheduler,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) {
    let sched_events = scheduler.tick();
    for ev in sched_events {
        journal_append(journal, turn_id, corr_id, &ev);
        events_log.push(ev);
    }
}

// ── System prompt construction ──────────────────────────────

/// Build the system prompt, injecting reasoning context and metacognition hints.
pub fn build_system_prompt_with_extras(
    base_prompt: Option<&String>,
    reasoning_engine: &ReasoningEngine,
    meta_hint: &mut Option<String>,
) -> Option<String> {
    let mut result = reasoning_engine.format_context().map_or_else(
        || base_prompt.cloned(),
        |reasoning_ctx| {
            if let Some(base) = base_prompt {
                Some(format!("{base}\n\n{reasoning_ctx}"))
            } else {
                Some(reasoning_ctx)
            }
        },
    );

    if let Some(hint) = meta_hint.take() {
        result = Some(result.map_or_else(
            || format!("[Metacognition] {hint}"),
            |base| format!("{base}\n\n[Metacognition] {hint}"),
        ));
    }

    result
}

// ── Cost + response events ──────────────────────────────────

pub fn record_llm_cost(
    response: &crate::llm::LlmResponse,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) {
    let cost_payload = Payload::LlmCallCompleted {
        input_tokens: response.usage.input_tokens,
        output_tokens: response.usage.output_tokens,
        model: response.model.clone(),
        estimated_cost_usd: crate::llm::cost::estimate_cost(
            &response.model,
            response.usage.input_tokens,
            response.usage.output_tokens,
        ),
    };
    journal_append(journal, turn_id, corr_id, &cost_payload);
    events_log.push(cost_payload);
}

/// Record the LLM response text as a `SideEffect` event and track any reasoning step.
pub fn record_response_events(ctx: &mut TpnLoopContext<'_>, response: &crate::llm::LlmResponse) {
    if let Some(text) = &response.text {
        let se = Payload::SideEffectRecorded {
            kind: cortex_types::SideEffectKind::LlmResponse,
            key: ctx.turn_id.to_string(),
            value: text.clone(),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &se);
        ctx.events_log.push(se);
    }

    if ctx.reasoning_engine.is_active()
        && let Some(text) = &response.text
    {
        let reasoning_events = ctx.reasoning_engine.track_step(text);
        for ev in reasoning_events {
            journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
            ctx.events_log.push(ev);
        }
    }
}

// ── Post-tool iteration ─────────────────────────────────────

/// Apply strategic compression and metacognition checks after a tool-call batch.
/// Returns `Some(early_exit_text)` when the loop should terminate early.
pub async fn post_tool_iteration(
    ctx: &mut TpnLoopContext<'_>,
    response: &crate::llm::LlmResponse,
    tool_iteration: usize,
    meta_hint: &mut Option<String>,
) -> Option<Option<String>> {
    let has_agent_call = response.tool_calls.iter().any(|tc| tc.name == "agent");
    if has_agent_call || tool_iteration.is_multiple_of(5) {
        apply_compress_history(&mut PressureContext {
            history: ctx.history,
            llm: ctx.llm,
            journal: ctx.journal,
            turn_id: ctx.turn_id,
            corr_id: ctx.corr_id,
            events_log: ctx.events_log,
            working_mem: ctx.working_mem,
            compress_template: ctx.compress_template,
            summary_cache: ctx.summary_cache,
            max_tokens: ctx.config.max_tokens,
        })
        .await;
    }

    apply_metacognition_alerts(ctx, meta_hint);
    apply_exploration_hint(ctx, meta_hint);
    apply_conditional_skills(ctx, meta_hint);
    None
}

// ── Metacognition ───────────────────────────────────────────

/// Handle metacognition alerts after tool execution.
///
/// Check metacognition alerts and apply appropriate responses.
fn apply_metacognition_alerts(ctx: &mut TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let alerts = ctx
        .meta_monitor
        .check_with_confidence(ctx.confidence.score());
    for alert in &alerts {
        ctx.tracer.trace_at(
            TraceCategory::Meta,
            cortex_types::TraceLevel::Basic,
            &format!("Alert: {:?}", alert.kind),
        );
        let action: &'static str = match alert.kind {
            crate::meta::AlertKind::DoomLoop => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-doom-loop"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_DOOM_LOOP.to_string()
                        }),
                );
                "doom_loop_strategy_switch"
            }
            crate::meta::AlertKind::Duration => {
                let payload = Payload::MetaControlApplied {
                    action: "duration_warning".into(),
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
                ctx.events_log.push(payload);
                "duration_warning"
            }
            crate::meta::AlertKind::Fatigue => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-fatigue"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_FATIGUE.to_string()
                        }),
                );
                "fatigue_step_break"
            }
            crate::meta::AlertKind::FrameAnchoring => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-frame-anchoring"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_FRAME_ANCHORING.to_string()
                        }),
                );
                "frame_anchoring_recheck"
            }
            crate::meta::AlertKind::HealthDegraded => "health_degraded_noted",
        };
        let payload = Payload::MetaControlApplied {
            action: action.into(),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
        ctx.events_log.push(payload);
    }

    // Bridge: alert -> skill activation
    if let Some(registry) = ctx.skill_registry {
        let alert_names: Vec<String> = alerts.iter().map(|a| format!("{:?}", a.kind)).collect();
        if !alert_names.is_empty() {
            let activated = registry.activated_skills("", "normal", &alert_names);
            for summary in activated {
                let already = meta_hint
                    .as_ref()
                    .is_some_and(|h| h.contains(&summary.name));
                if already {
                    continue;
                }
                if let Some(content) = registry.with_skill(&summary.name, |s| {
                    let crate::skills::SkillContent::Markdown(c) = s.content("");
                    c
                }) {
                    let skill_section = format!("\n[Skill: {}]\n{}", summary.name, content);
                    match meta_hint {
                        Some(existing) => existing.push_str(&skill_section),
                        None => *meta_hint = Some(skill_section),
                    }
                }
                let ev = Payload::SkillInvoked {
                    name: summary.name.clone(),
                    trigger: cortex_types::InvocationTrigger::MetacognitiveAlert(
                        alert_names.join(","),
                    )
                    .to_string(),
                    execution_mode: "inline".to_string(),
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
                ctx.events_log.push(ev);
            }
        }
    }
}

/// Drain messages injected mid-turn from external callers and append
/// them as user messages to the conversation history.
fn drain_injected_messages(ctx: &mut TpnLoopContext<'_>) {
    let Some(ref inbox) = ctx.message_inbox else {
        return;
    };
    let injected: Vec<String> = std::mem::take(
        &mut *inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    );
    for msg in injected {
        ctx.tracer.trace_at(
            TraceCategory::Phase,
            cortex_types::TraceLevel::Minimal,
            "Injected mid-turn user message",
        );
        ctx.history.push(cortex_types::Message::user(&msg));
    }
}

/// Check RPE exploration candidates and inject hint when uncertainty is high.
///
/// Emits `ExplorationTriggered` for the top candidate and, if no urgent
/// metacognition hint is active, injects a suggestion into the system prompt.
fn apply_exploration_hint(ctx: &mut TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let candidates = ctx.meta_monitor.rpe.exploration_candidates();
    if candidates.is_empty() {
        return;
    }

    // Emit event for the top candidate
    let (top_name, top_bonus) = &candidates[0];
    let ev = Payload::ExplorationTriggered {
        tool_name: top_name.clone(),
        bonus: *top_bonus,
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
    ctx.events_log.push(ev);

    // Inject hint only when no urgent metacognition hint is already set
    if meta_hint.is_some() {
        return;
    }

    let template = ctx
        .prompt_manager
        .and_then(|pm| pm.get_system_template("hint-exploration"))
        .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_HINT_EXPLORATION.to_string());

    let display: Vec<String> = candidates
        .iter()
        .take(3)
        .map(|(name, bonus)| format!("'{name}' (uncertainty bonus={bonus:.2})"))
        .collect();
    let hint = template.replace("__CANDIDATES__", &display.join(", "));
    *meta_hint = Some(hint);
}

fn apply_conditional_skills(ctx: &TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let Some(registry) = ctx.skill_registry else {
        return;
    };
    let input = ctx
        .history
        .last()
        .map(Message::text_content)
        .unwrap_or_default();
    let used: usize = ctx
        .history
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum();
    let occupancy = compute_occupancy(used, ctx.config.max_tokens);
    let pressure = PressureLevel::from_occupancy(occupancy, &ctx.config.pressure_thresholds);
    let pressure_name = pressure.name();
    // Gather current alert kinds from recent metacognition
    let alerts = ctx
        .meta_monitor
        .check_with_confidence(ctx.confidence.score());
    let alert_names: Vec<String> = alerts.iter().map(|a| format!("{:?}", a.kind)).collect();

    let activated = registry.activated_skills(&input, pressure_name, &alert_names);
    if activated.is_empty() {
        return;
    }
    let mut skill_text = String::from("[Auto-activated skills]\n");
    for summary in &activated {
        if let Some(content) = registry.with_skill(&summary.name, |s| {
            let crate::skills::SkillContent::Markdown(c) = s.content("");
            c
        }) {
            let _ = writeln!(skill_text, "\n## {}\n{}", summary.name, content);
        }
    }
    match meta_hint {
        Some(existing) => {
            existing.push('\n');
            existing.push_str(&skill_text);
        }
        None => *meta_hint = Some(skill_text),
    }
}

// ── Trace helpers ──────────────────────────────────────────

/// Truncate a JSON value's string representation to at most `max_len` characters.
fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = value.to_string();
    truncate_json_str(&s, max_len)
}

/// Truncate a string to at most `max_len` characters, appending "..." if truncated.
fn truncate_json_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        return s.to_string();
    }
    let mut end = max_len.min(s.len());
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...", &s[..end])
}

// ── Tool dispatch ───────────────────────────────────────────

struct ToolCallContext<'a> {
    journal: &'a Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &'a mut Vec<Payload>,
    confidence: &'a mut ConfidenceTracker,
    meta_monitor: &'a mut MetaMonitor,
    working_mem: &'a mut WorkingMemoryManager,
    denial_tracker: &'a mut DenialTracker,
    tools: &'a ToolRegistry,
    config: &'a TurnConfig,
    llm: &'a dyn LlmClient,
    gate: &'a dyn PermissionGate,
    history: &'a [Message],
    on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    on_tool_progress: Option<&'a (dyn Fn(&ToolProgress) + Send + Sync)>,
    skill_registry: Option<&'a crate::skills::SkillRegistry>,
    tracer: &'a dyn TurnTracer,
}

async fn dispatch_tool_call(
    tc_ctx: &ToolCallContext<'_>,
    tool_name: &str,
    tc_input: &serde_json::Value,
) -> ToolResult {
    if tool_name == "agent" {
        execute_agent_sub_turn(AgentSubTurnParams {
            input: tc_input,
            parent_config: tc_ctx.config,
            llm: tc_ctx.llm,
            journal: tc_ctx.journal,
            gate: tc_ctx.gate,
            parent_history: tc_ctx.history,
            on_text: tc_ctx.on_text,
            prompt_manager: tc_ctx.prompt_manager,
        })
        .await
    } else if tool_name == "skill" && should_fork_skill(tc_ctx.skill_registry, tc_input) {
        let Some(registry) = tc_ctx.skill_registry else {
            return ToolResult::error("skill_registry not available for fork execution");
        };
        execute_skill_sub_turn(SkillSubTurnParams {
            input: tc_input,
            skill_registry: registry,
            parent_config: tc_ctx.config,
            llm: tc_ctx.llm,
            journal: tc_ctx.journal,
            gate: tc_ctx.gate,
            on_text: tc_ctx.on_text,
        })
        .await
    } else if tool_name == "skill" {
        dispatch_inline_skill(tc_ctx, tc_input)
    } else {
        execute_tool(
            tc_ctx.tools,
            tool_name,
            tc_input,
            tc_ctx.config.tool_timeout_secs,
        )
    }
}

fn dispatch_inline_skill(tc_ctx: &ToolCallContext<'_>, tc_input: &serde_json::Value) -> ToolResult {
    let skill_name = tc_input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .trim_start_matches('/');
    let invoke_ev = Payload::SkillInvoked {
        name: skill_name.to_string(),
        trigger: cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
        execution_mode: "inline".to_string(),
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &invoke_ev);
    let start = std::time::Instant::now();
    let result = execute_tool(
        tc_ctx.tools,
        "skill",
        tc_input,
        tc_ctx.config.tool_timeout_secs,
    );
    let duration_ms = start.elapsed().as_millis();
    let complete_ev = Payload::SkillCompleted {
        name: skill_name.to_string(),
        success: !result.is_error,
        duration_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &complete_ev);
    if let Some(reg) = tc_ctx.skill_registry {
        reg.record_outcome(skill_name, !result.is_error);
    }
    result
}

/// Record permission-granted and tool-invocation-intent events.
fn record_tool_approval(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    tc_input: &serde_json::Value,
) {
    let grant_payload = Payload::PermissionGranted {
        tool_name: tool_name.to_string(),
    };
    journal_append(
        tc_ctx.journal,
        tc_ctx.turn_id,
        tc_ctx.corr_id,
        &grant_payload,
    );
    tc_ctx.events_log.push(grant_payload);

    let intent_payload = Payload::ToolInvocationIntent {
        tool_name: tool_name.to_string(),
        input: tc_input.to_string(),
    };
    journal_append(
        tc_ctx.journal,
        tc_ctx.turn_id,
        tc_ctx.corr_id,
        &intent_payload,
    );
    tc_ctx.events_log.push(intent_payload);
}

async fn process_approved_tool_call(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    tc_input: &serde_json::Value,
) -> (String, bool) {
    tc_ctx.denial_tracker.record_approval();

    record_tool_approval(tc_ctx, tool_name, tc_input);

    if let Some(cb) = tc_ctx.on_tool_progress {
        cb(&ToolProgress {
            tool_name: tool_name.to_string(),
            status: ToolProgressStatus::Started,
            message: None,
        });
    }

    trace_tool_start(tc_ctx.tracer, tool_name, tc_input);

    let result = dispatch_tool_call(tc_ctx, tool_name, tc_input).await;

    trace_tool_finish(tc_ctx.tracer, tool_name, &result);

    if let Some(cb) = tc_ctx.on_tool_progress {
        cb(&ToolProgress {
            tool_name: tool_name.to_string(),
            status: if result.is_error {
                ToolProgressStatus::Error
            } else {
                ToolProgressStatus::Completed
            },
            message: if result.is_error {
                Some(result.output.clone())
            } else {
                None
            },
        });
    }

    let result_payload = Payload::ToolInvocationResult {
        tool_name: tool_name.to_string(),
        output: result.output.clone(),
        is_error: result.is_error,
    };
    journal_append(
        tc_ctx.journal,
        tc_ctx.turn_id,
        tc_ctx.corr_id,
        &result_payload,
    );
    tc_ctx.events_log.push(result_payload);

    tc_ctx
        .meta_monitor
        .record_tool_call(tool_name, &tc_input.to_string());
    if let Some(reg) = tc_ctx.skill_registry {
        reg.record_tool_call(tool_name);
    }
    if result.is_error {
        tc_ctx.confidence.record_failure();
        tc_ctx
            .meta_monitor
            .record_tool_result(tool_name, false, &result.output);
    } else {
        let wm_events = tc_ctx.working_mem.rehearse(tool_name);
        for ev in wm_events {
            journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &ev);
            tc_ctx.events_log.push(ev);
        }
        tc_ctx.confidence.record_success();
        tc_ctx
            .meta_monitor
            .record_tool_result(tool_name, true, &result.output);
    }

    // Record ExternalIo side-effect for non-deterministic tools (replay support)
    if matches!(tool_name, "bash") {
        let truncated = if result.output.len() > 4096 {
            let mut end = 4093;
            while end > 0 && !result.output.is_char_boundary(end) {
                end -= 1;
            }
            format!("{}…", &result.output[..end])
        } else {
            result.output.clone()
        };
        let se = Payload::SideEffectRecorded {
            kind: cortex_types::SideEffectKind::ExternalIo,
            key: tool_name.to_string(),
            value: truncated,
        };
        journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &se);
        tc_ctx.events_log.push(se);
    }

    (result.output, result.is_error)
}

fn handle_denied_tool(
    tool_name: &str,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
    denial_tracker: &mut DenialTracker,
    confidence: &mut ConfidenceTracker,
) -> (String, bool) {
    denial_tracker.record_denial();
    confidence.record_denial();
    let deny_payload = Payload::PermissionDenied {
        tool_name: tool_name.to_string(),
        reason: "blocked by permission gate".into(),
    };
    journal_append(journal, turn_id, corr_id, &deny_payload);
    events_log.push(deny_payload);
    ("permission denied".to_string(), true)
}

/// Execute a tool with timeout enforcement.
///
/// Measures execution time against the configured timeout. If a tool exceeds
/// the limit, the result is replaced with a timeout error. Note: synchronous
/// tool code cannot be preemptively cancelled in Rust -- the timeout is checked
/// post-execution. For tools that may truly hang (e.g., bash), the tool itself
/// should implement internal timeout (bash already uses process timeouts).
fn execute_tool(
    tools: &ToolRegistry,
    name: &str,
    input: &serde_json::Value,
    global_timeout_secs: u64,
) -> ToolResult {
    let Some(tool) = tools.get(name) else {
        return ToolResult::error(format!("unknown tool: {name}"));
    };

    let timeout_secs = tool.timeout_secs().unwrap_or(global_timeout_secs);
    let input_clone = input.clone();
    let start = std::time::Instant::now();

    // Execute tool in a scoped OS thread to avoid blocking the tokio runtime.
    // Scoped threads can borrow `tool` (&dyn Tool) safely.
    let result = std::thread::scope(|s| {
        let handle = s.spawn(|| match tool.execute(input_clone) {
            Ok(r) => r,
            Err(e) => ToolResult::error(format!("tool error: {e}")),
        });
        handle
            .join()
            .unwrap_or_else(|_| ToolResult::error(format!("tool '{name}' panicked")))
    });

    let elapsed = start.elapsed();
    if elapsed.as_secs() > timeout_secs {
        return ToolResult::error(format!(
            "tool '{name}' exceeded timeout ({timeout_secs}s, took {:.1}s)",
            elapsed.as_secs_f64()
        ));
    }

    result
}

// ── Agent sub-turn ──────────────────────────────────────────

struct AgentSubTurnParams<'a> {
    input: &'a serde_json::Value,
    parent_config: &'a TurnConfig,
    llm: &'a dyn LlmClient,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    parent_history: &'a [Message],
    on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
}

async fn execute_agent_sub_turn(params: AgentSubTurnParams<'_>) -> ToolResult {
    let AgentSubTurnParams {
        input,
        parent_config,
        llm,
        journal,
        gate,
        parent_history,
        on_text,
        prompt_manager,
    } = params;
    // Parse agent parameters
    let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) else {
        return ToolResult::error("agent: missing prompt");
    };

    let mode_str = input
        .get("mode")
        .and_then(|v| v.as_str())
        .unwrap_or("readonly");

    let description = input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("sub-agent");

    // Check recursion depth
    if parent_config.agent_depth >= MAX_AGENT_DEPTH {
        return ToolResult::error(format!(
            "agent '{description}': max recursion depth ({MAX_AGENT_DEPTH}) exceeded"
        ));
    }

    // Build sub-Turn tool registry based on mode
    let sub_tools = build_sub_tool_registry(mode_str, parent_config.agent_depth);

    let sub_config = build_sub_turn_config(input, mode_str, parent_config, prompt_manager);

    // Build sub-Turn history
    let mut sub_history = if mode_str == "fork" {
        parent_history.to_vec()
    } else {
        Vec::new()
    };

    // Execute sub-Turn
    let sub_ctx = TurnContext {
        input: prompt,
        history: &mut sub_history,
        llm,
        tools: &sub_tools,
        journal,
        gate,
        config: &sub_config,
        on_text,
        on_tool_progress: None,
        images: vec![],
        compress_template: None,
        summary_cache: None,
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        cancel_flag: None,
        message_inbox: None,
    };

    match super::run_turn(sub_ctx).await {
        Ok(result) => result.response_text.map_or_else(
            || {
                ToolResult::success(format!(
                    "[Agent '{description}' ({mode_str} mode)] completed with no text response"
                ))
            },
            ToolResult::success,
        ),
        Err(e) => ToolResult::error(format!("agent '{description}' failed: {e}")),
    }
}

fn build_sub_turn_config(
    input: &serde_json::Value,
    mode_str: &str,
    parent_config: &TurnConfig,
    prompt_manager: Option<&cortex_kernel::PromptManager>,
) -> TurnConfig {
    let team_name = input
        .get("team_name")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    let system_prompt = match mode_str {
        "fork" => parent_config.system_prompt.clone(),
        "teammate" => {
            const TEAM_PLACEHOLDER: &str = "{team}";
            let template = prompt_manager
                .and_then(|pm| pm.get_system_template("agent-teammate"))
                .unwrap_or_else(|| {
                    cortex_kernel::prompt_manager::DEFAULT_AGENT_TEAMMATE.to_string()
                });
            Some(template.replace(TEAM_PLACEHOLDER, team_name))
        }
        _ => None,
    };

    TurnConfig {
        system_prompt,
        max_tokens: parent_config.max_tokens,
        agent_depth: parent_config.agent_depth + 1,
        working_memory_capacity: parent_config.working_memory_capacity,
        max_tool_iterations: parent_config.max_tool_iterations,
        auto_extract: false,
        extract_min_turns: parent_config.extract_min_turns,
        turns_since_extract: 0,
        tool_timeout_secs: parent_config.tool_timeout_secs,
        strip_think_tags: parent_config.strip_think_tags,
        evolution_weights: parent_config.evolution_weights,
        pressure_thresholds: parent_config.pressure_thresholds,
        metacognition: parent_config.metacognition.clone(),
        trace: parent_config.trace.clone(),
    }
}

fn build_sub_tool_registry(mode: &str, current_depth: usize) -> ToolRegistry {
    let allow_nested_agent = current_depth + 1 < MAX_AGENT_DEPTH;

    let mut registry = ToolRegistry::new();
    registry.register(Box::new(crate::tools::read::ReadTool));
    registry.register(Box::new(crate::tools::write::WriteTool));
    registry.register(Box::new(crate::tools::edit::EditTool));
    registry.register(Box::new(crate::tools::bash::BashTool));

    // Only add agent tool in non-readonly modes when depth allows it
    if mode != "readonly" && allow_nested_agent {
        registry.register(Box::new(crate::tools::agent::AgentTool));
    }

    registry
}

// ── Skill fork ──────────────────────────────────────────────

fn should_fork_skill(
    registry: Option<&crate::skills::SkillRegistry>,
    input: &serde_json::Value,
) -> bool {
    let Some(registry) = registry else {
        return false;
    };
    let Some(name) = input.get("skill").and_then(|v| v.as_str()) else {
        return false;
    };
    let name = name.trim().trim_start_matches('/');
    registry
        .with_skill(name, |s| {
            s.execution_mode() == cortex_types::ExecutionMode::Fork
        })
        .unwrap_or(false)
}

struct SkillSubTurnParams<'a> {
    input: &'a serde_json::Value,
    skill_registry: &'a crate::skills::SkillRegistry,
    parent_config: &'a TurnConfig,
    llm: &'a dyn LlmClient,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
}

async fn execute_skill_sub_turn(params: SkillSubTurnParams<'_>) -> ToolResult {
    let SkillSubTurnParams {
        input,
        skill_registry,
        parent_config,
        llm,
        journal,
        gate,
        on_text,
    } = params;

    let skill_name = input
        .get("skill")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .trim()
        .trim_start_matches('/');
    let args = input.get("args").and_then(|v| v.as_str()).unwrap_or("");

    let Some(content) = skill_registry.with_skill(skill_name, |s| {
        let crate::skills::SkillContent::Markdown(c) = s.content(args);
        c
    }) else {
        return ToolResult::error(format!("skill fork: unknown skill '{skill_name}'"));
    };

    if parent_config.agent_depth >= MAX_AGENT_DEPTH {
        return ToolResult::error(format!(
            "skill fork '{skill_name}': max depth ({MAX_AGENT_DEPTH}) exceeded"
        ));
    }
    let turn_id = TurnId::new();
    let corr_id = CorrelationId::new();
    let invoke_ev = Payload::SkillInvoked {
        name: skill_name.to_string(),
        trigger: cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
        execution_mode: "fork".to_string(),
    };
    journal_append(journal, turn_id, corr_id, &invoke_ev);
    let start = std::time::Instant::now();

    let sub_tools = build_sub_tool_registry("full", parent_config.agent_depth);
    let sub_config = TurnConfig {
        system_prompt: None,
        max_tokens: parent_config.max_tokens,
        agent_depth: parent_config.agent_depth + 1,
        working_memory_capacity: parent_config.working_memory_capacity,
        max_tool_iterations: parent_config.max_tool_iterations,
        auto_extract: false,
        extract_min_turns: parent_config.extract_min_turns,
        turns_since_extract: 0,
        tool_timeout_secs: parent_config.tool_timeout_secs,
        strip_think_tags: parent_config.strip_think_tags,
        evolution_weights: parent_config.evolution_weights,
        pressure_thresholds: parent_config.pressure_thresholds,
        metacognition: parent_config.metacognition.clone(),
        trace: parent_config.trace.clone(),
    };

    let mut sub_history = Vec::new();
    let sub_ctx = TurnContext {
        input: &content,
        history: &mut sub_history,
        llm,
        tools: &sub_tools,
        journal,
        gate,
        config: &sub_config,
        on_text,
        on_tool_progress: None,
        images: vec![],
        compress_template: None,
        summary_cache: None,
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        cancel_flag: None,
        message_inbox: None,
    };

    let result = match super::run_turn(sub_ctx).await {
        Ok(r) => r.response_text.map_or_else(
            || ToolResult::success(format!("[Skill '{skill_name}' (fork)] completed")),
            ToolResult::success,
        ),
        Err(e) => ToolResult::error(format!("skill fork '{skill_name}' failed: {e}")),
    };
    let duration_ms = start.elapsed().as_millis();
    let complete_ev = Payload::SkillCompleted {
        name: skill_name.to_string(),
        success: !result.is_error,
        duration_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
    };
    journal_append(journal, turn_id, corr_id, &complete_ev);
    skill_registry.record_outcome(skill_name, !result.is_error);
    result
}
