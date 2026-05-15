use crate::llm::LlmResponse;
use crate::llm::cost::estimate_cost;
use crate::orchestrator::{TraceCategory, TurnTracer};
use crate::tools::ToolResult;

pub(super) fn trace_llm_result(tracer: &dyn TurnTracer, response: &LlmResponse) {
    tracer.trace_at(
        TraceCategory::Llm,
        cortex_types::TraceLevel::Basic,
        &format!(
            "LLM complete: {}in/{}out tokens, {}cache-read/{}cache-write, est ${:.4}",
            response.usage.input_tokens,
            response.usage.output_tokens,
            response.usage.cache_read_input_tokens,
            response.usage.cache_creation_input_tokens,
            estimate_cost(
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

pub(super) fn trace_tool_start(
    tracer: &dyn TurnTracer,
    tool_name: &str,
    tc_input: &serde_json::Value,
) {
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

pub(super) fn trace_tool_finish(tracer: &dyn TurnTracer, tool_name: &str, result: &ToolResult) {
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

fn truncate_json(value: &serde_json::Value, max_len: usize) -> String {
    let s = value.to_string();
    truncate_json_str(&s, max_len)
}

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
