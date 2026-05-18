use cortex_kernel::Journal;
use cortex_types::{Attachment, CorrelationId, Message, Payload, TurnId};

use crate::attention::ChannelScheduler;
use crate::confidence::ConfidenceTracker;
use crate::llm::{LlmClient, LlmError, LlmRequest, LlmResponse};
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::risk::{DenialTracker, PermissionGate, RiskAssessor};
use crate::tools::ToolRegistry;
use crate::working_memory::WorkingMemoryManager;

use super::dmn::{PressureContext, apply_compress_history};
use super::journal_append;
use super::stream::ThinkStreamFilter;
use super::{
    TraceCategory, TurnConfig, TurnControl, TurnControlBoundary, TurnControlCheckpoint, TurnError,
    TurnStreamEvent, TurnTracer, dispatch_turn_control,
};

mod context;
mod events;
mod guardrails;
mod meta;
mod subturn;
mod subturn_contract;
mod tool_batch;
mod tool_runtime;
mod trace;

pub use context::{build_dynamic_context_frame, record_llm_cost, record_response_events};
use context::{build_request_messages, flush_scheduler_events_for_turn};
use tool_batch::{ToolBatchControl, process_tool_calls_batch};

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
    pub dynamic_context: Option<&'a String>,
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

fn handle_iteration_boundary_control(ctx: &mut TpnLoopContext<'_>) -> bool {
    match dispatch_turn_control(
        ctx.control.as_ref(),
        ctx.history,
        ctx.tracer,
        TurnControlCheckpoint::IterationBoundary,
    ) {
        TurnControlBoundary::Continue => false,
        TurnControlBoundary::RestartTurn => {
            events::emit_restart_boundary_event(ctx.on_event);
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
        events::emit_restart_boundary_event(ctx.on_event);
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
    let text = events::visible_assistant_text(ctx.config.strip_think_tags, &response.text?);
    if text.trim().is_empty() {
        return None;
    }
    let payload = Payload::AssistantMessage {
        content: text.clone(),
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
    ctx.events_log.push(payload);
    ctx.history.push(Message::assistant(&text));
    Some(text)
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
    trace::trace_llm_result(ctx.tracer, response);
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

async fn handle_tpn_context_pressure(ctx: &mut TpnLoopContext<'_>, active_llm: &dyn LlmClient) {
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
}

// ── Main loop ───────────────────────────────────────────────

pub async fn run_tpn_loop(ctx: &mut TpnLoopContext<'_>) -> Result<Option<String>, TurnError> {
    let mut final_text: Option<String> = None;
    // Metacognition strategy hint -- injected into the request-local runtime frame.
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

        handle_tpn_context_pressure(ctx, active_llm).await;

        let dynamic_context = build_dynamic_context_frame(
            ctx.dynamic_context.map(String::as_str),
            ctx.reasoning_engine,
            &mut meta_hint,
        );
        let request_messages = build_request_messages(ctx.history, dynamic_context.as_deref());

        ctx.tracer.trace_at(
            TraceCategory::Llm,
            cortex_types::TraceLevel::Basic,
            &format!("LLM call #{}", iteration + 1),
        );

        let on_event = ctx.on_event;
        let strip_stream_thinking = ctx.config.strip_think_tags;
        let stream_filter = std::sync::Mutex::new(ThinkStreamFilter::new(strip_stream_thinking));
        let main_text_emitter = |text: &str| {
            events::emit_filtered_stream_text(on_event, &stream_filter, text);
        };

        let request = build_llm_request(
            ctx,
            active_llm,
            ctx.system_prompt.map(String::as_str),
            &request_messages,
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
            let retry_messages = build_request_messages(ctx.history, dynamic_context.as_deref());
            let retry_request = build_llm_request(
                ctx,
                active_llm,
                ctx.system_prompt.map(String::as_str),
                &retry_messages,
                &main_text_emitter,
            );
            llm_result = active_llm.complete(retry_request).await;
        }

        let response = handle_llm_result(llm_result, ctx.history, has_images_for_request)?;
        events::emit_pending_stream_text(on_event, &stream_filter);

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
            meta::post_tool_iteration(ctx, &response, tool_iteration, &mut meta_hint).await
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
    messages: &'a [Message],
    main_text_emitter: &'a (dyn Fn(&str) + Send + Sync),
) -> LlmRequest<'a> {
    let can_use_tools = !ctx.tool_defs.is_empty()
        && (!messages.iter().any(cortex_types::Message::has_images)
            || llm.supports_tools_with_images());
    LlmRequest {
        system,
        messages,
        tools: can_use_tools.then_some(ctx.tool_defs),
        max_tokens: ctx.config.max_tokens,
        thinking: !ctx.config.strip_think_tags,
        transient_retries: ctx.config.llm_transient_retries,
        on_text: ctx.on_event.map(|_| main_text_emitter),
    }
}
