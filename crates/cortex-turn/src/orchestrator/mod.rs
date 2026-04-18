pub(crate) mod dmn;
pub mod perf;
pub mod post_turn;
pub mod resume;
pub(crate) mod sn;
pub(crate) mod tpn;

use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Event, Message, Payload, TurnId, TurnState};

use crate::context::ContextBuilder;
use crate::llm::LlmClient;
use crate::risk::{DenialTracker, PermissionGate, RiskAssessor};
use crate::tools::ToolRegistry;

pub use tpn::{ToolProgress, ToolProgressStatus};

pub const MAX_AGENT_DEPTH: usize = 3;

/// Strip `<think>…</think>` blocks and orphaned `</think>` prefixes from LLM
/// output.  Only applied to assistant responses so user-authored `<think>` text
/// is never touched.
/// Regex for matching `<think>…</think>` blocks.
static RE_THINK_BLOCK: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
    regex::Regex::new(r"(?s)<think>.*?</think>\s*").expect("valid regex")
});

pub(crate) fn strip_think_tags(text: &str) -> String {
    // 0. Some models (e.g. ZhipuAI glm-4.7) wrap output in a JSON object
    //    with "thought" and "response" fields.  Extract just the response.
    let text = extract_json_response(text);

    // 1. Remove complete <think>…</think> blocks (greedy-minimal across lines).
    let text = RE_THINK_BLOCK.replace_all(&text, "");

    // 2. Remove orphaned </think> that appears at the very start (model
    //    sometimes emits a closing tag without the opening one).
    let text = text.strip_prefix("</think>").unwrap_or(&text);

    text.trim().to_string()
}

/// If the text looks like a JSON object with a `"response"` key, extract that value.
/// Handles models that wrap output as `{"thought": "...", "response": "..."}`.
fn extract_json_response(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(response) = obj.get("response").and_then(serde_json::Value::as_str)
    {
        return response.to_string();
    }
    text.to_string()
}

/// Category tags for turn execution trace events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceCategory {
    /// SN/TPN/DMN phase transitions.
    Phase,
    /// LLM calls (model, tokens, cost).
    Llm,
    /// Tool dispatch and results.
    Tool,
    /// Metacognition: doom loop, fatigue, frame anchoring, confidence.
    Meta,
    /// Memory extraction, recall, consolidation.
    Memory,
    /// Context pressure and compression events.
    Context,
}

/// Callback interface for turn execution tracing.
///
/// Implementations receive structured trace events during turn execution.
/// The daemon provides a `tracing`-based implementation; SSE streaming
/// adds a second implementation that emits trace events to the client.
pub trait TurnTracer: Send + Sync {
    /// Emit a trace event if the category is enabled (legacy, maps to `Basic`).
    fn trace(&self, category: TraceCategory, message: &str) {
        self.trace_at(category, cortex_types::TraceLevel::Basic, message);
    }

    /// Emit a trace event at a specific detail level.
    ///
    /// Implementations should check whether the category's effective level
    /// is `>=` the requested level before emitting.
    fn trace_at(&self, category: TraceCategory, level: cortex_types::TraceLevel, message: &str);
}

/// No-op tracer that discards all trace events.
pub struct NullTracer;

impl TurnTracer for NullTracer {
    fn trace_at(&self, _category: TraceCategory, _level: cortex_types::TraceLevel, _message: &str) {
    }
}

pub struct TurnConfig {
    pub system_prompt: Option<String>,
    pub max_tokens: usize,
    pub agent_depth: usize,
    pub working_memory_capacity: usize,
    pub max_tool_iterations: usize,
    pub auto_extract: bool,
    pub extract_min_turns: usize,
    /// How many turns have elapsed since last extraction.
    /// The caller is responsible for tracking and passing this value.
    pub turns_since_extract: usize,
    /// Global tool execution timeout in seconds (from config).
    pub tool_timeout_secs: u64,
    /// Whether to strip `<think>…</think>` tags from final output.
    pub strip_think_tags: bool,
    /// Evolution signal weights (6 signals, configurable).
    pub evolution_weights: [f64; 6],
    /// Context pressure thresholds for Normal/Alert/Compress/Urgent/Degrade.
    pub pressure_thresholds: [f64; 4],
    /// Metacognition subsystem configuration.
    pub metacognition: cortex_types::config::MetacognitionConfig,
    /// Per-category trace switches.
    pub trace: cortex_types::config::TurnTraceConfig,
}

