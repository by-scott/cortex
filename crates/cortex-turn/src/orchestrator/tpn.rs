use std::fmt::Write as _;

use cortex_kernel::Journal;
use cortex_types::{Attachment, CorrelationId, Message, Payload, PermissionDecision, Role, TurnId};

use crate::attention::ChannelScheduler;
use crate::confidence::ConfidenceTracker;
use crate::context::pressure::{PressureLevel, compute_occupancy, estimate_tokens};
use crate::llm::{LlmClient, LlmError, LlmRequest, LlmResponse};
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::risk::{DenialTracker, PermissionGate, RiskAssessor};
use crate::tools::{ToolRegistry, ToolResult};
use crate::working_memory::WorkingMemoryManager;

use super::dmn::{PressureContext, apply_compress_history};
use super::journal_append;
use super::{
    MAX_AGENT_DEPTH, NullTracer, StreamLane, TraceCategory, TurnConfig, TurnContext, TurnControl,
    TurnControlBoundary, TurnControlCheckpoint, TurnError, TurnStreamBoundary, TurnStreamEvent,
    TurnTracer, dispatch_turn_control,
};

// ── Tool progress reporting ─────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolProgressStatus {
    Started,
    Running,
    Completed,
    Error,
}

// ── TPN loop context ────────────────────────────────────────

pub struct TpnLoopContext<'a> {
    pub history: &'a mut Vec<Message>,
    pub llm: &'a dyn LlmClient,
    pub vision_llm: Option<&'a dyn LlmClient>,
    pub tools: &'a ToolRegistry,
    pub journal: &'a Journal,
    pub gate: &'a dyn PermissionGate,
    pub config: &'a TurnConfig,
    pub on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
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
    pub response_media: &'a mut Vec<Attachment>,
    pub tracer: &'a dyn TurnTracer,
    /// Shared turn runtime control plane.
    pub control: Option<TurnControl>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolProgress {
    pub tool_name: String,
    pub status: ToolProgressStatus,
    pub message: Option<String>,
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

fn is_recoverable_llm_error(error: &LlmError) -> bool {
    let text = error.to_string().to_ascii_lowercase();
    [
        "context",
        "too many tokens",
        "maximum context",
        "context_length",
        "messages parameter is illegal",
        "invalid messages",
        "tool_use ids",
        "tool result",
    ]
    .iter()
    .any(|needle| text.contains(needle))
}

async fn compress_history_for_retry(ctx: &mut TpnLoopContext<'_>, llm: &dyn LlmClient) {
    apply_compress_history(&mut PressureContext {
        history: ctx.history,
        working_mem: ctx.working_mem,
        compress_template: ctx.compress_template,
        summary_cache: ctx.summary_cache,
        journal: ctx.journal,
        turn_id: ctx.turn_id,
        corr_id: ctx.corr_id,
        events_log: ctx.events_log,
        llm,
        max_tokens: ctx.config.max_tokens,
        pressure_thresholds: ctx.config.pressure_thresholds,
    })
    .await;
}

fn trace_tool_start(tracer: &dyn TurnTracer, tool_name: &str, tc_input: &serde_json::Value) {
    tracer.trace_at(
        TraceCategory::Tool,
        cortex_types::TraceLevel::Debug,
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
        cortex_types::TraceLevel::Debug,
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

fn emit_text_event(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    lane: StreamLane,
    source: Option<&str>,
    content: &str,
) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::Text {
            lane,
            source: source.map(str::to_string),
            content: content.to_string(),
        });
    }
}

fn emit_tool_progress(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    progress: ToolProgress,
) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::ToolProgress(progress));
    }
}

fn emit_restart_boundary_event(on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>) {
    if let Some(cb) = on_event {
        cb(&TurnStreamEvent::Boundary(TurnStreamBoundary::Restart));
    }
}

fn handle_iteration_boundary_control(ctx: &mut TpnLoopContext<'_>) -> bool {
    match dispatch_turn_control(
        ctx.control.as_ref(),
        ctx.history,
        ctx.tracer,
        TurnControlCheckpoint::IterationBoundary,
    ) {
        TurnControlBoundary::Continue => false,
        TurnControlBoundary::RestartTurn => {
            emit_restart_boundary_event(ctx.on_event);
            false
        }
        TurnControlBoundary::AbortTurn => true,
    }
}

fn handle_pre_final_response_control(ctx: &mut TpnLoopContext<'_>) -> TurnControlBoundary {
    let boundary = dispatch_turn_control(
        ctx.control.as_ref(),
        ctx.history,
        ctx.tracer,
        TurnControlCheckpoint::IterationBoundary,
    );
    if matches!(boundary, TurnControlBoundary::RestartTurn) {
        emit_restart_boundary_event(ctx.on_event);
    }
    boundary
}

fn handle_response_without_tools(
    ctx: &mut TpnLoopContext<'_>,
    response: crate::llm::LlmResponse,
    final_text: &mut Option<String>,
    aborted: &mut bool,
) -> bool {
    match handle_pre_final_response_control(ctx) {
        TurnControlBoundary::Continue => {
            *final_text = handle_final_response(ctx, response);
            true
        }
        TurnControlBoundary::RestartTurn => false,
        TurnControlBoundary::AbortTurn => {
            *aborted = true;
            true
        }
    }
}

