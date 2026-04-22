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
            turns_since_extract: 0,
            tool_timeout_secs: defaults.tool_timeout_secs,
            llm_transient_retries: defaults.llm_transient_retries,
            strip_think_tags: defaults.strip_think_tags,
            evolution_weights: [1.0, 0.8, 0.6, 0.5, 0.4, 0.3],
            pressure_thresholds: [0.60, 0.75, 0.85, 0.95],
            metacognition: cortex_types::config::MetacognitionConfig::default(),
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
        risk_assessor: &RiskAssessor,
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmResponse, LlmToolCall, MockLlmClient, Usage};
    use crate::risk::AutoApproveGate;
    use crate::skills::skill_tool::SkillTool;
    use crate::skills::{Skill, SkillContent, SkillRegistry};
    use crate::tools::register_core_tools_basic;
    use cortex_types::{ExecutionMode, SkillMetadata, SkillSource, TurnState};

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

    #[test]
    fn turn_control_poll_drains_messages_in_order() {
        let control = TurnControl::new();

        assert!(control.inject_message("first".into()));
        assert!(control.inject_message("second".into()));

        let first_poll = control.poll();
        assert_eq!(
            first_poll,
            TurnControlPoll {
                cancel_requested: false,
                injected_messages: vec!["first".into(), "second".into()],
            }
        );

        let second_poll = control.poll();
        assert_eq!(second_poll, TurnControlPoll::default());
    }

    #[test]
    fn turn_control_dispatch_returns_cancel_and_messages_together() {
        let control = TurnControl::new();

        assert!(control.inject_message("latest".into()));
        control.request_cancel();

        let dispatch = control.dispatch();
        assert_eq!(dispatch.action, TurnControlAction::AbortTurn);
        assert_eq!(dispatch.injected_messages, vec!["latest".to_string()]);

        let next_dispatch = control.dispatch();
        assert_eq!(next_dispatch.action, TurnControlAction::AbortTurn);
        assert!(next_dispatch.injected_messages.is_empty());
    }

    #[test]
    fn turn_control_execution_boundary_only_aborts_on_cancel() {
        let control = TurnControl::new();

        assert_eq!(control.execution_boundary(), TurnControlBoundary::Continue);
        assert!(control.inject_message("queued".into()));
        assert_eq!(control.execution_boundary(), TurnControlBoundary::Continue);

        control.request_cancel();

        assert_eq!(control.execution_boundary(), TurnControlBoundary::AbortTurn);
    }

    #[test]
    fn turn_control_rejects_messages_after_input_window_closes() {
        let control = TurnControl::new();
        control.close_input_window();

        assert!(!control.inject_message("late".into()));
        assert_eq!(control.poll(), TurnControlPoll::default());
    }

    fn setup_tools() -> ToolRegistry {
        let mut reg = ToolRegistry::new();
        register_core_tools_basic(&mut reg);
        reg
    }

    struct ForkTestSkill;

    impl Skill for ForkTestSkill {
        fn name(&self) -> &'static str {
            "fork-test"
        }

        fn description(&self) -> &'static str {
            "fork test skill"
        }

        fn when_to_use(&self) -> &'static str {
            "testing forked skill execution"
        }

        fn execution_mode(&self) -> ExecutionMode {
            ExecutionMode::Fork
        }

        fn content(&self, args: &str) -> SkillContent {
            SkillContent::Markdown(format!("investigate: {args}"))
        }

        fn metadata(&self) -> SkillMetadata {
            SkillMetadata {
                source: SkillSource::System,
                ..SkillMetadata::default()
            }
        }
    }

    fn setup_tools_with_skill_registry(
        skill_registry: std::sync::Arc<SkillRegistry>,
    ) -> ToolRegistry {
        let mut reg = setup_tools();
        reg.register(Box::new(SkillTool::new(skill_registry)));
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert!(result.response_text.unwrap().contains("file content here"));

        // History should have: user, assistant(tool_use), user(tool_result), assistant(text)
        assert_eq!(history.len(), 4);
    }

    #[tokio::test]
    async fn image_turn_uses_vision_once_then_text_llm_without_images() {
        let journal = Journal::open_in_memory().unwrap();
        let text_llm = MockLlmClient::new();
        let vision_llm = MockLlmClient::new();
        let dir = tempfile::tempdir().unwrap();
        let test_file = dir.path().join("image-notes.txt");
        std::fs::write(&test_file, "vision follow-up context").unwrap();

        vision_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_vision_read".into(),
                name: "read".into(),
                input: serde_json::json!({"file_path": test_file.to_str().unwrap()}),
            }],
            usage: Usage::default(),
            model: "vision-mock".into(),
        });
        text_llm.push_text("The image was handled, and I read the follow-up context.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "look at this image, then read my notes",
            history: &mut history,
            llm: &text_llm,
            vision_llm: Some(&vision_llm),
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![("image/png".into(), "base64-image".into())],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert_eq!(vision_llm.requests().len(), 1);
        assert!(vision_llm.requests()[0].has_images);
        assert_eq!(text_llm.requests().len(), 1);
        assert!(!text_llm.requests()[0].has_images);
        assert!(!history.iter().any(Message::has_images));
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        let text = result.response_text.unwrap();
        assert!(text.contains("auth"));
    }

    #[tokio::test]
    async fn agent_sub_turn_does_not_stream_text_to_parent_callback() {
        use std::sync::{Arc, Mutex};

        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

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
        mock_llm.push_text("Found 3 auth-related files.");
        mock_llm.push_text("The sub-agent found 3 auth-related files.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();
        let user_visible = Arc::new(Mutex::new(String::new()));
        let user_visible_clone = Arc::clone(&user_visible);
        let observer = Arc::new(Mutex::new(String::new()));
        let observer_clone = Arc::clone(&observer);
        let on_event = move |event: &TurnStreamEvent| match event {
            TurnStreamEvent::Text {
                lane: StreamLane::UserVisible,
                content,
                ..
            } => {
                let mut acc = user_visible_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                acc.push_str(content);
            }
            TurnStreamEvent::Text {
                lane: StreamLane::Observer,
                content,
                ..
            } => {
                let mut acc = observer_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                acc.push_str(content);
            }
            TurnStreamEvent::Boundary(_) | TurnStreamEvent::ToolProgress(_) => {}
        };

        let result = run_turn(TurnContext {
            input: "search for auth code",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: Some(&on_event),
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        let user_visible_text = user_visible
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let observer_text = observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(user_visible_text.contains("The sub-agent found 3 auth-related files."));
        assert!(!user_visible_text.contains("Found 3 auth-related files."));
        assert!(observer_text.contains("Found 3 auth-related files."));
    }

    #[tokio::test]
    async fn fork_skill_executes_via_unified_skill_plan() {
        use std::sync::{Arc, Mutex};

        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();
        let skill_registry = Arc::new(SkillRegistry::new());
        skill_registry.register(Box::new(ForkTestSkill));

        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_skill".into(),
                name: "skill".into(),
                input: serde_json::json!({
                    "skill": "fork-test",
                    "args": "auth flow"
                }),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });
        mock_llm.push_text("Fork skill observation.");
        mock_llm.push_text("The forked skill finished successfully.");

        let tools = setup_tools_with_skill_registry(Arc::clone(&skill_registry));
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();
        let observer = Arc::new(Mutex::new(String::new()));
        let observer_clone = Arc::clone(&observer);
        let on_event = move |event: &TurnStreamEvent| {
            if let TurnStreamEvent::Text {
                lane: StreamLane::Observer,
                content,
                ..
            } = event
            {
                let mut acc = observer_clone
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                acc.push_str(content);
            }
        };

        let result = run_turn(TurnContext {
            input: "use a forked skill",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: Some(&on_event),
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: Some(skill_registry.as_ref()),
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert_eq!(
            result.response_text.as_deref(),
            Some("The forked skill finished successfully.")
        );
        let observer_text = observer
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(observer_text.contains("Fork skill observation."));
    }

    #[tokio::test]
    async fn injected_input_restarts_before_remaining_tool_calls_in_batch() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();
        let control = TurnControl::new();

        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_read_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"file_path": "/dev/null"}),
                },
                LlmToolCall {
                    id: "tc_read_2".into(),
                    name: "read".into(),
                    input: serde_json::json!({"file_path": "/dev/null"}),
                },
            ],
            usage: Usage::default(),
            model: "mock".into(),
        });
        mock_llm.push_text("Resumed after the injected user input.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();
        let completed = Arc::new(AtomicUsize::new(0));
        let completed_clone = Arc::clone(&completed);
        let control_clone = control.clone();
        let on_event = move |event: &TurnStreamEvent| {
            if let TurnStreamEvent::ToolProgress(progress) = event
                && progress.tool_name == "read"
                && progress.status == ToolProgressStatus::Completed
                && completed_clone.fetch_add(1, Ordering::Relaxed) == 0
            {
                let _ =
                    control_clone.inject_message("Actually, include the new requirement.".into());
            }
        };

        let result = run_turn(TurnContext {
            input: "read twice",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: Some(&on_event),
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: Some(control),
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert_eq!(
            result.response_text.as_deref(),
            Some("Resumed after the injected user input.")
        );
        assert_eq!(completed.load(Ordering::Relaxed), 1);
        assert!(history.iter().any(|message| {
            message
                .text_content()
                .contains("Actually, include the new requirement.")
        }));
    }

    #[tokio::test]
    async fn injected_input_restarts_before_final_response_is_committed() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = crate::llm::MockLlmClient::new();
        let control = TurnControl::new();

        mock_llm.push_text("Reply for test2.");
        mock_llm.push_text("Reply for test2 plus injected test3.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = vec![Message::user("test1")];
        let control_clone = control.clone();
        let on_event = move |event: &TurnStreamEvent| {
            if let TurnStreamEvent::Text {
                lane: StreamLane::UserVisible,
                content,
                ..
            } = event
                && content.contains("Reply for test2.")
            {
                let _ = control_clone.inject_message("test3".into());
            }
        };

        let result = run_turn(TurnContext {
            input: "test2",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: Some(&on_event),
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: Some(control),
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.state, TurnState::Completed);
        assert_eq!(
            result.response_text.as_deref(),
            Some("Reply for test2 plus injected test3.")
        );
        assert!(
            history
                .iter()
                .any(|message| message.text_content().contains("test3"))
        );
        assert!(
            !history
                .iter()
                .any(|message| message.text_content().contains("Reply for test2."))
        );
    }

    #[tokio::test]
    async fn cancel_stops_remaining_tool_calls_in_batch() {
        use std::sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        };

        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();
        let control = TurnControl::new();

        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![
                LlmToolCall {
                    id: "tc_read_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"file_path": "/dev/null"}),
                },
                LlmToolCall {
                    id: "tc_read_2".into(),
                    name: "read".into(),
                    input: serde_json::json!({"file_path": "/dev/null"}),
                },
            ],
            usage: Usage::default(),
            model: "mock".into(),
        });

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();
        let completed = Arc::new(AtomicUsize::new(0));
        let completed_clone = Arc::clone(&completed);
        let control_clone = control.clone();
        let on_event = move |event: &TurnStreamEvent| {
            if let TurnStreamEvent::ToolProgress(progress) = event
                && progress.tool_name == "read"
                && progress.status == ToolProgressStatus::Completed
                && completed_clone.fetch_add(1, Ordering::Relaxed) == 0
            {
                control_clone.request_cancel();
            }
        };

        let result = run_turn(TurnContext {
            input: "read twice then stop",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: Some(&on_event),
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: Some(control),
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(completed.load(Ordering::Relaxed), 1);
        assert!(result.response_text.is_none());
        let tool_invocation_results = result
            .events
            .iter()
            .filter(|event| matches!(event, Payload::ToolInvocationResult { .. }))
            .count();
        assert_eq!(tool_invocation_results, 1);
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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

    #[tokio::test]
    async fn assistant_text_is_preserved_when_tool_calls_are_present() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        mock_llm.push_response(LlmResponse {
            text: Some("I will inspect the file first.".into()),
            tool_calls: vec![LlmToolCall {
                id: "tc_read".into(),
                name: "read".into(),
                input: serde_json::json!({"file_path": "/dev/null"}),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });
        mock_llm.push_text("The file is empty.");

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "inspect /dev/null",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await
        .unwrap();

        assert_eq!(result.response_text.as_deref(), Some("The file is empty."));
        let tool_turn_assistant = history
            .iter()
            .find(|message| {
                message.role == cortex_types::Role::Assistant
                    && message.has_tool_blocks()
                    && !message.text_content().is_empty()
            })
            .expect("assistant tool message should keep its text");
        assert_eq!(
            tool_turn_assistant.text_content(),
            "I will inspect the file first."
        );
    }

    #[tokio::test]
    async fn tool_execution_without_final_answer_returns_error() {
        let journal = Journal::open_in_memory().unwrap();
        let mock_llm = MockLlmClient::new();

        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: vec![LlmToolCall {
                id: "tc_read".into(),
                name: "read".into(),
                input: serde_json::json!({"file_path": "/dev/null"}),
            }],
            usage: Usage::default(),
            model: "mock".into(),
        });
        mock_llm.push_response(LlmResponse {
            text: None,
            tool_calls: Vec::new(),
            usage: Usage::default(),
            model: "mock".into(),
        });

        let tools = setup_tools();
        let gate = AutoApproveGate;
        let config = TurnConfig::default();
        let mut history = Vec::new();

        let result = run_turn(TurnContext {
            input: "read and answer",
            history: &mut history,
            llm: &mock_llm,
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
        })
        .await;

        let Err(err) = result else {
            panic!("missing final answer should fail loudly");
        };

        assert!(
            err.to_string()
                .contains("without a final assistant response")
        );
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
            vision_llm: None,
            tools: &tools,
            journal: &journal,
            gate: &gate,
            config: &config,
            on_event: None,
            images: vec![],
            compress_template: None,
            summary_cache: None,
            prompt_manager: None,
            skill_registry: None,
            post_turn_llm: None,
            tracer: &NullTracer,
            control: None,
            on_tpn_complete: None,
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
