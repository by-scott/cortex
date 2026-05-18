use cortex_kernel::Journal;
use cortex_types::{Attachment, CorrelationId, Message, Payload, PermissionDecision, Role, TurnId};

use crate::attention::ChannelScheduler;
use crate::confidence::ConfidenceTracker;
use crate::llm::{LlmClient, LlmError, LlmRequest, LlmResponse};
use crate::meta::monitor::MetaMonitor;
use crate::reasoning::ReasoningEngine;
use crate::risk::{DenialTracker, PermissionGate, RiskAssessor};
use crate::tools::{ToolRegistry, ToolResult};
use crate::working_memory::WorkingMemoryManager;

use super::dmn::{PressureContext, apply_compress_history};
use super::journal_append;
use super::permission::evaluate_tool_permission;
use super::stream::ThinkStreamFilter;
use super::tool_effects;
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
mod tool_runtime;
mod trace;

pub use context::{build_dynamic_context_frame, record_llm_cost, record_response_events};
use context::{build_request_messages, flush_scheduler_events_for_turn};
use guardrails::sdk_attachment_to_core;
pub use guardrails::{
    external_input_observed_payload, tool_output_guardrail_payload,
    untrusted_tool_result_for_history,
};
use subturn::{
    AgentSubTurnParams, SkillSubTurnParams, execute_agent_sub_turn, execute_skill_sub_turn,
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
        events::emit_restart_boundary_event(ctx.on_event);
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

    if let Some(text) = &response.text {
        let text = events::visible_assistant_text(ctx.config.strip_think_tags, text);
        if !text.trim().is_empty() {
            record_assistant_text(ctx.journal, ctx.turn_id, ctx.corr_id, ctx.events_log, &text);
            assistant_blocks.push(cortex_types::ContentBlock::Text { text });
        }
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

        let effects = ctx.tools.effects(&tool_name);
        tool_effects::record_preview(
            ctx.journal,
            ctx.turn_id,
            ctx.corr_id,
            ctx.events_log,
            &tool_name,
            &tc.input,
            &effects,
        );
        let permission = evaluate_tool_permission(ctx, &tool_name, &tc.input, &effects);

        let result = match permission.decision {
            PermissionDecision::Approved => {
                let mut tc_ctx = build_tool_call_context(ctx);
                process_approved_tool_call(&mut tc_ctx, &tc.id, &tool_name, &tc.input).await
            }
            PermissionDecision::Denied => {
                let reason = permission
                    .denial_reason
                    .as_deref()
                    .unwrap_or("blocked by permission gate");
                handle_denied_tool(ctx, &tool_name, reason)
            }
            PermissionDecision::Pending | PermissionDecision::TimedOut => {
                // Fallback: if we reach here the gate did not resolve interactively.
                // Treat as denied — safe default.
                let reason = permission
                    .denial_reason
                    .as_deref()
                    .unwrap_or("permission confirmation was not resolved");
                handle_denied_tool(ctx, &tool_name, reason)
            }
        };
        let tool_output = result.output;
        let is_error = result.is_error;
        if !is_error {
            ctx.response_media
                .extend(result.media.into_iter().map(sdk_attachment_to_core));
        }

        if !is_error {
            record_external_input_observed(ctx, &tool_name, &tool_output);
            record_tool_output_guardrail(ctx, &tool_name, &tool_output);
        }
        tool_results_for_history.push(cortex_types::ContentBlock::ToolResult {
            tool_use_id: tc.id.clone(),
            content: untrusted_tool_result_for_history(&tool_name, &tool_output),
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

fn record_external_input_observed(ctx: &mut TpnLoopContext<'_>, tool_name: &str, output: &str) {
    let payload = external_input_observed_payload(tool_name, output);
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
    ctx.events_log.push(payload);
}

fn record_tool_output_guardrail(ctx: &mut TpnLoopContext<'_>, tool_name: &str, output: &str) {
    if let Some(payload) = tool_output_guardrail_payload(tool_name, output) {
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
        ctx.events_log.push(payload);
    }
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
        ExecutionUnit::Tool { name, input } => tool_runtime::execute_tool(
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
    let result = tool_runtime::execute_tool(
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
        record_skill_execution_trace(
            reg,
            plan,
            cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
            u64::try_from(duration_ms).unwrap_or(u64::MAX),
            &result,
        );
    }
    result
}

fn record_skill_execution_trace(
    registry: &crate::skills::SkillRegistry,
    plan: &SkillExecutionPlan,
    trigger: String,
    duration_ms: u64,
    result: &ExecutionResult,
) {
    let mut trace = cortex_types::SkillExecutionTrace::started(
        format!("skill-trace-{}", cortex_types::EventId::new()),
        plan.name.clone(),
        trigger,
        plan.mode,
        summarize_skill_text(&plan.args),
    );
    if let Some(manifest) = registry.manifest(&plan.name) {
        trace = trace.with_manifest(&manifest);
    }
    registry.record_trace(trace.complete(
        !result.is_error,
        duration_ms,
        summarize_skill_text(&result.output),
    ));
}

fn summarize_skill_text(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= 160 {
        return trimmed.to_string();
    }
    trimmed.chars().take(160).collect()
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

fn acp_agent_id(input: &serde_json::Value) -> Option<String> {
    input
        .get("agent_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn record_acp_client_invoked(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    input: &serde_json::Value,
) {
    if tool_name != "acp_agent" {
        return;
    }
    let Some(agent_id) = acp_agent_id(input) else {
        return;
    };
    let payload = Payload::AcpClientInvoked {
        command: "configured".to_string(),
        agent_id,
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &payload);
    tc_ctx.events_log.push(payload);
}

fn record_acp_client_response(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    input: &serde_json::Value,
    result: &ToolResult,
) {
    if tool_name != "acp_agent" || result.is_error {
        return;
    }
    let Some(agent_id) = acp_agent_id(input) else {
        return;
    };
    let payload = Payload::AcpClientResponse {
        agent_id,
        response_len: result.output.len(),
    };
    journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &payload);
    tc_ctx.events_log.push(payload);
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
    tool_call_id: &str,
    tool_name: &str,
    tc_input: &serde_json::Value,
) -> ToolResult {
    tc_ctx.denial_tracker.record_approval();

    record_tool_approval(tc_ctx, tool_name, tc_input);
    record_acp_client_invoked(tc_ctx, tool_name, tc_input);

    events::emit_tool_progress(
        tc_ctx.on_event,
        ToolProgress {
            tool_name: tool_name.to_string(),
            status: ToolProgressStatus::Started,
            message: None,
        },
    );

    trace::trace_tool_start(tc_ctx.tracer, tool_name, tc_input);

    let result = if let Some(cancelled) = execution_unit_cancelled(tc_ctx, tool_name) {
        cancelled
    } else {
        match resolve_execution_unit(tc_ctx, tool_name, tc_input) {
            Ok(unit) => execute_execution_unit(tc_ctx, unit).await,
            Err(error) => error,
        }
    }
    .into_tool_result();

    trace::trace_tool_finish(tc_ctx.tracer, tool_name, &result);
    let effects = tc_ctx.tools.effects(tool_name);
    tool_effects::record_verification(
        tc_ctx.journal,
        tc_ctx.turn_id,
        tc_ctx.corr_id,
        tc_ctx.events_log,
        tool_name,
        &effects,
        &result,
    );
    record_acp_client_response(tc_ctx, tool_name, tc_input, &result);
    emit_tool_completion_progress(tc_ctx.on_event, tool_name, &result);
    record_tool_invocation_result(tc_ctx, tool_name, &result);
    update_tool_execution_state(tc_ctx, tool_name, tc_input, &result);
    record_external_io_side_effect(tc_ctx, tool_call_id, tool_name, &result);

    result
}

fn emit_tool_completion_progress(
    on_event: Option<&(dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    tool_name: &str,
    result: &ToolResult,
) {
    events::emit_tool_progress(
        on_event,
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
}

fn record_tool_invocation_result(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    result: &ToolResult,
) {
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
}

fn update_tool_execution_state(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_name: &str,
    tc_input: &serde_json::Value,
    result: &ToolResult,
) {
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
}

fn record_external_io_side_effect(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    result: &ToolResult,
) {
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
            key: format!("{}:{tool_call_id}:{tool_name}", tc_ctx.turn_id),
            value: truncated,
        };
        journal_append(tc_ctx.journal, tc_ctx.turn_id, tc_ctx.corr_id, &se);
        tc_ctx.events_log.push(se);
    }
}

fn handle_denied_tool(ctx: &mut TpnLoopContext<'_>, tool_name: &str, reason: &str) -> ToolResult {
    ctx.denial_tracker.record_denial();
    ctx.confidence.record_denial();
    let deny_payload = Payload::PermissionDenied {
        tool_name: tool_name.to_string(),
        reason: reason.to_string(),
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &deny_payload);
    ctx.events_log.push(deny_payload);
    let output = if reason.trim().is_empty() {
        "permission denied".to_string()
    } else {
        format!("permission denied: {reason}")
    };
    ToolResult {
        output,
        media: Vec::new(),
        is_error: true,
    }
}
