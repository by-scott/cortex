use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Message, Payload, TurnId};

use crate::agent_pool::delegation::DelegationContract;
use crate::llm::LlmClient;
use crate::risk::PermissionGate;
use crate::tools::ToolRegistry;

use super::super::{MAX_AGENT_DEPTH, NullTracer, StreamLane, TurnConfig, TurnContext};
use super::super::{TurnError, TurnResult, TurnStreamEvent, journal_append, run_turn};
use super::events;
use super::subturn_contract::{self, AgentSubTurnMode};
use super::tool_batch::{ExecutionResult, SkillExecutionPlan, record_skill_execution_trace};

pub(super) struct AgentSubTurnParams<'a> {
    pub(super) input: &'a serde_json::Value,
    pub(super) parent_config: &'a TurnConfig,
    pub(super) llm: &'a dyn LlmClient,
    pub(super) journal: &'a Journal,
    pub(super) gate: &'a dyn PermissionGate,
    pub(super) parent_history: &'a [Message],
    pub(super) on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
    pub(super) prompt_manager: Option<&'a cortex_kernel::PromptManager>,
}

pub(super) struct SkillSubTurnParams<'a> {
    pub(super) plan: &'a SkillExecutionPlan,
    pub(super) skill_registry: &'a crate::skills::SkillRegistry,
    pub(super) parent_config: &'a TurnConfig,
    pub(super) llm: &'a dyn LlmClient,
    pub(super) journal: &'a Journal,
    pub(super) gate: &'a dyn PermissionGate,
    pub(super) on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
}

type EventCallback<'a> = &'a (dyn Fn(&TurnStreamEvent) + Send + Sync);

enum SubTurnKind {
    Agent {
        description: String,
        mode: AgentSubTurnMode,
        contract: DelegationContract,
    },
    Skill {
        name: String,
    },
}

impl SubTurnKind {
    fn observer_label(&self) -> String {
        match self {
            Self::Agent { description, .. } => format!("worker:{description}"),
            Self::Skill { name } => format!("skill:{name}"),
        }
    }

    fn success_fallback(&self) -> String {
        match self {
            Self::Agent {
                description, mode, ..
            } => {
                format!("[Worker '{description}' ({mode} mode)] completed with no text response")
            }
            Self::Skill { name } => format!("[Skill '{name}' (fork)] completed"),
        }
    }

    fn failure_prefix(&self) -> String {
        match self {
            Self::Agent { description, .. } => format!("worker '{description}' failed"),
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
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("default");
                let template = prompt_manager
                    .and_then(|pm| pm.get_system_template("worker-teammate"))
                    .unwrap_or_else(|| {
                        cortex_kernel::prompt_manager::DEFAULT_WORKER_TEAMMATE.to_string()
                    });
                Some(template.replace(TEAM_PLACEHOLDER, team_name))
            }
            Self::Agent { .. } | Self::Skill { .. } => None,
        };

        let contract_limits = match self {
            Self::Agent { contract, .. } => Some(contract),
            Self::Skill { .. } => None,
        };

