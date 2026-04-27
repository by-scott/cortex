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
    /// Canonical actor identity for tool-visibility filtering.
    pub actor: Option<String>,
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_tokens: 2048,
            max_iterations: 10,
            actor: None,
        }
    }
}

/// Contract that bounds one delegated worker.
///
/// A worker never inherits broad parent authority by default. Tool access,
/// evidence access, budgets, artifact expectations, and merge review are stated
/// explicitly so delegation can be reviewed and replayed as a controlled action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationContract {
    pub scope: String,
    pub allowed_tools: Vec<String>,
    pub forbidden_actions: Vec<String>,
    pub token_budget: usize,
    pub iteration_budget: usize,
    pub evidence_budget: usize,
    pub allowed_evidence: Vec<String>,
    pub expected_artifact: String,
    pub merge_verifier: String,
    pub review_required: bool,
    pub inherit_parent_authority: bool,
}

impl DelegationContract {
    const DEFAULT_TOKEN_BUDGET: usize = 2048;
    const DEFAULT_ITERATION_BUDGET: usize = 1;

    #[must_use]
    pub fn readonly(scope: impl Into<String>, expected_artifact: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            allowed_tools: Vec::new(),
            forbidden_actions: default_forbidden_actions(),
            token_budget: Self::DEFAULT_TOKEN_BUDGET,
            iteration_budget: Self::DEFAULT_ITERATION_BUDGET,
            evidence_budget: 0,
            allowed_evidence: Vec::new(),
            expected_artifact: expected_artifact.into(),
            merge_verifier: "parent_review".to_string(),
            review_required: true,
            inherit_parent_authority: false,
        }
    }

    #[must_use]
    pub fn with_allowed_tool(mut self, tool: impl Into<String>) -> Self {
        self.allowed_tools.push(tool.into());
        self
    }

    #[must_use]
    pub fn with_forbidden_action(mut self, action: impl Into<String>) -> Self {
        self.forbidden_actions.push(action.into());
        self
    }

    #[must_use]
    pub const fn with_token_budget(mut self, budget: usize) -> Self {
        self.token_budget = budget;
        self
    }

    #[must_use]
    pub const fn with_iteration_budget(mut self, budget: usize) -> Self {
        self.iteration_budget = budget;
        self
    }

    #[must_use]
    pub const fn with_evidence_budget(mut self, budget: usize) -> Self {
        self.evidence_budget = budget;
        self
    }

    #[must_use]
    pub fn with_allowed_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.allowed_evidence.push(evidence.into());
        self
    }

    #[must_use]
    pub fn with_merge_verifier(mut self, verifier: impl Into<String>) -> Self {
        self.merge_verifier = verifier.into();
        self
    }

    #[must_use]
    pub const fn with_review_required(mut self, required: bool) -> Self {
        self.review_required = required;
        self
    }

    #[must_use]
    pub const fn with_parent_authority_inheritance(mut self, inherit: bool) -> Self {
        self.inherit_parent_authority = inherit;
        self
    }

    #[must_use]
    pub fn permits_tool(&self, tool: &str) -> bool {
        self.allowed_tools.iter().any(|allowed| allowed == tool)
            && !self
                .forbidden_actions
                .iter()
                .any(|forbidden| forbidden == tool)
    }

    /// Validate this contract before a worker is started.
    ///
    /// # Errors
    /// Returns `DelegationContractError` when a required contract field is
    /// missing, a budget is zero, or authority inheritance is too broad.
    pub fn validate(&self) -> Result<(), DelegationContractError> {
        if self.scope.trim().is_empty() {
            return Err(DelegationContractError::MissingScope);
        }
        if self.expected_artifact.trim().is_empty() {
            return Err(DelegationContractError::MissingExpectedArtifact);
        }
        if self.merge_verifier.trim().is_empty() {
            return Err(DelegationContractError::MissingMergeVerifier);
        }
        if self.token_budget == 0 {
            return Err(DelegationContractError::ZeroTokenBudget);
        }
        if self.iteration_budget == 0 {
            return Err(DelegationContractError::ZeroIterationBudget);
        }
        if self.inherit_parent_authority && self.allowed_tools.is_empty() {
            return Err(DelegationContractError::BroadAuthorityInheritance);
        }
        Ok(())
    }

    #[must_use]
    pub fn worker_prompt(&self, task_prompt: &str, extra_messages: &[String]) -> String {
        let mut prompt = format!(
            "Delegation contract:\n\
             - scope: {}\n\
             - allowed_tools: {}\n\
             - forbidden_actions: {}\n\
             - token_budget: {}\n\
             - iteration_budget: {}\n\
             - evidence_budget: {}\n\
             - allowed_evidence: {}\n\
             - expected_artifact: {}\n\
             - merge_verifier: {}\n\
             - review_required: {}\n\
             - inherit_parent_authority: {}\n\n\
             Task:\n{}",
            self.scope,
            list_or_none(&self.allowed_tools),
            list_or_none(&self.forbidden_actions),
            self.token_budget,
            self.iteration_budget,
            self.evidence_budget,
            list_or_none(&self.allowed_evidence),
            self.expected_artifact,
            self.merge_verifier,
            self.review_required,
            self.inherit_parent_authority,
            task_prompt
        );
        if !extra_messages.is_empty() {
            prompt.push_str("\n\nAdditional routed context:\n");
            prompt.push_str(&extra_messages.join("\n"));
        }
        prompt
    }
}