fn handle_final_response(
    ctx: &mut TpnLoopContext<'_>,
    response: crate::llm::LlmResponse,
) -> Option<String> {
    if let Some(text) = &response.text {
        let payload = Payload::AssistantMessage {
            content: text.clone(),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
        ctx.events_log.push(payload);
        ctx.history.push(Message::assistant(text));
    }
    response.text
}

fn record_assistant_text(
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
    text: &str,
) {
    let payload = Payload::AssistantMessage {
        content: text.to_string(),
    };
    journal_append(journal, turn_id, corr_id, &payload);
    events_log.push(payload);
}

fn record_successful_llm_response(
    ctx: &mut TpnLoopContext<'_>,
    response: &LlmResponse,
    has_images_for_request: bool,
) {
    trace_llm_result(ctx.tracer, response);
    record_llm_cost(
        response,
        ctx.journal,
        ctx.turn_id,
        ctx.corr_id,
        ctx.events_log,
    );
    record_response_events(ctx, response);

    if has_images_for_request {
        crate::llm::sanitize_history_for_text_only_turn(ctx.history);
    }
}

fn flush_scheduler_events_for_turn(ctx: &mut TpnLoopContext<'_>) {
    flush_scheduler_events(
        ctx.scheduler,
        ctx.journal,
        ctx.turn_id,
        ctx.corr_id,
        ctx.events_log,
    );
}

// ── Main loop ───────────────────────────────────────────────

pub async fn run_tpn_loop(ctx: &mut TpnLoopContext<'_>) -> Result<Option<String>, TurnError> {
    let mut final_text: Option<String> = None;
    // Metacognition strategy hint -- injected into system prompt when alerts fire
    let mut meta_hint: Option<String> = None;
    let mut tool_iteration: usize = 0;
    let mut aborted = false;

    for iteration in 0..ctx.config.max_tool_iterations {
        if handle_iteration_boundary_control(ctx) {
            aborted = true;
            break;
        }
        flush_scheduler_events_for_turn(ctx);

        let (active_llm, has_images_for_request) =
            select_active_llm(ctx.history, ctx.llm, ctx.vision_llm);

        super::dmn::handle_context_pressure(&mut PressureContext {
            history: ctx.history,
            working_mem: ctx.working_mem,
            compress_template: ctx.compress_template,
            summary_cache: ctx.summary_cache,
            journal: ctx.journal,
            turn_id: ctx.turn_id,
            corr_id: ctx.corr_id,
            events_log: ctx.events_log,
            llm: active_llm,
            max_tokens: ctx.config.max_tokens,
            pressure_thresholds: ctx.config.pressure_thresholds,
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

        let on_event = ctx.on_event;
        let main_text_emitter = |text: &str| {
            emit_text_event(on_event, StreamLane::UserVisible, None, text);
        };

        let request = build_llm_request(
            ctx,
            active_llm,
            system_with_extras.as_deref(),
            &main_text_emitter,
        );

        let mut llm_result = active_llm.complete(request).await;
        if let Err(error) = &llm_result
            && is_recoverable_llm_error(error)
        {
            ctx.tracer.trace_at(
                TraceCategory::Llm,
                cortex_types::TraceLevel::Basic,
                &format!("LLM request failed with recoverable error; compacting and retrying once: {error}"),
            );
            compress_history_for_retry(ctx, active_llm).await;
            let retry_request = build_llm_request(
                ctx,
                active_llm,
                system_with_extras.as_deref(),
                &main_text_emitter,
            );
            llm_result = active_llm.complete(retry_request).await;
        }

        let response = handle_llm_result(llm_result, ctx.history, has_images_for_request)?;

        record_successful_llm_response(ctx, &response, has_images_for_request);

        if response.tool_calls.is_empty() {
            if handle_response_without_tools(ctx, response, &mut final_text, &mut aborted) {
                break;
            }
            continue;
        }

        match process_tool_calls_batch(ctx, &response).await {
            ToolBatchControl::Continue => {}
            ToolBatchControl::PauseForDenials => {
                final_text =
                    Some("Multiple tool calls were denied. Please confirm direction.".into());
                break;
            }
            ToolBatchControl::RestartTurn => continue,
            ToolBatchControl::AbortTurn => {
                aborted = true;
                break;
            }
        }

        tool_iteration += 1;
        if let Some(early_exit) =
            post_tool_iteration(ctx, &response, tool_iteration, &mut meta_hint).await
        {
            return Ok(early_exit);
        }
    }

    ensure_final_response_exists(final_text.is_some(), tool_iteration, aborted)?;
    Ok(final_text)
}

fn ensure_final_response_exists(
    has_final_text: bool,
    tool_iteration: usize,
    aborted: bool,
) -> Result<(), TurnError> {
    if !has_final_text && tool_iteration > 0 && !aborted {
        return Err(TurnError::LlmError(
            "turn ended without a final assistant response after tool execution".into(),
        ));
    }
    Ok(())
}

fn select_active_llm<'a>(
    history: &[Message],
    llm: &'a dyn LlmClient,
    vision_llm: Option<&'a dyn LlmClient>,
) -> (&'a dyn LlmClient, bool) {
    let has_images_for_request = history.iter().any(cortex_types::Message::has_images);
    let active_llm = if has_images_for_request {
        vision_llm.unwrap_or(llm)
    } else {
        llm
    };
    (active_llm, has_images_for_request)
}

fn handle_llm_result(
    result: Result<LlmResponse, LlmError>,
    history: &mut Vec<Message>,
    has_images_for_request: bool,
) -> Result<LlmResponse, TurnError> {
    match result {
        Ok(response) => Ok(response),
        Err(e) => {
            if has_images_for_request {
                crate::llm::sanitize_history_for_text_only_turn(history);
            }
            Err(TurnError::LlmError(e.to_string()))
        }
    }
}

fn build_llm_request<'a>(
    ctx: &'a TpnLoopContext<'a>,
    llm: &'a dyn LlmClient,
    system: Option<&'a str>,
    main_text_emitter: &'a (dyn Fn(&str) + Send + Sync),
) -> LlmRequest<'a> {
    let can_use_tools = !ctx.tool_defs.is_empty()
        && (!ctx.history.iter().any(cortex_types::Message::has_images)
            || llm.supports_tools_with_images());
    LlmRequest {
        system,
        messages: ctx.history,
        tools: can_use_tools.then_some(ctx.tool_defs),
        max_tokens: ctx.config.max_tokens,
        transient_retries: ctx.config.llm_transient_retries,
        on_text: ctx.on_event.map(|_| main_text_emitter),
    }
}