        TurnConfig {
            system_prompt,
            dynamic_context: parent_config.dynamic_context.clone(),
            max_tokens: contract_limits.map_or(parent_config.max_tokens, |contract| {
                parent_config.max_tokens.min(contract.token_budget)
            }),
            agent_depth: parent_config.agent_depth + 1,
            working_memory_capacity: parent_config.working_memory_capacity,
            max_tool_iterations: contract_limits.map_or(
                parent_config.max_tool_iterations,
                |contract| {
                    parent_config
                        .max_tool_iterations
                        .min(contract.iteration_budget)
                },
            ),
            auto_extract: false,
            extract_min_turns: parent_config.extract_min_turns,
            reconsolidation_memories: Vec::new(),
            turns_since_extract: 0,
            tool_timeout_secs: parent_config.tool_timeout_secs,
            llm_transient_retries: parent_config.llm_transient_retries,
            strip_think_tags: parent_config.strip_think_tags,
            evolution_weights: parent_config.evolution_weights,
            pressure_thresholds: parent_config.pressure_thresholds,
            metacognition: parent_config.metacognition.clone(),
            risk: parent_config.risk.clone(),
            trace: parent_config.trace.clone(),
            session_id: parent_config.session_id.clone(),
            actor: parent_config.actor.clone(),
            source: parent_config.source.clone(),
            execution_scope: parent_config.execution_scope,
            protected_runtime_roots: parent_config.protected_runtime_roots.clone(),
        }
    }

    fn build_tools(&self, current_depth: usize) -> ToolRegistry {
        let can_recurse_agent = current_depth + 1 < MAX_AGENT_DEPTH;
        let mut registry = ToolRegistry::new();
        let permits = |name: &str| match self {
            Self::Agent { contract, .. } => contract.permits_tool(name),
            Self::Skill { .. } => true,
        };

        if permits("read") {
            registry.register(Box::new(crate::tools::read::ReadTool));
        }
        if permits("write") {
            registry.register(Box::new(crate::tools::write::WriteTool));
        }
        if permits("edit") {
            registry.register(Box::new(crate::tools::edit::EditTool));
        }
        if permits("bash") {
            registry.register(Box::new(crate::tools::bash::BashTool));
        }

        let execution_allows_agent = match self {
            Self::Agent {
                mode: AgentSubTurnMode::Readonly,
                ..
            } => false,
            Self::Agent { .. } | Self::Skill { .. } => true,
        };
        if execution_allows_agent && can_recurse_agent && permits("agent") {
            registry.register(Box::new(crate::tools::agent::AgentTool));
        }

        registry
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
        TurnStreamEvent::Text { content, .. } => events::emit_text_event(
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

async fn run_observed_sub_turn(params: ObservedSubTurnParams<'_>) -> Result<TurnResult, TurnError> {
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

    run_turn(sub_ctx).await
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

pub(super) async fn execute_agent_sub_turn(params: AgentSubTurnParams<'_>) -> ExecutionResult {
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
    let Some(prompt) = input.get("prompt").and_then(serde_json::Value::as_str) else {
        return ExecutionResult {
            output: "delegated worker: missing prompt".to_string(),
            media: Vec::new(),
            is_error: true,
        };
    };

    let mode = AgentSubTurnMode::parse(input.get("mode").and_then(serde_json::Value::as_str));

    let description = input
        .get("description")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("delegated worker");
    let contract = match subturn_contract::agent_delegation_contract(input, description) {
        Ok(contract) => contract,
        Err(error) => {
            return ExecutionResult {
                output: format!("delegated worker '{description}': {error}"),
                media: Vec::new(),
                is_error: true,
            };
        }
    };

    if parent_config.agent_depth >= MAX_AGENT_DEPTH {
        return ExecutionResult {
            output: format!(
                "delegated worker '{description}': max recursion depth ({MAX_AGENT_DEPTH}) exceeded"
            ),
            media: Vec::new(),
            is_error: true,
        };
    }

    let kind = SubTurnKind::Agent {
        description: description.to_string(),
        mode,
        contract: contract.clone(),
    };
    let worker_prompt = contract.worker_prompt(prompt, &[]);
    launch_sub_turn(SubTurnLaunch {
        kind,
        input: &worker_prompt,
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

pub(super) async fn execute_skill_sub_turn(params: SkillSubTurnParams<'_>) -> ExecutionResult {
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
    let start = std::time::Instant::now();
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
    let duration_ms = u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX);
    skill_registry.record_outcome(&plan.name, !result.is_error);
    record_skill_execution_trace(
        skill_registry,
        plan,
        cortex_types::InvocationTrigger::AgentAutonomous.to_string(),
        duration_ms,
        &result,
    );
    result
}
