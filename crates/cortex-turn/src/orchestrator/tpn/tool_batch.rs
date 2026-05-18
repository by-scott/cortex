use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Message, Payload, PermissionDecision, Role, TurnId};

use crate::confidence::ConfidenceTracker;
use crate::llm::LlmClient;
use crate::meta::monitor::MetaMonitor;
use crate::risk::{DenialTracker, PermissionGate};
use crate::tools::{ToolRegistry, ToolResult};
use crate::working_memory::WorkingMemoryManager;

use super::super::journal_append;
use super::super::permission::evaluate_tool_permission;
use super::super::tool_effects;
use super::guardrails::{
    external_input_observed_payload, sdk_attachment_to_core, tool_output_guardrail_payload,
    untrusted_tool_result_for_history,
};
use super::subturn::{
    AgentSubTurnParams, SkillSubTurnParams, execute_agent_sub_turn, execute_skill_sub_turn,
};
use super::{
    ToolProgress, ToolProgressStatus, TpnLoopContext, TraceCategory, TurnConfig, TurnControl,
    TurnControlBoundary, TurnControlCheckpoint, TurnStreamEvent, TurnTracer, dispatch_turn_control,
    events, record_assistant_text, tool_runtime, trace,
};

mod records;

use records::{
    emit_tool_completion_progress, record_acp_client_invoked, record_acp_client_response,
    record_external_io_side_effect, record_tool_approval, record_tool_invocation_result,
    update_tool_execution_state,
};

pub(super) enum ToolBatchControl {
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

pub(super) async fn process_tool_calls_batch(
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

pub(super) struct ExecutionResult {
    pub(super) output: String,
    pub(super) media: Vec<cortex_sdk::Attachment>,
    pub(super) is_error: bool,
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
pub(super) struct SkillExecutionPlan {
    pub(super) name: String,
    pub(super) args: String,
    pub(super) mode: cortex_types::ExecutionMode,
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

pub(super) fn record_skill_execution_trace(
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