// ── Tool call processing ────────────────────────────────────

enum ToolBatchControl {
    Continue,
    PauseForDenials,
    RestartTurn,
    AbortTurn,
}

fn poll_turn_control_boundary(ctx: &mut TpnLoopContext<'_>) -> TurnControlBoundary {
    let boundary = dispatch_turn_control(
        ctx.control.as_ref(),
        ctx.history,
        ctx.tracer,
        TurnControlCheckpoint::ToolBatchBoundary,
    );
    if matches!(boundary, TurnControlBoundary::RestartTurn) {
        emit_restart_boundary_event(ctx.on_event);
    }
    boundary
}

fn finalize_tool_batch(
    ctx: &mut TpnLoopContext<'_>,
    assistant_blocks: Vec<cortex_types::ContentBlock>,
    tool_results_for_history: Vec<cortex_types::ContentBlock>,
    control_flow: ToolBatchControl,
) -> ToolBatchControl {
    if !assistant_blocks.is_empty() {
        ctx.history.push(Message {
            role: Role::Assistant,
            content: assistant_blocks,
            attachments: Vec::new(),
        });
    }
    if !tool_results_for_history.is_empty() {
        ctx.history.push(Message {
            role: Role::User,
            content: tool_results_for_history,
            attachments: Vec::new(),
        });
    }

    if matches!(control_flow, ToolBatchControl::Continue) && ctx.denial_tracker.should_pause() {
        ToolBatchControl::PauseForDenials
    } else {
        control_flow
    }
}

fn build_tool_call_context<'a>(ctx: &'a mut TpnLoopContext<'_>) -> ToolCallContext<'a> {
    ToolCallContext {
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
        on_event: ctx.on_event,
        prompt_manager: ctx.prompt_manager,
        skill_registry: ctx.skill_registry,
        tracer: ctx.tracer,
        control: ctx.control.clone(),
    }
}

async fn process_tool_calls_batch(
    ctx: &mut TpnLoopContext<'_>,
    response: &crate::llm::LlmResponse,
) -> ToolBatchControl {
    let mut tool_results_for_history: Vec<cortex_types::ContentBlock> = Vec::new();
    let mut assistant_blocks: Vec<cortex_types::ContentBlock> = Vec::new();
    let mut control_flow = ToolBatchControl::Continue;

    if let Some(text) = &response.text
        && !text.trim().is_empty()
    {
        record_assistant_text(ctx.journal, ctx.turn_id, ctx.corr_id, ctx.events_log, text);
        assistant_blocks.push(cortex_types::ContentBlock::Text { text: text.clone() });
    }

    for tc in &response.tool_calls {
        match poll_turn_control_boundary(ctx) {
            TurnControlBoundary::Continue => {}
            TurnControlBoundary::RestartTurn => {
                control_flow = ToolBatchControl::RestartTurn;
                break;
            }
            TurnControlBoundary::AbortTurn => {
                control_flow = ToolBatchControl::AbortTurn;
                break;
            }
        }

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

        let result = match decision {
            PermissionDecision::Approved => {
                let mut tc_ctx = build_tool_call_context(ctx);
                process_approved_tool_call(&mut tc_ctx, &tool_name, &tc.input).await
            }
            PermissionDecision::Denied => {
                let (output, is_error) = handle_denied_tool(
                    &tool_name,
                    ctx.journal,
                    ctx.turn_id,
                    ctx.corr_id,
                    ctx.events_log,
                    ctx.denial_tracker,
                    ctx.confidence,
                );
                ToolResult {
                    output,
                    media: Vec::new(),
                    is_error,
                }
            }
            PermissionDecision::Pending | PermissionDecision::TimedOut => {
                // Fallback: if we reach here the gate did not resolve interactively.
                // Treat as denied — safe default.
                let (output, is_error) = handle_denied_tool(
                    &tool_name,
                    ctx.journal,
                    ctx.turn_id,
                    ctx.corr_id,
                    ctx.events_log,
                    ctx.denial_tracker,
                    ctx.confidence,
                );
                ToolResult {
                    output,
                    media: Vec::new(),
                    is_error,
                }
            }
        };
        let tool_output = result.output;
        let is_error = result.is_error;
        if !is_error {
            ctx.response_media
                .extend(result.media.into_iter().map(sdk_attachment_to_core));
        }

        tool_results_for_history.push(cortex_types::ContentBlock::ToolResult {
            tool_use_id: tc.id.clone(),
            content: tool_output,
            is_error,
        });
    }

    finalize_tool_batch(
        ctx,
        assistant_blocks,
        tool_results_for_history,
        control_flow,
    )
}

