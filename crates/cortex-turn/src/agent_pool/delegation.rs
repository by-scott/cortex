use std::sync::Arc;

use cortex_types::Message;

use super::{AgentPool, WorkerResult};
use crate::llm::{LlmClient, LlmRequest, LlmResponse, LlmToolCall, Usage};
use crate::risk::PermissionGate;
use crate::tools::{ToolRegistry, ToolResult};

/// Configuration for worker LLM calls within delegation.
#[derive(Debug, Clone)]
pub struct DelegationConfig {
    /// Maximum tokens for each LLM call.
    pub max_tokens: usize,
    /// Maximum LLM-tool loop iterations (full/teammate modes).
    pub max_iterations: usize,
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2048,
            max_iterations: 10,
        }
    }
}

/// A structured task to delegate to an agent worker.
#[derive(Debug, Clone)]
pub struct TaskDelegation {
    /// Unique name for this task.
    pub name: String,
    /// The prompt/instruction for the agent.
    pub prompt: String,
    /// Execution mode: "readonly", "full", "fork", "teammate".
    pub mode: String,
    /// Team name for teammate mode.
    pub team_name: Option<String>,
}

impl TaskDelegation {
    /// Create a new task delegation with default readonly mode.
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            mode: "readonly".into(),
            team_name: None,
        }
    }

    #[must_use]
    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = mode.into();
        self
    }

    #[must_use]
    pub fn with_team_name(mut self, team_name: impl Into<String>) -> Self {
        self.team_name = Some(team_name.into());
        self
    }
}

/// Result from a delegated task.
#[derive(Debug, Clone)]
pub struct DelegationResult {
    /// Task name (matches `TaskDelegation::name`).
    pub name: String,
    /// Output from the agent worker.
    pub output: String,
    /// Whether the task completed successfully.
    pub success: bool,
    /// LLM input tokens consumed.
    pub input_tokens: usize,
    /// LLM output tokens consumed.
    pub output_tokens: usize,
}

/// Return a mode-appropriate system prompt for delegation workers.
///
/// Tries to load from `PromptManager` system templates first, falls back to hardcoded defaults.
fn worker_system_prompt(
    mode: &str,
    team_name: Option<&str>,
    pm: Option<&cortex_kernel::PromptManager>,
) -> Option<String> {
    match mode {
        "readonly" => {
            let from_pm = pm.and_then(|p| p.get_system_template("agent-readonly"));
            Some(from_pm.unwrap_or_else(|| {
                cortex_kernel::prompt_manager::DEFAULT_AGENT_READONLY.to_string()
            }))
        }
        "full" => {
            let from_pm = pm.and_then(|p| p.get_system_template("agent-full"));
            Some(
                from_pm.unwrap_or_else(|| {
                    cortex_kernel::prompt_manager::DEFAULT_AGENT_FULL.to_string()
                }),
            )
        }
        "teammate" => {
            const TEAM_PLACEHOLDER: &str = "{team}";
            let team = team_name.unwrap_or("default");
            let from_pm = pm
                .and_then(|p| p.get_system_template("agent-teammate"))
                .map(|t| t.replace(TEAM_PLACEHOLDER, team));
            Some(from_pm.unwrap_or_else(|| {
                cortex_kernel::prompt_manager::DEFAULT_AGENT_TEAMMATE
                    .replace(TEAM_PLACEHOLDER, team)
            }))
        }
        _ => None,
    }
}

/// Execute the worker's LLM loop: single call for readonly, multi-iteration for full/teammate.
async fn run_worker_llm_loop(
    llm: &dyn LlmClient,
    prompt: &str,
    system_prompt: Option<&str>,
    config: &DelegationConfig,
    tools: Option<&ToolRegistry>,
    gate: Option<&dyn PermissionGate>,
) -> (String, Usage) {
    let mut messages: Vec<Message> = vec![Message::user(prompt)];
    let mut total_usage = Usage::default();

    // Build tool definitions if tools are available
    let tool_defs: Vec<serde_json::Value> =
        tools.map(ToolRegistry::definitions).unwrap_or_default();

    let max_iters = if tools.is_some() {
        config.max_iterations
    } else {
        1 // readonly: single LLM call
    };

    for _iteration in 0..max_iters {
        let request = LlmRequest {
            system: system_prompt,
            messages: &messages,
            tools: if tool_defs.is_empty() {
                None
            } else {
                Some(&tool_defs)
            },
            max_tokens: config.max_tokens,
            transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
            on_text: None,
        };

        let response: LlmResponse = match llm.complete(request).await {
            Ok(r) => r,
            Err(e) => {
                return (format!("LLM error: {e}"), total_usage);
            }
        };

        total_usage.input_tokens += response.usage.input_tokens;
        total_usage.output_tokens += response.usage.output_tokens;

        // No tool calls -> return text response
        if response.tool_calls.is_empty() {
            let text = response.text.unwrap_or_else(|| "[no response text]".into());
            return (text, total_usage);
        }

        // Process tool calls (full/teammate mode)
        if let (Some(tool_reg), Some(perm_gate)) = (tools, gate) {
            let mut assistant_blocks: Vec<cortex_types::ContentBlock> = Vec::new();
            let mut tool_result_blocks: Vec<cortex_types::ContentBlock> = Vec::new();

            for tc in &response.tool_calls {
                assistant_blocks.push(cortex_types::ContentBlock::ToolUse {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    input: tc.input.clone(),
                });

                let result = execute_worker_tool(tool_reg, perm_gate, tc);
                tool_result_blocks.push(cortex_types::ContentBlock::ToolResult {
                    tool_use_id: tc.id.clone(),
                    content: result.output,
                    is_error: result.is_error,
                });
            }

            messages.push(Message {
                role: cortex_types::Role::Assistant,
                content: assistant_blocks,
                attachments: Vec::new(),
            });
            messages.push(Message {
                role: cortex_types::Role::User,
                content: tool_result_blocks,
                attachments: Vec::new(),
            });
        } else {
            // No tools available but LLM returned tool calls -- extract text if any
            let text = response
                .text
                .unwrap_or_else(|| "[tool calls returned but no tools available]".into());
            return (text, total_usage);
        }
    }

    // Max iterations reached
    ("[max iterations reached]".into(), total_usage)
}

