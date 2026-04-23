use std::collections::VecDeque;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamLane {
    UserVisible,
    Observer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TurnStreamEvent {
    Text {
        lane: StreamLane,
        source: Option<String>,
        content: String,
    },
    Boundary(TurnStreamBoundary),
    ToolProgress(ToolProgress),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnStreamBoundary {
    Restart,
}

#[derive(Default)]
struct TurnControlState {
    cancel_requested: std::sync::atomic::AtomicBool,
    accepting_input: std::sync::atomic::AtomicBool,
    pending_signals: std::sync::Mutex<VecDeque<TurnControlSignal>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TurnControlSignal {
    CancelRequested,
    UserInput(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TurnControlPoll {
    pub cancel_requested: bool,
    pub injected_messages: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnControlAction {
    #[default]
    Continue,
    RestartTurn,
    AbortTurn,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnControlBoundary {
    #[default]
    Continue,
    RestartTurn,
    AbortTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TurnControlCheckpoint {
    IterationBoundary,
    ToolBatchBoundary,
}

impl TurnControlCheckpoint {
    pub(crate) const fn cancel_trace(self) -> &'static str {
        match self {
            Self::IterationBoundary => "Turn cancelled by user (/stop)",
            Self::ToolBatchBoundary => "Turn cancelled during tool batch",
        }
    }

    pub(crate) const fn input_trace(self) -> &'static str {
        match self {
            Self::IterationBoundary => "Injected mid-turn user message",
            Self::ToolBatchBoundary => "Injected mid-turn user message during tool batch",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct TurnControlDispatch {
    pub action: TurnControlAction,
    pub injected_messages: Vec<String>,
}

impl TurnControlDispatch {
    pub(crate) fn apply_to_history(&self, history: &mut Vec<Message>) {
        for msg in &self.injected_messages {
            history.push(Message::user(msg));
        }
    }

    #[must_use]
    pub(crate) const fn trace_message(
        &self,
        checkpoint: TurnControlCheckpoint,
    ) -> Option<&'static str> {
        match self.action {
            TurnControlAction::Continue => None,
            TurnControlAction::RestartTurn => Some(checkpoint.input_trace()),
            TurnControlAction::AbortTurn => Some(checkpoint.cancel_trace()),
        }
    }

    #[must_use]
    pub(crate) const fn boundary(&self) -> TurnControlBoundary {
        match self.action {
            TurnControlAction::Continue => TurnControlBoundary::Continue,
            TurnControlAction::RestartTurn => TurnControlBoundary::RestartTurn,
            TurnControlAction::AbortTurn => TurnControlBoundary::AbortTurn,
        }
    }
}

/// Shared control-plane handle for a running turn.
///
/// This separates runtime controls from the turn's data/context payload:
/// cancellation, mid-turn user input, and the answer boundary after TPN.
#[derive(Clone, Default)]
pub struct TurnControl {
    state: std::sync::Arc<TurnControlState>,
}

impl TurnControl {
    #[must_use]
    pub fn new() -> Self {
        let control = Self::default();
        control
            .state
            .accepting_input
            .store(true, std::sync::atomic::Ordering::Relaxed);
        control
    }

    pub fn request_cancel(&self) {
        let was_requested = self
            .state
            .cancel_requested
            .swap(true, std::sync::atomic::Ordering::Relaxed);
        if !was_requested {
            self.state
                .pending_signals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push_back(TurnControlSignal::CancelRequested);
        }
    }

    #[must_use]
    pub fn is_cancel_requested(&self) -> bool {
        self.state
            .cancel_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[must_use]
    pub fn inject_message(&self, text: String) -> bool {
        if !self
            .state
            .accepting_input
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return false;
        }
        self.state
            .pending_signals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(TurnControlSignal::UserInput(text));
        true
    }

    #[must_use]
    pub(crate) fn poll(&self) -> TurnControlPoll {
        let signals: Vec<_> = self
            .state
            .pending_signals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        let mut poll = TurnControlPoll::default();
        for signal in signals {
            match signal {
                TurnControlSignal::CancelRequested => {
                    poll.cancel_requested = true;
                }
                TurnControlSignal::UserInput(text) => {
                    poll.injected_messages.push(text);
                }
            }
        }
        poll.cancel_requested |= self.is_cancel_requested();
        poll
    }

    #[must_use]
    pub(crate) fn dispatch(&self) -> TurnControlDispatch {
        let poll = self.poll();
        let action = if poll.cancel_requested {
            TurnControlAction::AbortTurn
        } else if poll.injected_messages.is_empty() {
            TurnControlAction::Continue
        } else {
            TurnControlAction::RestartTurn
        };
        TurnControlDispatch {
            action,
            injected_messages: poll.injected_messages,
        }
    }

    #[must_use]
    pub(crate) fn execution_boundary(&self) -> TurnControlBoundary {
        if self.is_cancel_requested() {
            TurnControlBoundary::AbortTurn
        } else {
            TurnControlBoundary::Continue
        }
    }

    pub fn close_input_window(&self) {
        self.state
            .accepting_input
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

pub(crate) fn dispatch_turn_control(
    control: Option<&TurnControl>,
    history: &mut Vec<Message>,
    tracer: &dyn TurnTracer,
    checkpoint: TurnControlCheckpoint,
) -> TurnControlBoundary {
    let Some(control) = control else {
        return TurnControlBoundary::Continue;
    };
    let dispatch = control.dispatch();
    dispatch.apply_to_history(history);
    if let Some(message) = dispatch.trace_message(checkpoint) {
        tracer.trace_at(
            TraceCategory::Phase,
            cortex_types::TraceLevel::Minimal,
            message,
        );
    }
    dispatch.boundary()
}

pub const MAX_AGENT_DEPTH: usize = 3;

/// Strip `<think>…</think>` blocks and orphaned `</think>` prefixes from LLM
/// output.  Only applied to assistant responses so user-authored `<think>` text
/// is never touched.
/// Regex for matching `<think>…</think>` blocks.
static RE_THINK_BLOCK: std::sync::LazyLock<regex::Regex> =
    std::sync::LazyLock::new(|| match regex::Regex::new(r"(?s)<think>.*?</think>\s*") {
        Ok(regex) => regex,
        Err(err) => panic!("invalid think-block regex: {err}"),
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
    /// Active reconsolidation candidates visible to memory extraction.
    pub reconsolidation_memories: Vec<cortex_types::MemoryEntry>,
    /// How many turns have elapsed since last extraction.
    /// The caller is responsible for tracking and passing this value.
    pub turns_since_extract: usize,
    /// Global tool execution timeout in seconds (from config).
    pub tool_timeout_secs: u64,
    /// Retry count for transient LLM failures before visible output starts.
    pub llm_transient_retries: usize,
    /// Whether to strip `<think>…</think>` tags from final output.
    pub strip_think_tags: bool,
    /// Evolution signal weights (6 signals, configurable).
    pub evolution_weights: [f64; 6],
    /// Context pressure thresholds for Normal/Alert/Compress/Urgent/Degrade.
    pub pressure_thresholds: [f64; 4],
    /// Metacognition subsystem configuration.
    pub metacognition: cortex_types::config::MetacognitionConfig,
    /// Tool risk policy configuration.
    pub risk: cortex_types::config::RiskConfig,
    /// Per-category trace switches.
    pub trace: cortex_types::config::TurnTraceConfig,
    /// Session id exposed to plugin tools.
    pub session_id: Option<String>,
    /// Canonical actor exposed to plugin tools.
    pub actor: Option<String>,
    /// Invocation source / transport exposed to plugin tools.
    pub source: Option<String>,
    /// Foreground/background execution scope exposed to plugin tools.
    pub execution_scope: cortex_sdk::ExecutionScope,
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
            reconsolidation_memories: Vec::new(),
            turns_since_extract: 0,
            tool_timeout_secs: defaults.tool_timeout_secs,
            llm_transient_retries: defaults.llm_transient_retries,
            strip_think_tags: defaults.strip_think_tags,
            evolution_weights: [1.0, 0.8, 0.6, 0.5, 0.4, 0.3],
            pressure_thresholds: [0.60, 0.75, 0.85, 0.95],
            metacognition: cortex_types::config::MetacognitionConfig::default(),
            risk: cortex_types::config::RiskConfig::default(),
            trace: cortex_types::config::TurnTraceConfig::default(),
            session_id: None,
            actor: None,
            source: None,
            execution_scope: cortex_sdk::ExecutionScope::Foreground,
        }
    }
}

pub struct TurnResult {
    pub response_text: Option<String>,
    pub response_media: Vec<cortex_types::Attachment>,
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
    pub vision_llm: Option<&'a dyn LlmClient>,
    pub tools: &'a ToolRegistry,
    pub journal: &'a Journal,
    pub gate: &'a dyn PermissionGate,
    pub config: &'a TurnConfig,
    pub on_event: Option<&'a (dyn Fn(&TurnStreamEvent) + Send + Sync)>,
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
    /// Shared turn runtime control plane: cancellation, pending input, and
    /// answer-boundary input gating.
    pub control: Option<TurnControl>,
    /// Callback fired immediately after the TPN loop completes and before
    /// post-turn DMN work begins. Callers use this to stop accepting
    /// mid-turn injections once the visible response generation has ended.
    pub on_tpn_complete: Option<&'a (dyn Fn() + Send + Sync)>,
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
        vision_llm,
        tools,
        journal,
        gate,
        config,
        on_event,
        images,
        compress_template,
        summary_cache,
        prompt_manager,
        skill_registry,
        post_turn_llm,
        tracer,
        control,
        on_tpn_complete,
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
    let has_current_images = !images.is_empty();
    push_user_message(history, input, images);
    if !has_current_images {
        crate::llm::sanitize_history_for_text_only_turn(history);
    }
    let tool_defs = tools.definitions();
    trace_phase(tracer, "TPN");
    let tpn_start = std::time::Instant::now();
    let mut response_media = Vec::new();
    let risk_assessor = RiskAssessor::new(config.risk.clone());
    let final_text = tpn::run_tpn_loop(&mut tpn::TpnLoopContext {
        history,
        llm,
        vision_llm,
        tools,
        journal,
        gate,
        config,
        on_event,
        compress_template: compress_template.as_ref(),
        summary_cache,
        system_prompt: system_prompt.as_ref(),
        tool_defs: &tool_defs,
        working_mem: &mut working_mem,
        scheduler: &mut scheduler,
        confidence: &mut confidence,
        meta_monitor: &mut meta_monitor,
        denial_tracker: &mut DenialTracker::new(config.metacognition.denial.clone()),
        risk_assessor: &risk_assessor,
        reasoning_engine: &mut reasoning_engine,
        prompt_manager,
        skill_registry,
        turn_id,
        corr_id,
        events_log: &mut events_log,
        response_media: &mut response_media,
        tracer,
        control,
    })
    .await?;
    complete_turn_after_tpn(CompleteTurnAfterTpnInput {
        on_tpn_complete,
        tpn_start,
        journal,
        config,
        tracer,
        turn_id,
        corr_id,
        state,
        final_text,
        response_media,
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
    })
    .await
}

struct CompleteTurnAfterTpnInput<'a> {
    on_tpn_complete: Option<&'a (dyn Fn() + Send + Sync)>,
    tpn_start: std::time::Instant,
    journal: &'a Journal,
    config: &'a TurnConfig,
    tracer: &'a dyn TurnTracer,
    turn_id: TurnId,
    corr_id: CorrelationId,
    state: TurnState,
    final_text: Option<String>,
    response_media: Vec<cortex_types::Attachment>,
    reasoning_engine: crate::reasoning::ReasoningEngine,
    confidence: crate::confidence::ConfidenceTracker,
    meta_monitor: crate::meta::MetaMonitor,
    working_mem: crate::working_memory::WorkingMemoryManager,
    events_log: Vec<Payload>,
    prompt_manager: Option<&'a cortex_kernel::PromptManager>,
    skill_registry: Option<&'a crate::skills::SkillRegistry>,
    input: &'a str,
    llm: &'a dyn LlmClient,
    post_turn_llm: Option<&'a dyn LlmClient>,
    history: &'a mut Vec<Message>,
}

async fn complete_turn_after_tpn(
    input: CompleteTurnAfterTpnInput<'_>,
) -> Result<TurnResult, TurnError> {
    let CompleteTurnAfterTpnInput {
        on_tpn_complete,
        tpn_start,
        journal,
        config,
        tracer,
        turn_id,
        corr_id,
        state,
        final_text,
        response_media,
        reasoning_engine,
        confidence,
        meta_monitor,
        working_mem,
        mut events_log,
        prompt_manager,
        skill_registry,
        input,
        llm,
        post_turn_llm,
        history,
    } = input;
    if let Some(callback) = on_tpn_complete {
        callback();
    }
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
        response_media,
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