fn sdk_attachment_to_core(attachment: cortex_sdk::Attachment) -> Attachment {
    Attachment {
        media_type: attachment.media_type,
        mime_type: attachment.mime_type,
        url: attachment.url,
        caption: attachment.caption,
        size: attachment.size,
    }
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
    _response: &crate::llm::LlmResponse,
    _tool_iteration: usize,
    meta_hint: &mut Option<String>,
) -> Option<Option<String>> {
    let used: usize = ctx
        .history
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum();
    let pressure = PressureLevel::from_occupancy(
        compute_occupancy(used, ctx.config.max_tokens),
        &ctx.config.pressure_thresholds,
    );
    if pressure >= PressureLevel::Compress {
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
            pressure_thresholds: ctx.config.pressure_thresholds,
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
                if let Some(content) = registry.render(&summary.name, "").map(|rendered| {
                    let crate::skills::SkillContent::Markdown(content) = rendered.content;
                    content
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
        if let Some(content) = registry.render(&summary.name, "").map(|rendered| {
            let crate::skills::SkillContent::Markdown(content) = rendered.content;
            content
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
    on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    skill_registry: Option<&'a crate::skills::SkillRegistry>,
    tracer: &'a dyn TurnTracer,
    control: Option<TurnControl>,
}

impl ToolCallContext<'_> {
    fn invocation_context(&self, tool_name: &str) -> cortex_sdk::InvocationContext {
        cortex_sdk::InvocationContext {
            tool_name: tool_name.to_string(),
            session_id: self.config.session_id.clone(),
            actor: self.config.actor.clone(),
            source: self.config.source.clone(),
            execution_scope: self.config.execution_scope,
        }
    }
}

struct ExecutionResult {
    output: String,
    media: Vec<cortex_sdk::Attachment>,
    is_error: bool,
}

impl ExecutionResult {
    fn into_tool_result(self) -> ToolResult {
        if self.is_error {
            ToolResult::error(self.output)
        } else {
            ToolResult::success(self.output).with_media_many(self.media)
        }
    }
}

#[derive(Clone)]
struct SkillExecutionPlan {
    name: String,
    args: String,
    mode: cortex_types::ExecutionMode,
}

enum ExecutionUnit<'a> {
    Tool {
        name: &'a str,
        input: &'a serde_json::Value,
    },
    AgentSubTurn {
        input: &'a serde_json::Value,
    },
    InlineSkill {
        plan: SkillExecutionPlan,
    },
    ForkedSkill {
        plan: SkillExecutionPlan,
    },
}

fn resolve_execution_unit<'a>(
    tc_ctx: &ToolCallContext<'a>,
    tool_name: &'a str,
    tc_input: &'a serde_json::Value,
) -> Result<ExecutionUnit<'a>, ExecutionResult> {
    match tool_name {
        "agent" => Ok(ExecutionUnit::AgentSubTurn { input: tc_input }),
        "skill" => {
            let plan = resolve_skill_execution_plan(tc_ctx.skill_registry, tc_input)?;
            Ok(match plan.mode {
                cortex_types::ExecutionMode::Inline => ExecutionUnit::InlineSkill { plan },
                cortex_types::ExecutionMode::Fork => ExecutionUnit::ForkedSkill { plan },
            })
        }
        _ => Ok(ExecutionUnit::Tool {
            name: tool_name,
            input: tc_input,
        }),
    }
}

async fn execute_execution_unit(
    tc_ctx: &ToolCallContext<'_>,
    unit: ExecutionUnit<'_>,
) -> ExecutionResult {
    match unit {
        ExecutionUnit::Tool { name, input } => execute_tool(
            tc_ctx.tools,
            name,
            input,
            tc_ctx.config.tool_timeout_secs,
            tc_ctx.invocation_context(name),
            tc_ctx.on_event,
        ),
        ExecutionUnit::AgentSubTurn { input } => {
            execute_agent_sub_turn(AgentSubTurnParams {
                input,
                parent_config: tc_ctx.config,
                llm: tc_ctx.llm,
                journal: tc_ctx.journal,
                gate: tc_ctx.gate,
                parent_history: tc_ctx.history,
                on_event: tc_ctx.on_event,
                prompt_manager: tc_ctx.prompt_manager,
            })
            .await
        }
        ExecutionUnit::InlineSkill { plan } => dispatch_inline_skill(tc_ctx, &plan),
        ExecutionUnit::ForkedSkill { plan } => {
            let Some(registry) = tc_ctx.skill_registry else {
                return ExecutionResult {
                    output: "skill_registry not available for fork execution".to_string(),
                    media: Vec::new(),
                    is_error: true,
                };
            };
            execute_skill_sub_turn(SkillSubTurnParams {
                plan: &plan,
                skill_registry: registry,
                parent_config: tc_ctx.config,
                llm: tc_ctx.llm,
                journal: tc_ctx.journal,
                gate: tc_ctx.gate,
                on_event: tc_ctx.on_event,
            })
            .await
        }
    }
}

fn resolve_skill_execution_plan(
    skill_registry: Option<&crate::skills::SkillRegistry>,
    tc_input: &serde_json::Value,
) -> Result<SkillExecutionPlan, ExecutionResult> {
    let skill_name = tc_input
        .get("skill")
        .and_then(|v| v.as_str())
        .ok_or_else(|| ExecutionResult {
            output: "missing skill name".to_string(),
            media: Vec::new(),
            is_error: true,
        })?
        .trim()
        .trim_start_matches('/');
    let args = tc_input
        .get("args")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let Some(registry) = skill_registry else {
        return Err(ExecutionResult {
            output: format!("skill registry unavailable for '{skill_name}'"),
            media: Vec::new(),
            is_error: true,
        });
    };
    let Some(definition) = registry.definition(skill_name) else {
        return Err(ExecutionResult {
            output: format!(
                "Unknown skill: '{skill_name}'. Available: {}",
                registry.names().join(", ")
            ),
            media: Vec::new(),
            is_error: true,
        });
    };
    Ok(SkillExecutionPlan {
        name: definition.name,
        args,
        mode: definition.execution_mode,
    })
}

fn record_skill_invocation(
    tc_ctx: &ToolCallContext<'_>,
    skill_name: &str,
    mode: cortex_types::ExecutionMode,
) {
    let execution_mode = match mode {
        cortex_types::ExecutionMode::Inline => "inline",
        cortex_types::ExecutionMode::Fork => "fork",
    };
    let invoke_ev = Payload::SkillInvoked {
        name: skill_name.to_string(),
        trigger: cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
        execution_mode: execution_mode.to_string(),
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &invoke_ev);
}

fn dispatch_inline_skill(
    tc_ctx: &ToolCallContext<'_>,
    plan: &SkillExecutionPlan,
) -> ExecutionResult {
    record_skill_invocation(tc_ctx, &plan.name, plan.mode);
    let start = std::time::Instant::now();
    let result = execute_tool(
        tc_ctx.tools,
        "skill",
        &serde_json::json!({
            "skill": plan.name,
            "args": plan.args,
        }),
        tc_ctx.config.tool_timeout_secs,
        tc_ctx.invocation_context("skill"),
        tc_ctx.on_event,
    );
    let duration_ms = start.elapsed().as_millis();
    let complete_ev = Payload::SkillCompleted {
        name: plan.name.clone(),
        success: !result.is_error,
        duration_ms: u64::try_from(duration_ms).unwrap_or(u64::MAX),
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &complete_ev);
    if let Some(reg) = tc_ctx.skill_registry {
        reg.record_outcome(&plan.name, !result.is_error);
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

fn execution_unit_cancelled(
    tc_ctx: &ToolCallContext<'_>,
    tool_name: &str,
) -> Option<ExecutionResult> {
    if tc_ctx
        .control
        .as_ref()
        .is_some_and(|control| control.execution_boundary() == TurnControlBoundary::AbortTurn)
    {
        tc_ctx.tracer.trace_at(
            TraceCategory::Phase,
            cortex_types::TraceLevel::Minimal,
            &format!("Execution unit '{tool_name}' cancelled before start"),
        );
        Some(ExecutionResult {
            output: "cancelled by user (/stop)".to_string(),
            media: Vec::new(),
            is_error: true,
        })
    } else {
        None
    }
}

async fn process_approved_tool_call(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    tc_input: &serde_json::Value,
) -> ToolResult {
    tc_ctx.denial_tracker.record_approval();

    record_tool_approval(tc_ctx, tool_name, tc_input);

    emit_tool_progress(
        tc_ctx.on_event,
        ToolProgress {
            tool_name: tool_name.to_string(),
            status: ToolProgressStatus::Started,
            message: None,
        },
    );

    trace_tool_start(tc_ctx.tracer, tool_name, tc_input);

    let result = if let Some(cancelled) = execution_unit_cancelled(tc_ctx, tool_name) {
        cancelled
    } else {
        match resolve_execution_unit(tc_ctx, tool_name, tc_input) {
            Ok(unit) => execute_execution_unit(tc_ctx, unit).await,
            Err(error) => error,
        }
    }
    .into_tool_result();

    trace_tool_finish(tc_ctx.tracer, tool_name, &result);

    emit_tool_progress(
        tc_ctx.on_event,
        ToolProgress {
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
        },
    );

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

    result
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
    invocation: cortex_sdk::InvocationContext,
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
) -> ExecutionResult {
    let Some(tool) = tools.get(name) else {
        return ExecutionResult {
            output: format!("unknown tool: {name}"),
            media: Vec::new(),
            is_error: true,
        };
    };

    let timeout_secs = tool.timeout_secs().unwrap_or(global_timeout_secs);
    let input_clone = input.clone();
    let start = std::time::Instant::now();

    // Execute tool in a scoped OS thread to avoid blocking the tokio runtime.
    // Scoped threads can borrow `tool` (&dyn Tool) safely.
    let result = std::thread::scope(|s| {
        let handle =
            s.spawn(move || {
                struct ToolRuntimeBridge<'a> {
                    invocation: cortex_sdk::InvocationContext,
                    on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
                }

                impl cortex_sdk::ToolRuntime for ToolRuntimeBridge<'_> {
                    fn invocation(&self) -> &cortex_sdk::InvocationContext {
                        &self.invocation
                    }

                    fn emit_progress(&self, message: &str) {
                        if let Some(callback) = &self.on_event {
                            callback(&TurnStreamEvent::ToolProgress(ToolProgress {
                                tool_name: self.invocation.tool_name.clone(),
                                status: ToolProgressStatus::Running,
                                message: Some(message.to_string()),
                            }));
                        }
                    }

                    fn emit_observer(&self, source: Option<&str>, content: &str) {
                        if let Some(callback) = &self.on_event {
                            callback(&TurnStreamEvent::Text {
                                lane: StreamLane::Observer,
                                source: Some(source.map_or_else(
                                    || self.invocation.tool_name.clone(),
                                    str::to_string,
                                )),
                                content: content.to_string(),
                            });
                        }
                    }
                }

                let runtime = ToolRuntimeBridge {
                    invocation,
                    on_event,
                };
                match tool.execute_with_runtime(input_clone, &runtime) {
                    Ok(r) => ExecutionResult {
                        output: r.output,
                        media: r.media,
                        is_error: r.is_error,
                    },
                    Err(e) => ExecutionResult {
                        output: format!("tool error: {e}"),
                        media: Vec::new(),
                        is_error: true,
                    },
                }
            });
        handle.join().unwrap_or_else(|_| ExecutionResult {
            output: format!("tool '{name}' panicked"),
            media: Vec::new(),
            is_error: true,
        })
    });

    let elapsed = start.elapsed();
    if elapsed.as_secs() > timeout_secs {
        return ExecutionResult {
            output: format!(
                "tool '{name}' exceeded timeout ({timeout_secs}s, took {:.1}s)",
                elapsed.as_secs_f64()
            ),
            media: Vec::new(),
            is_error: true,
        };
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
    on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
}

type EventCallback<'a> = &'a (dyn Fn(&TurnStreamEvent) + Send + Sync);

enum SubTurnKind {
    Agent {
        description: String,
        mode: AgentSubTurnMode,
    },
    Skill {
        name: String,
    },
}

impl SubTurnKind {
    fn observer_label(&self) -> String {
        match self {
            Self::Agent { description, .. } => format!("agent:{description}"),
            Self::Skill { name } => format!("skill:{name}"),
        }
    }

    fn success_fallback(&self) -> String {
        match self {
            Self::Agent { description, mode } => {
                format!("[Agent '{description}' ({mode} mode)] completed with no text response")
            }
            Self::Skill { name } => format!("[Skill '{name}' (fork)] completed"),
        }
    }

    fn failure_prefix(&self) -> String {
        match self {
            Self::Agent { description, .. } => format!("agent '{description}' failed"),
            Self::Skill { name } => format!("skill fork '{name}' failed"),
        }
    }

    fn invocation_payload(&self) -> Payload {
        match self {
            Self::Agent { description, .. } => Payload::AgentWorkerSpawned {
                worker_name: description.clone(),
            },
            Self::Skill { name } => Payload::SkillInvoked {
                name: name.clone(),
                trigger: cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
                execution_mode: "fork".to_string(),
            },
        }
    }

    fn completion_payload(&self, result: &ExecutionResult, start: std::time::Instant) -> Payload {
        match self {
            Self::Agent { description, .. } => Payload::AgentWorkerCompleted {
                worker_name: description.clone(),
                result_len: result.output.len(),
                input_tokens: 0,
                output_tokens: 0,
            },
            Self::Skill { name } => Payload::SkillCompleted {
                name: name.clone(),
                success: !result.is_error,
                duration_ms: u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX),
            },
        }
    }

    fn build_history(&self, parent_history: &[Message]) -> Vec<Message> {
        match self {
            Self::Agent {
                mode: AgentSubTurnMode::Fork,
                ..
            } => parent_history.to_vec(),
            Self::Agent { .. } | Self::Skill { .. } => Vec::new(),
        }
    }

    fn build_config(
        &self,
        input: &serde_json::Value,
        parent_config: &TurnConfig,
        prompt_manager: Option<&cortex_kernel::PromptManager>,
    ) -> TurnConfig {
        const TEAM_PLACEHOLDER: &str = "{team}";
        let system_prompt = match self {
            Self::Agent {
                mode: AgentSubTurnMode::Fork,
                ..
            } => parent_config.system_prompt.clone(),
            Self::Agent {
                mode: AgentSubTurnMode::Teammate,
                ..
            } => {
                let team_name = input
                    .get("team_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                let template = prompt_manager
                    .and_then(|pm| pm.get_system_template("agent-teammate"))
                    .unwrap_or_else(|| {
                        cortex_kernel::prompt_manager::DEFAULT_AGENT_TEAMMATE.to_string()
                    });
                Some(template.replace(TEAM_PLACEHOLDER, team_name))
            }
            Self::Agent { .. } | Self::Skill { .. } => None,
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
            llm_transient_retries: parent_config.llm_transient_retries,
            strip_think_tags: parent_config.strip_think_tags,
            evolution_weights: parent_config.evolution_weights,
            pressure_thresholds: parent_config.pressure_thresholds,
            metacognition: parent_config.metacognition.clone(),
            trace: parent_config.trace.clone(),
            session_id: parent_config.session_id.clone(),
            actor: parent_config.actor.clone(),
            source: parent_config.source.clone(),
            execution_scope: parent_config.execution_scope,
        }
    }

    fn build_tools(&self, current_depth: usize) -> ToolRegistry {
        let can_recurse_agent = current_depth + 1 < MAX_AGENT_DEPTH;
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(crate::tools::read::ReadTool));
        registry.register(Box::new(crate::tools::write::WriteTool));
        registry.register(Box::new(crate::tools::edit::EditTool));
        registry.register(Box::new(crate::tools::bash::BashTool));

        let execution_allows_agent = match self {
            Self::Agent {
                mode: AgentSubTurnMode::Readonly,
                ..
            } => false,
            Self::Agent { .. } | Self::Skill { .. } => true,
        };
        if execution_allows_agent && can_recurse_agent {
            registry.register(Box::new(crate::tools::agent::AgentTool));
        }

        registry
    }
}

#[derive(Clone, Copy)]
enum AgentSubTurnMode {
    Readonly,
    Fork,
    Teammate,
    Full,
}

impl AgentSubTurnMode {
    fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("readonly") {
            "fork" => Self::Fork,
            "teammate" => Self::Teammate,
            "full" => Self::Full,
            _ => Self::Readonly,
        }
    }
}

