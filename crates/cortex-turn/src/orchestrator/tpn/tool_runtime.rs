use super::tool_batch::ExecutionResult;
use super::{ToolProgress, ToolProgressStatus};
use crate::orchestrator::{StreamLane, TurnStreamEvent};
use crate::tools::ToolRegistry;

/// Execute a tool with timeout enforcement.
///
/// Measures execution time against the configured timeout. If a tool exceeds
/// the limit, the result is replaced with a timeout error. Note: synchronous
/// tool code cannot be preemptively cancelled in Rust -- the timeout is checked
/// post-execution. For tools that may truly hang (e.g., bash), the tool itself
/// should implement internal timeout (bash already uses process timeouts).
pub(super) fn execute_tool(
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
        let handle = s.spawn(move || {
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
                source: Some(
                    source.map_or_else(|| self.invocation.tool_name.clone(), str::to_string),
                ),
                content: content.to_string(),
            });
        }
    }
}