impl Default for TurnConfig {
    fn default() -> Self {
        let defaults = cortex_types::config::TurnSection::default();
        Self {
            system_prompt: None,
            max_tokens: cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK,
            agent_depth: 0,
            working_memory_capacity: 5,
            max_tool_iterations: defaults.max_tool_iterations,
            auto_extract: true,
            extract_min_turns: 5,
            turns_since_extract: 0,
            tool_timeout_secs: defaults.tool_timeout_secs,
            strip_think_tags: defaults.strip_think_tags,
            evolution_weights: [1.0, 0.8, 0.6, 0.5, 0.4, 0.3],
            pressure_thresholds: [0.60, 0.75, 0.85, 0.95],
            metacognition: cortex_types::config::MetacognitionConfig::default(),
            trace: cortex_types::config::TurnTraceConfig::default(),
        }
    }
}

pub struct TurnResult {
    pub response_text: Option<String>,
    pub state: TurnState,
    pub events: Vec<Payload>,
    pub prompt_updates: Vec<(cortex_types::PromptLayer, String)>,
    pub entity_relations: Vec<cortex_types::MemoryRelation>,
    /// Memories extracted from the conversation (to be saved by the caller).
    pub extracted_memories: Vec<cortex_types::MemoryEntry>,
}

pub struct TurnContext<'a> {
    pub input: &'a str,
    pub history: &'a mut Vec<Message>,
    pub llm: &'a dyn LlmClient,
    pub tools: &'a ToolRegistry,
    pub journal: &'a Journal,
    pub gate: &'a dyn PermissionGate,
    pub config: &'a TurnConfig,
    pub on_text: Option<&'a (dyn Fn(&str) + Send + Sync)>,
    pub on_tool_progress: Option<&'a (dyn Fn(&ToolProgress) + Send + Sync)>,
    pub images: Vec<(String, String)>,
    pub compress_template: Option<String>,
    pub summary_cache: Option<&'a mut crate::context::SummaryCache>,
    pub prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    pub skill_registry: Option<&'a crate::skills::SkillRegistry>,
    /// Optional lightweight LLM for post-turn sub-endpoints (extraction, compression, etc.).
    /// Falls back to `llm` if `None`.
    pub post_turn_llm: Option<&'a dyn LlmClient>,
    /// Turn execution tracer for external observability.
    pub tracer: &'a dyn TurnTracer,
    /// External cancellation flag — when set to `true`, the TPN loop exits
    /// early at the next iteration boundary.
    pub cancel_flag: Option<std::sync::Arc<std::sync::atomic::AtomicBool>>,
    /// Shared inbox for mid-turn message injection.  External callers push
    /// messages here; the TPN loop drains them at each iteration and appends
    /// them as user messages to the conversation history.
    pub message_inbox: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
}

#[derive(Debug)]
pub enum TurnError {
    StateTransition(String),
    LlmError(String),
}

impl std::fmt::Display for TurnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::StateTransition(e) => write!(f, "state transition error: {e}"),
            Self::LlmError(e) => write!(f, "LLM error: {e}"),
        }
    }
}

impl std::error::Error for TurnError {}

// ── Public entry point ─────────────────────────────────────