impl std::fmt::Display for AgentSubTurnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Readonly => write!(f, "readonly"),
            Self::Fork => write!(f, "fork"),
            Self::Teammate => write!(f, "teammate"),
            Self::Full => write!(f, "full"),
        }
    }
}

struct SubTurnSpec<'a> {
    kind: SubTurnKind,
    input: &'a str,
    history: &'a mut Vec<Message>,
    llm: &'a dyn LlmClient,
    tools: &'a ToolRegistry,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    config: &'a TurnConfig,
    parent_on_event: Option<EventCallback<'a>>,
}

fn forward_sub_turn_event(
    parent_on_event: Option<EventCallback<'_>>,
    observer_source: &str,
    event: &TurnStreamEvent,
) {
    match event {
        TurnStreamEvent::Text { content, .. } => emit_text_event(
            parent_on_event,
            StreamLane::Observer,
            Some(observer_source),
            content,
        ),
        TurnStreamEvent::Boundary(_) | TurnStreamEvent::ToolProgress(_) => {}
    }
}

struct ObservedSubTurnParams<'a> {
    input: &'a str,
    history: &'a mut Vec<Message>,
    llm: &'a dyn LlmClient,
    tools: &'a ToolRegistry,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    config: &'a TurnConfig,
    parent_on_event: Option<EventCallback<'a>>,
    observer_source: &'a str,
}

