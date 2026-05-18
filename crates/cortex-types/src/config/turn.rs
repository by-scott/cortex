use serde::{Deserialize, Serialize};

/// Default max tool iterations per turn.
const DEFAULT_MAX_TOOL_ITERATIONS: usize = 1024;

/// Default tool execution timeout in seconds.
const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 1800;

/// Default foreground turn execution timeout in seconds. Zero disables it.
const DEFAULT_TURN_EXECUTION_TIMEOUT_SECS: u64 = 0;

/// Default transient LLM retry count for a single request.
pub const DEFAULT_LLM_TRANSIENT_RETRIES: usize = 5;

/// Trace detail level, ordered from least to most verbose.
///
/// Levels form a total order: `Off < Minimal < Basic < Summary < Full < Debug`.
/// When a category's effective level is `>= N`, all messages at level `N` or
/// lower are emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Hash)]
#[serde(rename_all = "lowercase")]
pub enum TraceLevel {
    /// No output for this category.
    Off,
    /// Event names only (e.g. "SN phase").
    Minimal,
    /// + key metrics (e.g. token counts).
    Basic,
    /// + summary information (default).
    Summary,
    /// + complete parameters and results.
    Full,
    /// + internal state details.
    Debug,
}

/// Per-category trace configuration with global default and per-category
/// overrides.
///
/// Categories: `phase`, `llm`, `tool`, `meta`, `memory`, `context`.
///
/// ```toml
/// [turn.trace]
/// level = "summary"     # global default
/// # phase = "minimal"   # override for phase traces
/// # llm = "full"        # override for LLM traces
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnTraceConfig {
    /// Global default trace level.
    pub level: TraceLevel,
    /// Per-category overrides. Missing = use global level.
    #[serde(default)]
    pub phase: Option<TraceLevel>,
    #[serde(default)]
    pub llm: Option<TraceLevel>,
    #[serde(default)]
    pub tool: Option<TraceLevel>,
    #[serde(default)]
    pub meta: Option<TraceLevel>,
    #[serde(default)]
    pub memory: Option<TraceLevel>,
    #[serde(default)]
    pub context: Option<TraceLevel>,
}

impl Default for TurnTraceConfig {
    fn default() -> Self {
        Self {
            level: TraceLevel::Off,
            phase: None,
            llm: None,
            tool: Some(TraceLevel::Summary),
            meta: None,
            memory: None,
            context: None,
        }
    }
}

impl TurnTraceConfig {
    /// Get effective level for a category.
    #[must_use]
    pub fn level_for(&self, category: &str) -> TraceLevel {
        match category {
            "phase" => self.phase.unwrap_or(self.level),
            "llm" => self.llm.unwrap_or(self.level),
            "tool" => self.tool.unwrap_or(self.level),
            "meta" => self.meta.unwrap_or(self.level),
            "memory" => self.memory.unwrap_or(self.level),
            "context" => self.context.unwrap_or(self.level),
            _ => self.level,
        }
    }

    /// Check if a category is enabled at at least a given level.
    #[must_use]
    pub fn is_enabled_at(&self, category: &str, min_level: TraceLevel) -> bool {
        self.level_for(category) >= min_level
    }

    /// Check if a category is enabled (level >= `Minimal`).
    #[must_use]
    pub fn is_enabled(&self, category: &str) -> bool {
        self.level_for(category) >= TraceLevel::Minimal
    }

    /// Shorthand accessors for the six standard categories.
    #[must_use]
    pub fn phase(&self) -> bool {
        self.is_enabled("phase")
    }

    #[must_use]
    pub fn llm(&self) -> bool {
        self.is_enabled("llm")
    }

    #[must_use]
    pub fn tool(&self) -> bool {
        self.is_enabled("tool")
    }

    #[must_use]
    pub fn meta(&self) -> bool {
        self.is_enabled("meta")
    }

    #[must_use]
    pub fn memory(&self) -> bool {
        self.is_enabled("memory")
    }

    #[must_use]
    pub fn context(&self) -> bool {
        self.is_enabled("context")
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TurnSection {
    pub max_tool_iterations: usize,
    /// Global timeout for a foreground turn, including all LLM calls and tools.
    /// Zero disables the whole-turn timeout.
    pub execution_timeout_secs: u64,
    /// Global timeout for individual tool executions, in seconds.
    /// Tools can override via `Tool::timeout_secs()`.
    pub tool_timeout_secs: u64,
    /// Retry count for transient LLM transport/provider failures before any
    /// user-visible text has been emitted.
    pub llm_transient_retries: usize,
    /// Whether to strip provider thinking from user-visible LLM output.
    /// Defaults to `true`. Can be toggled persistently via `/think`,
    /// `/config set`, `cortex config set`, config file edits, or install env.
    /// Main OpenAI-compatible turns also map this to the configured
    /// provider thinking parameter when one is declared.
    pub strip_think_tags: bool,
    /// Per-category trace switches for turn execution tracing.
    pub trace: TurnTraceConfig,
}

impl Default for TurnSection {
    fn default() -> Self {
        Self {
            max_tool_iterations: DEFAULT_MAX_TOOL_ITERATIONS,
            execution_timeout_secs: DEFAULT_TURN_EXECUTION_TIMEOUT_SECS,
            tool_timeout_secs: DEFAULT_TOOL_TIMEOUT_SECS,
            llm_transient_retries: DEFAULT_LLM_TRANSIENT_RETRIES,
            strip_think_tags: true,
            trace: TurnTraceConfig::default(),
        }
    }
}
