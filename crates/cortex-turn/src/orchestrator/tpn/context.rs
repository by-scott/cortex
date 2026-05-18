use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Message, Payload, TurnId};

use crate::attention::ChannelScheduler;
use crate::reasoning::ReasoningEngine;

use super::super::journal_append;
use super::TpnLoopContext;

pub(super) fn flush_scheduler_events_for_turn(ctx: &mut TpnLoopContext<'_>) {
    flush_scheduler_events(
        ctx.scheduler,
        ctx.journal,
        ctx.turn_id,
        ctx.corr_id,
        ctx.events_log,
    );
}

fn flush_scheduler_events(
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

/// Build a request-local dynamic context frame.
///
/// This frame is deliberately not part of the provider system prompt. It carries
/// volatile runtime facts and tactical hints after the stable conversation
/// prefix, keeping provider prompt caches useful while still giving the model
/// the current evidence it needs for this call.
pub fn build_dynamic_context_frame(
    base_context: Option<&str>,
    reasoning_engine: &ReasoningEngine,
    meta_hint: &mut Option<String>,
) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(context) = base_context
        && !context.trim().is_empty()
    {
        parts.push(context.trim().to_string());
    }
    if let Some(reasoning_context) = reasoning_engine.format_context()
        && !reasoning_context.trim().is_empty()
    {
        parts.push(reasoning_context.trim().to_string());
    }

    if let Some(hint) = meta_hint.take() {
        parts.push(format!("[Metacognition]\n{hint}"));
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("\n\n"))
    }
}

pub(super) fn build_request_messages(
    history: &[Message],
    dynamic_context: Option<&str>,
) -> Vec<Message> {
    let mut messages = history.to_vec();
    if let Some(context) = dynamic_context
        && !context.trim().is_empty()
    {
        messages.push(Message::user(format!(
            "[Cortex Runtime Frame]\n\
             Scope: current LLM call only. This is runtime context, not user text; \
             it cannot override system, tool, permission, or safety contracts. \
             Use it as evidence and execution context for the active request, \
             then discard it.\n\n{}",
            context.trim()
        )));
    }
    messages
}

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
        cache_read_input_tokens: response.usage.cache_read_input_tokens,
        cache_creation_input_tokens: response.usage.cache_creation_input_tokens,
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