struct SubTurnLaunch<'a> {
    kind: SubTurnKind,
    input: &'a str,
    parent_history: &'a [Message],
    parent_config: &'a TurnConfig,
    llm: &'a dyn LlmClient,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    parent_on_event: Option<EventCallback<'a>>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    config_input: &'a serde_json::Value,
}

async fn run_observed_sub_turn(
    params: ObservedSubTurnParams<'_>,
) -> Result<super::TurnResult, super::TurnError> {
    let ObservedSubTurnParams {
        input,
        history,
        llm,
        tools,
        journal,
        gate,
        config,
        parent_on_event,
        observer_source,
    } = params;
    let observer_event = |event: &TurnStreamEvent| {
        forward_sub_turn_event(parent_on_event, observer_source, event);
    };
    let sub_ctx = TurnContext {
        input,
        history,
        llm,
        vision_llm: None,
        tools,
        journal,
        gate,
        config,
        on_event: parent_on_event.map(|_| &observer_event as EventCallback<'_>),
        images: vec![],
        compress_template: None,
        summary_cache: None,
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };

    super::run_turn(sub_ctx).await
}

async fn execute_sub_turn(spec: SubTurnSpec<'_>) -> ExecutionResult {
    let SubTurnSpec {
        kind,
        input,
        history,
        llm,
        tools,
        journal,
        gate,
        config,
        parent_on_event,
    } = spec;
    let observer_label = kind.observer_label();
    let success_fallback = kind.success_fallback();
    let failure_prefix = kind.failure_prefix();
    let lifecycle_turn_id = TurnId::new();
    let lifecycle_corr_id = CorrelationId::new();
    let invocation_payload = kind.invocation_payload();
    journal_append(
        journal,
        lifecycle_turn_id,
        lifecycle_corr_id,
        &invocation_payload,
    );
    let start = std::time::Instant::now();
    let result = match run_observed_sub_turn(ObservedSubTurnParams {
        input,
        history,
        llm,
        tools,
        journal,
        gate,
        config,
        parent_on_event,
        observer_source: &observer_label,
    })
    .await
    {
        Ok(result) => result.response_text.map_or_else(
            || ExecutionResult {
                output: success_fallback,
                media: Vec::new(),
                is_error: false,
            },
            |text| ExecutionResult {
                output: text,
                media: Vec::new(),
                is_error: false,
            },
        ),
        Err(error) => ExecutionResult {
            output: format!("{failure_prefix}: {error}"),
            media: Vec::new(),
            is_error: true,
        },
    };
    let completion_payload = kind.completion_payload(&result, start);
    journal_append(
        journal,
        lifecycle_turn_id,
        lifecycle_corr_id,
        &completion_payload,
    );
    result
}

async fn launch_sub_turn(params: SubTurnLaunch<'_>) -> ExecutionResult {
    let SubTurnLaunch {
        kind,
        input,
        parent_history,
        parent_config,
        llm,
        journal,
        gate,
        parent_on_event,
        prompt_manager,
        config_input,
    } = params;
    let sub_tools = kind.build_tools(parent_config.agent_depth);
    let sub_config = kind.build_config(config_input, parent_config, prompt_manager);
    let mut sub_history = kind.build_history(parent_history);
    execute_sub_turn(SubTurnSpec {
        kind,
        input,
        history: &mut sub_history,
        llm,
        tools: &sub_tools,
        journal,
        gate,
        config: &sub_config,
        parent_on_event,
    })
    .await
}

async fn execute_agent_sub_turn(params: AgentSubTurnParams<'_>) -> ExecutionResult {
    let AgentSubTurnParams {
        input,
        parent_config,
        llm,
        journal,
        gate,
        parent_history,
        on_event,
        prompt_manager,
    } = params;
    // Parse agent parameters
    let Some(prompt) = input.get("prompt").and_then(|v| v.as_str()) else {
        return ExecutionResult {
            output: "agent: missing prompt".to_string(),
            media: Vec::new(),
            is_error: true,
        };
    };

    let mode = AgentSubTurnMode::parse(input.get("mode").and_then(|v| v.as_str()));

    let description = input
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("sub-agent");

    // Check recursion depth
    if parent_config.agent_depth >= MAX_AGENT_DEPTH {
        return ExecutionResult {
            output: format!(
                "agent '{description}': max recursion depth ({MAX_AGENT_DEPTH}) exceeded"
            ),
            media: Vec::new(),
            is_error: true,
        };
    }

    let kind = SubTurnKind::Agent {
        description: description.to_string(),
        mode,
    };
    launch_sub_turn(SubTurnLaunch {
        kind,
        input: prompt,
        parent_history,
        parent_config,
        llm,
        journal,
        gate,
        parent_on_event: on_event,
        prompt_manager,
        config_input: input,
    })
    .await
}

struct SkillSubTurnParams<'a> {
    plan: &'a SkillExecutionPlan,
    skill_registry: &'a crate::skills::SkillRegistry,
    parent_config: &'a TurnConfig,
    llm: &'a dyn LlmClient,
    journal: &'a Journal,
    gate: &'a dyn PermissionGate,
    on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
}