impl Default for DelegationContract {
    fn default() -> Self {
        Self::readonly("bounded investigation", "written answer")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationContractError {
    MissingScope,
    MissingExpectedArtifact,
    MissingMergeVerifier,
    ZeroTokenBudget,
    ZeroIterationBudget,
    BroadAuthorityInheritance,
    ToolNotAllowed(String),
}

impl std::fmt::Display for DelegationContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingScope => write!(f, "delegation contract missing scope"),
            Self::MissingExpectedArtifact => {
                write!(f, "delegation contract missing expected artifact")
            }
            Self::MissingMergeVerifier => write!(f, "delegation contract missing merge verifier"),
            Self::ZeroTokenBudget => write!(f, "delegation contract token budget is zero"),
            Self::ZeroIterationBudget => write!(f, "delegation contract iteration budget is zero"),
            Self::BroadAuthorityInheritance => {
                write!(
                    f,
                    "delegation contract inherits parent authority too broadly"
                )
            }
            Self::ToolNotAllowed(tool) => {
                write!(f, "tool '{tool}' is outside the delegation contract")
            }
        }
    }
}

impl std::error::Error for DelegationContractError {}

fn default_forbidden_actions() -> Vec<String> {
    [
        "write",
        "edit",
        "bash",
        "cron",
        "send_media",
        "memory_save",
        "deploy",
        "publish",
        "credential",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

/// A structured task to delegate to a worker.
#[derive(Debug, Clone)]
pub struct TaskDelegation {
    /// Unique name for this task.
    pub name: String,
    /// The prompt/instruction for the worker.
    pub prompt: String,
    /// Execution mode: "readonly", "full", "fork", "teammate".
    pub mode: String,
    /// Team name for teammate mode.
    pub team_name: Option<String>,
    /// Explicit authority, evidence, budget, and merge contract.
    pub contract: DelegationContract,
}

impl TaskDelegation {
    /// Create a new task delegation with default readonly mode.
    pub fn new(name: impl Into<String>, prompt: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            prompt: prompt.into(),
            mode: "readonly".into(),
            team_name: None,
            contract: DelegationContract::default(),
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

    #[must_use]
    pub fn with_contract(mut self, contract: DelegationContract) -> Self {
        self.contract = contract;
        self
    }
}

/// Result from a delegated task.
#[derive(Debug, Clone)]
pub struct DelegationResult {
    /// Task name (matches `TaskDelegation::name`).
    pub name: String,
    /// Output from the delegated worker.
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
            let from_pm = pm.and_then(|p| p.get_system_template("worker-readonly"));
            Some(from_pm.unwrap_or_else(|| {
                cortex_kernel::prompt_manager::DEFAULT_WORKER_READONLY.to_string()
            }))
        }
        "full" => {
            let from_pm = pm.and_then(|p| p.get_system_template("worker-full"));
            Some(
                from_pm.unwrap_or_else(|| {
                    cortex_kernel::prompt_manager::DEFAULT_WORKER_FULL.to_string()
                }),
            )
        }
        "teammate" => {
            const TEAM_PLACEHOLDER: &str = "{team}";
            let team = team_name.unwrap_or("default");
            let from_pm = pm
                .and_then(|p| p.get_system_template("worker-teammate"))
                .map(|t| t.replace(TEAM_PLACEHOLDER, team));
            Some(from_pm.unwrap_or_else(|| {
                cortex_kernel::prompt_manager::DEFAULT_WORKER_TEAMMATE
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
    contract: &DelegationContract,
    tools: Option<&ToolRegistry>,
    gate: Option<&dyn PermissionGate>,
) -> (String, Usage) {
    let mut messages: Vec<Message> = vec![Message::user(prompt)];
    let mut total_usage = Usage::default();

    // Build tool definitions if tools are available
    let tool_defs = delegation_tool_defs(tools, config.actor.as_deref(), contract);

    let max_iters = if tools.is_some() {
        config.max_iterations.min(contract.iteration_budget)
    } else {
        1 // readonly: single LLM call
    };
    let max_tokens = config.max_tokens.min(contract.token_budget);

    for _iteration in 0..max_iters {
        let request = LlmRequest {
            system: system_prompt,
            messages: &messages,
            tools: if tool_defs.is_empty() {
                None
            } else {
                Some(&tool_defs)
            },
            max_tokens,
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

                let result = execute_worker_tool(tool_reg, perm_gate, contract, tc);
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

fn delegation_tool_defs(
    tools: Option<&ToolRegistry>,
    actor: Option<&str>,
    contract: &DelegationContract,
) -> Vec<serde_json::Value> {
    tools
        .map(|registry| registry.definitions_for_actor(actor))
        .map(|definitions| {
            definitions
                .into_iter()
                .filter(|definition| {
                    definition
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|name| contract.permits_tool(name))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Execute a single tool call within a worker, checking permissions.
fn execute_worker_tool(
    tools: &ToolRegistry,
    gate: &dyn PermissionGate,
    contract: &DelegationContract,
    tc: &LlmToolCall,
) -> ToolResult {
    use crate::risk::RiskAssessor;
    if !contract.permits_tool(&tc.name) {
        return ToolResult::error(
            DelegationContractError::ToolNotAllowed(tc.name.clone()).to_string(),
        );
    }
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
    let mut rejected = Vec::new();

    for task in &tasks {
        if let Err(error) = task.contract.validate() {
            rejected.push(DelegationResult {
                name: task.name.clone(),
                output: error.to_string(),
                success: false,
                input_tokens: 0,
                output_tokens: 0,
            });
            continue;
        }

        let llm = Arc::clone(&llm);
        let config = config.clone();
        let tools = Arc::clone(&tools);
        let gate = Arc::clone(&gate);
        let prompt = task.prompt.clone();
        let mode = task.mode.clone();
        let team_name = task.team_name.clone();
        let contract = task.contract.clone();

        let _ = pool.spawn_worker(task.name.clone(), move |_name, mut rx| async move {
            // Collect any additional messages routed to this worker
            let mut extra_messages = Vec::new();
            while let Ok(msg) = rx.try_recv() {
                extra_messages.push(msg);
            }

            let sys_prompt = worker_system_prompt(&mode, team_name.as_deref(), None);

            let full_prompt = contract.worker_prompt(&prompt, &extra_messages);

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
                &contract,
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
    rejected.extend(worker_results_to_delegation(worker_results));
    rejected
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

#[cfg(test)]
mod tests {
    use super::{
        DelegationConfig, DelegationContract, DelegationContractError, delegation_tool_defs,
    };
    use crate::tools::{Tool, ToolError, ToolRegistry, ToolResult};

    struct NamedTool(&'static str);

    impl Tool for NamedTool {
        fn name(&self) -> &'static str {
            self.0
        }

        fn description(&self) -> &'static str {
            "test tool"
        }

        fn input_schema(&self) -> serde_json::Value {
            serde_json::json!({ "type": "object" })
        }

        fn execute(&self, _input: serde_json::Value) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::success("ok".to_string()))
        }
    }

    #[test]
    fn delegation_filters_local_operator_only_tools_for_non_local_actors() {
        let mut registry = ToolRegistry::new();
        for name in ["audit", "prompt_inspect", "memory_graph", "read"] {
            registry.register(Box::new(NamedTool(name)));
        }
        let contract = DelegationContract::readonly("inspect repository", "findings")
            .with_allowed_tool("audit")
            .with_allowed_tool("prompt_inspect")
            .with_allowed_tool("memory_graph")
            .with_allowed_tool("read");

        let names = tool_names(delegation_tool_defs(
            Some(&registry),
            Some("user:scott"),
            &contract,
        ));
        assert!(
            !names.iter().any(|name| {
                matches!(name.as_str(), "audit" | "prompt_inspect" | "memory_graph")
            }),
            "delegation should hide self-introspection tools for non-local actors: {names:?}"
        );
        assert!(names.iter().any(|name| name == "read"));
    }

    #[test]
    fn delegation_config_defaults_to_no_actor_override() {
        let config = DelegationConfig::default();
        assert!(config.actor.is_none());
    }

    #[test]
    fn delegation_contract_filters_tools_and_validates_budgets() {
        let mut registry = ToolRegistry::new();
        for name in ["read", "bash", "write"] {
            registry.register(Box::new(NamedTool(name)));
        }
        let contract = DelegationContract::readonly("read source files", "summary")
            .with_allowed_tool("read")
            .with_forbidden_action("bash")
            .with_token_budget(512)
            .with_iteration_budget(2)
            .with_allowed_evidence("src/**/*.rs");

        assert!(contract.validate().is_ok());
        assert!(contract.permits_tool("read"));
        assert!(!contract.permits_tool("bash"));
        assert!(!contract.permits_tool("write"));

        let names = tool_names(delegation_tool_defs(
            Some(&registry),
            Some("local:operator"),
            &contract,
        ));
        assert_eq!(names, vec!["read".to_string()]);
    }

    #[test]
    fn delegation_contract_rejects_broad_authority_inheritance() {
        let contract = DelegationContract::readonly("implement isolated change", "patch")
            .with_parent_authority_inheritance(true);

        assert_eq!(
            contract.validate(),
            Err(DelegationContractError::BroadAuthorityInheritance)
        );
    }

    fn tool_names(definitions: Vec<serde_json::Value>) -> Vec<String> {
        definitions
            .into_iter()
            .filter_map(|def| {
                def.get("name")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_string)
            })
            .collect()
    }
}