/// Execute a complete turn: `SN` -> `TPN` loop -> `DMN` phase.
///
/// # Errors
///
/// Returns `TurnError::StateTransition` if the turn state machine
/// rejects a transition, or `TurnError::LlmError` if an LLM call fails.
#[must_use]
pub fn run_turn(
    ctx: TurnContext<'_>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<TurnResult, TurnError>> + Send + '_>>
{
    Box::pin(run_turn_inner(ctx))
}

/// Trace SN phase result: working memory activation count.
fn trace_sn_result(
    _config: &TurnConfig,
    tracer: &dyn TurnTracer,
    working_mem: &crate::working_memory::WorkingMemoryManager,
) {
    tracer.trace_at(
        TraceCategory::Context,
        cortex_types::TraceLevel::Basic,
        &format!(
            "Working memory: {} items activated",
            working_mem.active_count()
        ),
    );
}

/// Record TPN latency metric and emit DMN phase trace.
fn record_tpn_latency(
    tpn_start: std::time::Instant,
    journal: &Journal,
    _config: &TurnConfig,
    tracer: &dyn TurnTracer,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) {
    let tpn_ms = tpn_start.elapsed().as_millis();
    let ev = Payload::MetaControlApplied {
        action: format!("turn_latency: TPN={tpn_ms}ms"),
    };
    journal_append(journal, turn_id, corr_id, &ev);
    events_log.push(ev);
    tracer.trace_at(
        TraceCategory::Phase,
        cortex_types::TraceLevel::Minimal,
        "DMN phase: default mode network",
    );
}

/// Record wall-clock timestamp and SN phase trace before SN initialization.
fn init_turn_trace(
    journal: &Journal,
    _config: &TurnConfig,
    tracer: &dyn TurnTracer,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) {
    let wc = Payload::SideEffectRecorded {
        kind: cortex_types::SideEffectKind::WallClock,
        key: "turn_start".into(),
        value: chrono::Utc::now().to_rfc3339(),
    };
    journal_append(journal, turn_id, corr_id, &wc);
    events_log.push(wc);
    tracer.trace_at(
        TraceCategory::Phase,
        cortex_types::TraceLevel::Minimal,
        "SN phase: sensory network initialization",
    );
}

fn trace_phase(tracer: &dyn TurnTracer, name: &str) {
    tracer.trace_at(
        TraceCategory::Phase,
        cortex_types::TraceLevel::Minimal,
        name,
    );
}

async fn run_turn_inner(ctx: TurnContext<'_>) -> Result<TurnResult, TurnError> {
    let TurnContext {
        input,
        history,
        llm,
        tools,
        journal,
        gate,
        config,
        on_text,
        on_tool_progress,
        images,
        compress_template,
        summary_cache,
        prompt_manager,
        skill_registry,
        post_turn_llm,
        tracer,
        cancel_flag,
        message_inbox,
    } = ctx;
    let mut sc_owned = crate::context::SummaryCache::new();
    let summary_cache = summary_cache.unwrap_or(&mut sc_owned);
    let (turn_id, corr_id) = (TurnId::new(), CorrelationId::new());
    let mut events_log = Vec::new();
    let state = begin_turn(input, journal, turn_id, corr_id, &mut events_log)?;
    init_turn_trace(journal, config, tracer, turn_id, corr_id, &mut events_log);
    let (mut confidence, mut meta_monitor, mut working_mem, mut scheduler, mut reasoning_engine) =
        sn::init_turn_state(input, config, journal, turn_id, corr_id, &mut events_log);
    trace_sn_result(config, tracer, &working_mem);
    let system_prompt = config
        .system_prompt
        .clone()
        .or_else(|| ContextBuilder::new().build());
    push_user_message(history, input, images);
    let tool_defs = tools.definitions();
    trace_phase(tracer, "TPN");
    let tpn_start = std::time::Instant::now();
    let final_text = tpn::run_tpn_loop(&mut tpn::TpnLoopContext {
        history,
        llm,
        tools,
        journal,
        gate,
        config,
        on_text,
        on_tool_progress,
        compress_template: compress_template.as_ref(),
        summary_cache,
        system_prompt: system_prompt.as_ref(),
        tool_defs: &tool_defs,
        working_mem: &mut working_mem,
        scheduler: &mut scheduler,
        confidence: &mut confidence,
        meta_monitor: &mut meta_monitor,
        denial_tracker: &mut DenialTracker::new(config.metacognition.denial.clone()),
        risk_assessor: &RiskAssessor,
        reasoning_engine: &mut reasoning_engine,
        prompt_manager,
        skill_registry,
        turn_id,
        corr_id,
        events_log: &mut events_log,
        tracer,
        cancel_flag,
        message_inbox,
    })
    .await?;
    record_tpn_latency(
        tpn_start,
        journal,
        config,
        tracer,
        turn_id,
        corr_id,
        &mut events_log,
    );
    dmn::run_post_tpn_phase(dmn::PostTpnContext {
        state,
        final_text,
        reasoning_engine,
        confidence,
        meta_monitor,
        working_mem,
        events_log,
        prompt_manager,
        skill_registry,
        input,
        llm,
        post_turn_llm,
        history,
        config,
        journal,
        turn_id,
        corr_id,
        tracer,
    })
    .await
}

// ── Helpers ────────────────────────────────────────────────

fn begin_turn(
    input: &str,
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
) -> Result<TurnState, TurnError> {
    let state = TurnState::Idle
        .try_transition(TurnState::Processing)
        .map_err(|e| TurnError::StateTransition(e.to_string()))?;

    let payload = Payload::TurnStarted;
    journal_append(journal, turn_id, corr_id, &payload);
    events_log.push(payload);

    let payload = Payload::UserMessage {
        content: input.to_string(),
    };
    journal_append(journal, turn_id, corr_id, &payload);
    events_log.push(payload);

    Ok(state)
}

fn push_user_message(history: &mut Vec<Message>, input: &str, images: Vec<(String, String)>) {
    if images.is_empty() {
        history.push(Message::user(input));
    } else {
        history.push(Message::user_with_images(input, images));
    }
}

pub(crate) fn journal_append(
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    payload: &Payload,
) {
    let event = Event::new(turn_id, corr_id, payload.clone());
    let _ = journal.append(&event);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmResponse, LlmToolCall, MockLlmClient, Usage};
    use crate::risk::AutoApproveGate;
    use crate::tools::register_core_tools_basic;
    use cortex_types::TurnState;

    #[test]
    fn strip_think_complete_block() {
        let input = "<think>internal reasoning</think>Hello world";
        assert_eq!(strip_think_tags(input), "Hello world");
    }

    #[test]
    fn strip_think_multiline() {
        let input = "<think>\nstep 1\nstep 2\n</think>\nAnswer: 42";
        assert_eq!(strip_think_tags(input), "Answer: 42");
    }

    #[test]
    fn strip_think_orphaned_close() {
        let input = "</think>The actual response";
        assert_eq!(strip_think_tags(input), "The actual response");
    }

    #[test]
    fn strip_think_preserves_clean_text() {
        let input = "No think tags here";
        assert_eq!(strip_think_tags(input), "No think tags here");
    }

    #[test]
    fn strip_think_empty_block() {
        let input = "<think></think>Result";
        assert_eq!(strip_think_tags(input), "Result");
    }

    #[test]
    fn strip_think_json_thought_response() {
        let input = r#"{"thought": "thinking about it", "response": "The answer is 42"}"#;
        assert_eq!(strip_think_tags(input), "The answer is 42");
    }

    #[test]
    fn strip_think_json_preserves_non_response() {
        let input = r#"{"key": "value"}"#;
        assert_eq!(strip_think_tags(input), r#"{"key": "value"}"#);
    }

    fn setup_tools() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        register_core_tools_basic(&mut reg);
        reg
    }

    #[tokio::test]
    async fn pure_text_turn() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();
        mock_llm.push_text("Hello, I can help you!");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "hello",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert_eq!(result.response_text.unwrap(), "Hello, I can help you!");

        // Check journal has events
        let events = journal.recent_events(10).unwrap();
        assert!(events.len() >= 4); // TurnStarted, UserMessage, AssistantMessage, TurnCompleted
    }

    #[tokio::test]
    async fn turn_with_tool_call() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("test.txt");
        std::fs::write(&test_file, "file content here").unwrap();

        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_1".into(),
                name: "read".into(),
                input: serde_json::json!({"file_path": test_file.to_str().unwrap()}),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // Second response: text with tool result
        mock_llm.push_text("The file contains: file content here");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "read test.txt",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert!(result.response_text.unwrap().contains("file content here"));

        // History should have: user, assistant(tool_use), user(tool_result), assistant(text)
        assert_eq!(history.len(), 4);
    }

    #[tokio::test]
    async fn turn_tool_permission_denied() {
        use crate::risk::DefaultPermissionGate;
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        // LLM wants to call bash with dangerous command
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_1".into(),
                name: "bash".into(),
                input: serde_json::json!({"command": "rm -rf /"}),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // After denial, LLM responds with text
        mock_llm.push_text("I understand, that command is too dangerous.");

        let tools = setup_tools();
        let gate = DefaultPermissionGate; // This will block high-risk bash
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "delete everything",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);

        // Check that permission denied was recorded
        let has_denied = result
            .events
            .iter()
            .any(|e| matches!(e, Payload::PermissionDenied { .. }));
        // DefaultPermissionGate returns Pending for RequireConfirmation, not Denied
        // But rm -rf with bash should be Block -> Denied
        assert!(
            has_denied
                || result
                    .events
                    .iter()
                    .any(|e| { matches!(e, Payload::PermissionRequested { .. }) })
        );
    }

    #[tokio::test]
    async fn agent_readonly_sub_turn() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        // Parent LLM: call agent tool
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_agent".into(),
                name: "agent".into(),
                input: serde_json::json!({
                    "prompt": "find auth files",
                    "mode": "readonly",
                    "description": "auth-finder"
                }),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // Sub-Turn LLM: responds with text (the sub-Turn's answer)
        mock_llm.push_text("Found 3 auth-related files.");

        // Parent LLM: final response after getting agent result
        mock_llm.push_text("The sub-agent found 3 auth-related files.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "search for auth code",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        let text = result.response_text.unwrap();
        assert!(text.contains("auth"));
    }

    #[tokio::test]
    async fn agent_depth_exceeded() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        // LLM: call agent at depth 3
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_agent".into(),
                name: "agent".into(),
                input: serde_json::json!({
                    "prompt": "nested task",
                    "mode": "readonly"
                }),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // After the depth-exceeded error, LLM gives final text
        mock_llm.push_text("Could not execute nested agent.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig {
            agent_depth: MAX_AGENT_DEPTH, // already at max
            ..Default::default()
        };
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "do something",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        // The tool result should contain depth exceeded error
        let has_tool_result = result.events.iter().any(|e| {
            matches!(e, Payload::ToolInvocationResult { output, is_error, .. } if *is_error && output.contains("max recursion depth"))
        });
        assert!(has_tool_result);
    }

    #[test]
    fn turn_config_default_depth() {
        let config = TurnConfig::default();
        assert_eq!(config.agent_depth, 0);
    }

    #[tokio::test]
    async fn agent_fork_mode_clones_history() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        // First: parent turn gets a text response to build history
        mock_llm.push_text("I understand the context.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        // Turn 1: build some history
        let _ = run_turn(TurnContext {
            input: "hello",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(history.len(), 2); // user + assistant

        // Turn 2: agent fork call -- sub-Turn should see parent history
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_fork".into(),
                name: "agent".into(),
                input: serde_json::json!({
                    "prompt": "continue work",
                    "mode": "fork",
                    "description": "forked-agent"
                }),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // Sub-Turn LLM response
        mock_llm.push_text("Continuing from where you left off.");
        // Parent final response
        mock_llm.push_text("The forked agent continued successfully.");

        let result = run_turn(TurnContext {
            input: "fork and continue",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert!(result.response_text.unwrap().contains("forked"));
    }

    #[tokio::test]
    async fn agent_teammate_mode_sub_turn() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        // Parent LLM: call agent in teammate mode
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_team".into(),
                name: "agent".into(),
                input: serde_json::json!({
                    "prompt": "review the auth module",
                    "mode": "teammate",
                    "team_name": "code-review",
                    "description": "reviewer"
                }),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });

        // Sub-Turn LLM: worker response
        mock_llm.push_text("Auth module looks secure. No issues found.");

        // Parent LLM: final response
        mock_llm.push_text("The reviewer found no issues in the auth module.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "review auth",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        let text = result.response_text.unwrap();
        assert!(text.contains("auth"));
    }

    #[tokio::test]
    async fn working_memory_events_in_journal() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("data.txt");
        std::fs::write(&test_file, "some data").unwrap();

        // LLM calls read tool, then gives final text
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_1".into(),
                name: "read".into(),
                input: serde_json::json!({"file_path": test_file.to_str().unwrap()}),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });
        mock_llm.push_text("Here is the data.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "read the data file",
            history: &mut history,
            llm: &mock_llm,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_text: None,
            on_tool_progress: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            cancel_flag: None,
            message_inbox: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);

        // Check that working memory events are present
        let has_activated = result
            .events
            .iter()
            .any(|e| matches!(e, Payload::WorkingMemoryItemActivated { .. }));
        assert!(
            has_activated,
            "should have WorkingMemoryItemActivated events from input keywords"
        );

        // Verify events are also in journal
        let all_events = journal.recent_events(50).unwrap();
        let has_wm_events = all_events
            .iter()
            .any(|e| e.event_type.starts_with("WorkingMemory"));
        assert!(has_wm_events, "journal should contain WorkingMemory events");
    }
}