fn resolve_forked_skill_content(
    skill_registry: &crate::skills::SkillRegistry,
    skill_name: &str,
    args: &str,
) -> Option<String> {
    skill_registry.render(skill_name, args).map(|rendered| {
        let crate::skills::SkillContent::Markdown(content) = rendered.content;
        content
    })
}

async fn execute_skill_sub_turn(params: SkillSubTurnParams<'_>) -> ExecutionResult {
    let SkillSubTurnParams {
        plan,
        skill_registry,
        parent_config,
        llm,
        journal,
        gate,
        on_event,
    } = params;

    let Some(content) = resolve_forked_skill_content(skill_registry, &plan.name, &plan.args) else {
        return ExecutionResult {
            output: format!("skill fork: unknown skill '{}'", plan.name),
            media: Vec::new(),
            is_error: true,
        };
    };

    if parent_config.agent_depth >= MAX_AGENT_DEPTH {
        return ExecutionResult {
            output: format!(
                "skill fork '{}': max depth ({MAX_AGENT_DEPTH}) exceeded",
                plan.name
            ),
            media: Vec::new(),
            is_error: true,
        };
    }
    let kind = SubTurnKind::Skill {
        name: plan.name.clone(),
    };
    let config_input = serde_json::Value::Null;
    let result = launch_sub_turn(SubTurnLaunch {
        kind,
        input: &content,
        parent_history: &[],
        parent_config,
        llm,
        journal,
        gate,
        parent_on_event: on_event,
        prompt_manager: None,
        config_input: &config_input,
    })
    .await;
    skill_registry.record_outcome(&plan.name, !result.is_error);
    result
}
