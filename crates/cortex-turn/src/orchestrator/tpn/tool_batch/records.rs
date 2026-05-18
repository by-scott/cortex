use cortex_types::Payload;

use crate::tools::ToolResult;

use super::super::super::journal_append;
use super::{ToolCallContext, ToolProgress, ToolProgressStatus, TurnStreamEvent, events};

pub(super) fn record_tool_approval(
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

pub(super) fn record_acp_client_invoked(
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

pub(super) fn record_acp_client_response(
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

pub(super) fn emit_tool_completion_progress(
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

pub(super) fn record_tool_invocation_result(
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

pub(super) fn update_tool_execution_state(
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

pub(super) fn record_external_io_side_effect(
    tc_ctx: &mut ToolCallContext<'_>,
    tool_call_id: &str,
    tool_name: &str,
    result: &ToolResult,
) {
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

fn acp_agent_id(input: &serde_json::Value) -> Option<String> {
    input
        .get("agent_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}