/// Execute a single tool call within a worker, checking permissions.
fn execute_worker_tool(
    tools: &ToolRegistry,
    gate: &dyn PermissionGate,
    tc: &LlmToolCall,
) -> ToolResult {
    use crate::risk::RiskAssessor;
    let risk_assessor = RiskAssessor::default();
    let risk_level = risk_assessor.assess_level(&tc.name, &tc.input);
    let decision = gate.check(&tc.name, risk_level);

    match decision {
        cortex_types::PermissionDecision::Approved => tools.get(&tc.name).map_or_else(
            || ToolResult::error(format!("unknown tool: {}", tc.name)),
            |tool| match tool.execute(tc.input.clone()) {
                Ok(result) => result,
                Err(e) => ToolResult::error(format!("tool error: {e}")),
            },
        ),
        _ => ToolResult::error("permission denied"),
    }
}

/// Execute multiple tasks concurrently via `AgentPool` with LLM-driven workers.
///
/// Each worker calls the LLM to process its prompt and returns the LLM's response.
/// Readonly workers do a single LLM call; full/teammate workers support tool loops.
pub async fn delegate_tasks(
    tasks: Vec<TaskDelegation>,
    llm: Arc<dyn LlmClient>,
    config: DelegationConfig,
    tools: Arc<ToolRegistry>,
    gate: Arc<dyn PermissionGate>,
) -> Vec<DelegationResult> {
    if tasks.is_empty() {
        return Vec::new();
    }

    let mut pool = AgentPool::new();

    for task in &tasks {
        let llm = Arc::clone(&llm);
        let config = config.clone();
        let tools = Arc::clone(&tools);
        let gate = Arc::clone(&gate);
        let prompt = task.prompt.clone();
        let mode = task.mode.clone();
        let team_name = task.team_name.clone();

        let _ = pool.spawn_worker(task.name.clone(), move |_name, mut rx| async move {
            // Collect any additional messages routed to this worker
            let mut extra_messages = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                extra_messages.push(msg);
            }

            let sys_prompt = worker_system_prompt(&mode, team_name.as_deref(), None);

            // Append routed messages to prompt if any
            let full_prompt = if extra_messages.is_empty() {
                prompt
            } else {
                format!(
                    "{}\n\n[Additional context: {}]",
                    prompt,
                    extra_messages.join("; ")
                )
            };

            // Determine tool availability by mode
            let (tool_ref, gate_ref): (Option<&ToolRegistry>, Option<&dyn PermissionGate>) =
                match mode.as_str() {
                    "full" | "teammate" => (Some(&*tools), Some(&*gate)),
                    _ => (None, None), // readonly: no tools
                };

            let (output, usage) = run_worker_llm_loop(
                &*llm,
                &full_prompt,
                sys_prompt.as_deref(),
                &config,
                tool_ref,
                gate_ref,
            )
            .await;

            // Encode usage into output via a parseable suffix
            format!(
                "{}\n__USAGE__:{}:{}",
                output, usage.input_tokens, usage.output_tokens
            )
        });
    }

    let worker_results = pool.wait_all().await;
    worker_results_to_delegation(worker_results)
}

fn worker_results_to_delegation(results: Vec<WorkerResult>) -> Vec<DelegationResult> {
    results
        .into_iter()
        .map(|wr| {
            let (output, input_tokens, output_tokens) = parse_usage_suffix(&wr.output);
            let success =
                !output.starts_with("LLM error:") && !output.starts_with("worker panicked:");
            DelegationResult {
                name: wr.name,
                output,
                success,
                input_tokens,
                output_tokens,
            }
        })
        .collect()
}

/// Parse the `__USAGE__` suffix appended by workers to extract usage stats.
fn parse_usage_suffix(raw: &str) -> (String, usize, usize) {
    if let Some(idx) = raw.rfind("\n__USAGE__:") {
        let prefix = &raw[..idx];
        let suffix = &raw[idx + "\n__USAGE__:".len()..];
        if let Some((inp, out)) = suffix.split_once(':') {
            let input_tokens = inp.parse().unwrap_or(0);
            let output_tokens = out.parse().unwrap_or(0);
            return (prefix.to_string(), input_tokens, output_tokens);
        }
    }
    (raw.to_string(), 0, 0)
}

/// Aggregate delegation results into a structured summary string.
#[must_use]
pub fn aggregate_results(results: &[DelegationResult]) -> String {
    use std::fmt::Write;

    if results.is_empty() {
        return "No tasks delegated.".into();
    }

    let total = results.len();
    let succeeded = results.iter().filter(|r| r.success).count();
    let failed = total - succeeded;
    let total_input: usize = results.iter().map(|r| r.input_tokens).sum();
    let total_output: usize = results.iter().map(|r| r.output_tokens).sum();
    let mut summary = format!("Delegation summary: {succeeded}/{total} tasks succeeded");
    if failed > 0 {
        let _ = write!(summary, ", {failed} failed");
    }
    let _ = write!(summary, " (tokens: {total_input}in/{total_output}out)");
    summary.push('\n');

    for r in results {
        let status = if r.success { "OK" } else { "FAILED" };
        let _ = write!(summary, "\n[{status}] {}: {}\n", r.name, r.output);
    }

    summary
}
