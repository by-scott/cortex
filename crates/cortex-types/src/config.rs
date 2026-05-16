use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{RiskLevel, model_routing::ModelCapability};

mod autonomous;
mod integrations;
mod media;
mod model_limits;
mod provider;

pub use self::autonomous::{AutonomousConfig, AutonomousLimits, AutonomousThresholds};
pub use self::integrations::{
    AcpClientConfig, AcpConfig, McpConfig, McpServerConfig, McpTransportType,
};
pub use self::media::MediaConfig;
pub use self::model_limits::{inferred_model_token_limits, resolved_model_token_limits};
pub use self::provider::{
    AuthType, ModelTokenLimits, OpenAiImageInputMode, OpenAiThinkingParameter, ProviderConfig,
    ProviderProtocol, ProviderRegistry, ResolvedEndpoint,
};

// ── Named Constants ──

/// Conservative output `max_tokens` fallback when model-specific limits are unknown.
pub const DEFAULT_MAX_TOKENS_FALLBACK: usize = 8_192;

/// Safe default output token cap for multimodal/vision requests.
pub const DEFAULT_VISION_MAX_OUTPUT_TOKENS: usize = 8192;

/// Default context window override. `0` means infer from provider/model limits.
pub const DEFAULT_CONTEXT_MAX_TOKENS: usize = 0;

/// Default API provider name.
const DEFAULT_PROVIDER: &str = "anthropic";

/// Default primary model — empty means resolve from provider's models list.
const DEFAULT_MODEL: &str = "";

/// Default embedding provider name.
const DEFAULT_EMBEDDING_PROVIDER: &str = "ollama";

/// Default embedding model identifier.
const DEFAULT_EMBEDDING_MODEL: &str = "nomic-embed-text";

/// Default minimum embedding samples before eligibility.
const DEFAULT_EMBEDDING_MIN_SAMPLES: u32 = 10;

/// Default minimum samples before considering model switch.
const DEFAULT_SWITCH_THRESHOLD_SAMPLES: u32 = 50;

/// Default minimum precision improvement delta.
const DEFAULT_SWITCH_PRECISION_DELTA: f64 = 0.1;

/// Default maximum Brave search results.
const DEFAULT_BRAVE_MAX_RESULTS: usize = 10;

/// Default context pressure thresholds.
const DEFAULT_PRESSURE_THRESHOLDS: [f64; 4] = [0.60, 0.75, 0.85, 0.95];

/// Default memory max recall count.
const DEFAULT_MAX_RECALL: usize = 10;

/// Default memory decay rate.
const DEFAULT_DECAY_RATE: f64 = 0.05;

/// Default minimum turns before memory extraction.
const DEFAULT_EXTRACT_MIN_TURNS: usize = 5;

/// Default consolidation interval in hours.
const DEFAULT_CONSOLIDATE_INTERVAL_HOURS: u64 = 24;

/// Default semantic similarity threshold for memory consolidation.
const DEFAULT_CONSOLIDATION_SIMILARITY_THRESHOLD: f64 = 0.85;

/// Default semantic similarity threshold for episodic-to-semantic upgrades.
const DEFAULT_SEMANTIC_UPGRADE_SIMILARITY_THRESHOLD: f64 = 0.90;

/// Default doom-loop detection threshold.
const DEFAULT_DOOM_LOOP_THRESHOLD: usize = 3;

/// Default metacognition duration limit in seconds.
const DEFAULT_DURATION_LIMIT_SECS: u64 = 86400;

/// Default fatigue threshold.
const DEFAULT_FATIGUE_THRESHOLD: f64 = 0.8;

/// Default frame anchoring threshold for adaptive thresholds.
const DEFAULT_FRAME_ANCHORING_THRESHOLD: f64 = 0.5;

/// Default goal stagnation threshold (turns with identical goal).
const DEFAULT_GOAL_STAGNATION_THRESHOLD: usize = 5;

/// Default tool monotony ratio threshold.
const DEFAULT_MONOTONY_THRESHOLD: f64 = 0.7;

/// Default user-correction count threshold.
const DEFAULT_CORRECTION_THRESHOLD: usize = 3;

/// Default consecutive failure streak threshold.
const DEFAULT_FAILURE_STREAK_THRESHOLD: usize = 3;

/// Default low-confidence score threshold.
const DEFAULT_LOW_CONFIDENCE_THRESHOLD: f64 = 0.3;

/// Default weight for goal-stagnation signal.
const DEFAULT_WEIGHT_GOAL_STAGNATION: f64 = 0.25;

/// Default weight for tool-monotony signal.
const DEFAULT_WEIGHT_TOOL_MONOTONY: f64 = 0.25;

/// Default weight for correction-frequency signal.
const DEFAULT_WEIGHT_CORRECTION: f64 = 0.20;

/// Default weight for low-confidence signal.
const DEFAULT_WEIGHT_LOW_CONFIDENCE: f64 = 0.15;

/// Default weight for failure-streak signal.
const DEFAULT_WEIGHT_FAILURE_STREAK: f64 = 0.15;

/// Default RPE low-utility threshold.
const DEFAULT_LOW_UTILITY_THRESHOLD: f64 = 0.5;

/// Default RPE drift ratio threshold.
const DEFAULT_DRIFT_RATIO_THRESHOLD: f64 = 10.0;

/// Default health recovery dimension threshold.
const DEFAULT_DIMENSION_THRESHOLD: f64 = 0.7;

/// Default consecutive denial threshold for pause suggestion.
const DEFAULT_CONSECUTIVE_DENIAL_THRESHOLD: usize = 3;

/// Default session denial threshold for escalation.
const DEFAULT_SESSION_DENIAL_THRESHOLD: usize = 10;

/// Default UI prompt symbol.
const DEFAULT_PROMPT_SYMBOL: &str = "cortex> ";

/// Default UI locale.
const DEFAULT_LOCALE: &str = "auto";

/// Default per-session rate limit (requests per minute).
const DEFAULT_PER_SESSION_RPM: usize = 10;

/// Default global rate limit (requests per minute).
const DEFAULT_GLOBAL_RPM: usize = 60;

/// Default auth token expiry in hours.
const DEFAULT_TOKEN_EXPIRY_HOURS: u64 = 24;

/// Default health check interval in turns.
const DEFAULT_HEALTH_CHECK_INTERVAL_TURNS: usize = 10;

/// Default health degraded threshold.
const DEFAULT_HEALTH_DEGRADED_THRESHOLD: f64 = 0.3;

/// Default health dimension weight (equal across 4 dimensions).
const DEFAULT_HEALTH_WEIGHT: f64 = 0.25;

/// Default max tool iterations per turn.
const DEFAULT_MAX_TOOL_ITERATIONS: usize = 1024;

/// Default tool execution timeout in seconds.
const DEFAULT_TOOL_TIMEOUT_SECS: u64 = 1800;

/// Default foreground turn execution timeout in seconds. Zero disables it.
const DEFAULT_TURN_EXECUTION_TIMEOUT_SECS: u64 = 0;

/// Default transient LLM retry count for a single request.
pub const DEFAULT_LLM_TRANSIENT_RETRIES: usize = 5;

/// Default max active skill summaries.
const DEFAULT_MAX_ACTIVE_SUMMARIES: usize = 30;

/// Default skill execution timeout in seconds.
const DEFAULT_SKILL_TIMEOUT_SECS: u64 = 600;

/// Default daemon listen address (OS-assigned port).
const DEFAULT_DAEMON_ADDR: &str = "127.0.0.1:0";

/// Evolution signal weight: user correction detected.
const DEFAULT_CORRECTION_WEIGHT: f64 = 1.0;

/// Evolution signal weight: explicit preference stated.
const DEFAULT_PREFERENCE_WEIGHT: f64 = 0.8;

/// Evolution signal weight: new domain detected.
const DEFAULT_NEW_DOMAIN_WEIGHT: f64 = 0.6;

/// Evolution signal weight: first turn of session.
const DEFAULT_FIRST_SESSION_WEIGHT: f64 = 0.5;

/// Evolution signal weight: tool-intensive turn.
const DEFAULT_TOOL_INTENSIVE_WEIGHT: f64 = 0.4;

/// Evolution signal weight: long user input.
const DEFAULT_LONG_INPUT_WEIGHT: f64 = 0.3;

// ── Cortex Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct CortexConfig {
    // ── Essentials (top of config.toml) ──
    pub daemon: DaemonSection,
    pub api: ApiConfig,
    pub embedding: EmbeddingConfig,
    pub web: WebConfig,
    pub plugins: PluginsConfig,

    // ── LLM routing & external tools ──
    #[serde(default)]
    pub llm_groups: HashMap<String, LlmGroupConfig>,
    pub mcp: McpConfig,
    pub acp: AcpConfig,

    // ── Cognitive engine ──
    pub memory: MemoryConfig,
    pub turn: TurnSection,
    pub metacognition: MetacognitionConfig,
    pub autonomous: AutonomousConfig,
    pub context: ContextConfig,
    pub skills: SkillsConfig,

    // ── Security & limits ──
    pub auth: AuthConfig,
    pub tls: TlsConfig,
    pub risk: RiskConfig,
    pub rate_limit: RateLimitConfig,

    // ── Remaining ──
    pub tools: ToolsConfig,
    pub health: HealthConfig,
    pub evolution: EvolutionConfig,
    pub ui: UiConfig,
    pub memory_share: MemoryShareConfig,
    pub media: MediaConfig,
}

// ── Daemon Config ──

/// Daemon server configuration persisted in `config.toml`.
///
/// Default `addr` is `127.0.0.1:0` -- the OS assigns a random
/// available port.  This is required for multi-instance support
/// (`--id`): each instance gets its own port without conflict.
/// After first bind the actual address is persisted to config.toml,
/// so subsequent starts use the same port.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DaemonSection {
    /// Listen address (default: `127.0.0.1:0` -- random port).
    pub addr: String,
    /// Maintenance cycle interval in seconds (default: 1800 = 30 min).
    pub maintenance_interval_secs: u64,
    /// Model info cache TTL in hours (default: 168 = 7 days).
    pub model_info_ttl_hours: u64,
}

impl Default for DaemonSection {
    fn default() -> Self {
        Self {
            addr: DEFAULT_DAEMON_ADDR.into(),
            maintenance_interval_secs: 1800,
            model_info_ttl_hours: 168,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SkillsConfig {
    /// Extra directories to search for skills.
    pub extra_dirs: Vec<String>,
    /// Maximum skill summaries injected into system prompt.
    pub max_active_summaries: usize,
    /// Default execution timeout for skills (seconds).
    pub default_timeout_secs: u64,
    /// Whether to inject skill summaries into system prompt.
    pub inject_summaries: bool,
}

impl Default for SkillsConfig {
    fn default() -> Self {
        Self {
            extra_dirs: Vec::new(),
            max_active_summaries: DEFAULT_MAX_ACTIVE_SUMMARIES,
            default_timeout_secs: DEFAULT_SKILL_TIMEOUT_SECS,
            inject_summaries: true,
        }
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RateLimitConfig {
    pub per_session_rpm: usize,
    pub global_rpm: usize,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            per_session_rpm: DEFAULT_PER_SESSION_RPM,
            global_rpm: DEFAULT_GLOBAL_RPM,
        }
    }
}

const fn default_auto_approve_up_to() -> RiskLevel {
    RiskLevel::Review
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RiskConfig {
    /// Per-tool risk policy overrides keyed by tool name.
    pub tools: HashMap<String, ToolRiskPolicy>,
    /// If non-empty, only matching tool names are eligible to run.
    pub allow: Vec<String>,
    /// Matching tool names are always blocked.
    pub deny: Vec<String>,
    /// Highest non-block risk level that can run without user confirmation.
    #[serde(default = "default_auto_approve_up_to")]
    pub auto_approve_up_to: RiskLevel,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            tools: HashMap::new(),
            allow: Vec::new(),
            deny: Vec::new(),
            auto_approve_up_to: default_auto_approve_up_to(),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolRiskPolicy {
    /// Override the base tool risk axis.
    pub tool_risk: Option<f32>,
    /// Override the file sensitivity axis.
    pub file_sensitivity: Option<f32>,
    /// Override the blast radius axis.
    pub blast_radius: Option<f32>,
    /// Override the irreversibility axis.
    pub irreversibility: Option<f32>,
    /// Force at least `RequireConfirmation` regardless of composite score.
    pub require_confirmation: bool,
    /// Block the tool regardless of composite score.
    pub block: bool,
    /// Whether this tool is allowed in background execution contexts.
    pub allow_background: bool,
}

/// Plugin system configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginsConfig {
    /// Plugin directory relative to `CORTEX_HOME` (default: `"plugins"`).
    /// Shared across all instances.
    pub dir: String,
    /// Plugins enabled for this instance (by name from manifest).
    /// Only plugins in this list are loaded at startup.
    pub enabled: Vec<String>,
}

impl Default for PluginsConfig {
    fn default() -> Self {
        Self {
            dir: "plugins".into(),
            enabled: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub secret: String,
    pub token_expiry_hours: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            secret: String::new(),
            token_expiry_hours: DEFAULT_TOKEN_EXPIRY_HOURS,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
    /// Activation profile for sub-endpoints. Default: `full` (all enabled).
    pub preset: LlmPreset,
    /// Per-endpoint enabled/disabled. Preset sets defaults; manual overrides here.
    #[serde(default)]
    pub endpoints: HashMap<String, bool>,
    /// Per-endpoint LLM group override. Key = endpoint name, value = group name.
    #[serde(default)]
    pub endpoint_groups: HashMap<String, String>,
    /// Vision model config (provider + model for vision, not an LLM sub-endpoint).
    pub vision: VisionOverride,
}

impl Default for ApiConfig {
    fn default() -> Self {
        Self {
            provider: DEFAULT_PROVIDER.into(),
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
            max_tokens: 0,
            preset: LlmPreset::default(),
            endpoints: HashMap::new(),
            endpoint_groups: HashMap::new(),
            vision: VisionOverride::default(),
        }
    }
}

impl ApiConfig {
    /// Apply preset: enable endpoints from preset unless manually set in `endpoints`.
    pub fn apply_preset(&mut self) {
        for name in self.preset.enabled_endpoints() {
            self.endpoints.entry((*name).to_string()).or_insert(true);
        }
    }

    /// Check if a named endpoint is enabled.
    #[must_use]
    pub fn is_endpoint_enabled(&self, name: &str) -> bool {
        self.endpoints.get(name).copied().unwrap_or(false)
    }

    /// Get the LLM group name for a named endpoint (empty = use main api config).
    #[must_use]
    pub fn endpoint_group(&self, name: &str) -> Option<&str> {
        self.endpoint_groups
            .get(name)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ApiEndpointConfig {
    /// Whether this sub-endpoint is enabled.  Disabled endpoints skip the
    /// LLM call and log a warning.  Default: `false`.
    pub enabled: bool,
    /// Reference to a named group in `[llm_groups]`. When set, inherits
    /// `provider`/`model`/`max_tokens` from the group (unless overridden here).
    pub group: String,
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
}

/// Predefined endpoint activation profiles.
///
/// Set via `[api].preset` in config.toml or `CORTEX_LLM_PRESET` env on first run.
///
/// | Preset | Enabled endpoints |
/// |--------|-------------------|
/// | `minimal` | (none -- main LLM only) |
/// | `standard` | memory_extract, compress, entity_extract |
/// | `cognitive` | standard + self_update, causal_analyze, autonomous |
/// | `full` | all 7 sub-endpoints |
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LlmPreset {
    Minimal,
    Standard,
    Cognitive,
    #[default]
    Full,
}

impl LlmPreset {
    /// Endpoint names enabled by this preset.
    #[must_use]
    pub const fn enabled_endpoints(&self) -> &[&str] {
        match self {
            Self::Minimal => &[],
            Self::Standard => &["memory_extract", "compress", "entity_extract"],
            Self::Cognitive => &[
                "memory_extract",
                "compress",
                "entity_extract",
                "self_update",
                "causal_analyze",
                "autonomous",
            ],
            Self::Full => &[
                "memory_extract",
                "entity_extract",
                "compress",
                "summary",
                "self_update",
                "causal_analyze",
                "autonomous",
            ],
        }
    }
}

/// Vision model override. If both fields are empty, auto-discovery is used.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct VisionOverride {
    pub provider: String,
    pub model: String,
}

/// User-defined LLM endpoint group (e.g., "main", "light").
/// Defined in `[llm_groups]` section of config.toml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmGroupConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
    /// Declared model capabilities. Empty means Cortex infers conservative
    /// defaults from provider protocol, group name, model name, and profile
    /// scores.
    #[serde(default)]
    pub capabilities: Vec<ModelCapability>,
    /// Declared input context window. `0` means infer from runtime defaults.
    pub context_tokens: usize,
    /// Declared output token ceiling. `0` means infer from runtime defaults.
    pub output_tokens: usize,
    /// Expected median latency in milliseconds. `0` means infer by group tier.
    pub latency_ms: u32,
    /// Input cost per million tokens. `0` means infer by group tier.
    pub input_cost_per_million: f32,
    /// Output cost per million tokens. `0` means infer by group tier.
    pub output_cost_per_million: f32,
    /// Safety score in `[0, 1]`. `0` means infer by group tier/model name.
    pub safety_score: f32,
    /// Reasoning depth score in `[0, 1]`. `0` means infer by group tier/model name.
    pub reasoning_depth: f32,
    /// Structured-output reliability score in `[0, 1]`. `0` means infer by protocol.
    pub json_reliability: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WebConfig {
    /// Search backend: `"brave"` for Brave Search API, `"llm"` for model knowledge.
    pub search_backend: String,
    /// Brave Search API key. If set, `web_search` uses Brave for real-time results.
    /// If empty, falls back to LLM-based search (model knowledge only).
    pub brave_api_key: String,
    pub brave_max_results: usize,
    /// Hard limit on search results — LLM cannot exceed this.
    pub brave_max_results_limit: usize,
    /// Default max characters for `web_fetch` content (LLM can override per-call).
    pub fetch_max_chars: usize,
    /// Hard limit — no request can exceed this regardless of LLM or config values.
    pub fetch_max_chars_limit: usize,
}

impl Default for WebConfig {
    fn default() -> Self {
        Self {
            search_backend: "brave".into(),
            brave_api_key: String::new(),
            brave_max_results: DEFAULT_BRAVE_MAX_RESULTS,
            brave_max_results_limit: 20,
            fetch_max_chars: 100_000,
            fetch_max_chars_limit: 500_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EmbeddingConfig {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    pub dimensions: usize,
    /// Candidate embedding models for auto-selection based on recall precision.
    #[serde(default)]
    pub candidates: Vec<String>,
    /// Minimum sample count before a candidate model is eligible for selection.
    pub min_samples: u32,
    /// Enable automatic model switching based on precision data.
    pub auto_switch: bool,
    /// Minimum samples before considering a switch.
    pub switch_threshold_samples: u32,
    /// Minimum precision improvement to trigger a switch.
    pub switch_precision_delta: f64,
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            provider: DEFAULT_EMBEDDING_PROVIDER.into(),
            api_key: String::new(),
            model: DEFAULT_EMBEDDING_MODEL.into(),
            dimensions: 0,
            candidates: Vec::new(),
            min_samples: DEFAULT_EMBEDDING_MIN_SAMPLES,
            auto_switch: false,
            switch_threshold_samples: DEFAULT_SWITCH_THRESHOLD_SAMPLES,
            switch_precision_delta: DEFAULT_SWITCH_PRECISION_DELTA,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub max_tokens: usize,
    pub pressure_thresholds: Vec<f64>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: DEFAULT_CONTEXT_MAX_TOKENS,
            pressure_thresholds: DEFAULT_PRESSURE_THRESHOLDS.to_vec(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryConfig {
    pub max_recall: usize,
    pub decay_rate: f64,
    pub auto_extract: bool,
    pub extract_min_turns: usize,
    pub consolidate_interval_hours: u64,
    pub consolidation_similarity_threshold: f64,
    pub semantic_upgrade_similarity_threshold: f64,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            max_recall: DEFAULT_MAX_RECALL,
            decay_rate: DEFAULT_DECAY_RATE,
            auto_extract: true,
            extract_min_turns: DEFAULT_EXTRACT_MIN_TURNS,
            consolidate_interval_hours: DEFAULT_CONSOLIDATE_INTERVAL_HOURS,
            consolidation_similarity_threshold: DEFAULT_CONSOLIDATION_SIMILARITY_THRESHOLD,
            semantic_upgrade_similarity_threshold: DEFAULT_SEMANTIC_UPGRADE_SIMILARITY_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MetacognitionConfig {
    pub doom_loop_threshold: usize,
    pub duration_limit_secs: u64,
    pub fatigue_threshold: f64,
    pub frame_anchoring_threshold: f64,
    pub frame_audit: FrameAuditConfig,
    pub rpe: RpeConfig,
    pub health_recovery: HealthRecoveryConfig,
    pub denial: DenialConfig,
}

impl Default for MetacognitionConfig {
    fn default() -> Self {
        Self {
            doom_loop_threshold: DEFAULT_DOOM_LOOP_THRESHOLD,
            duration_limit_secs: DEFAULT_DURATION_LIMIT_SECS,
            fatigue_threshold: DEFAULT_FATIGUE_THRESHOLD,
            frame_anchoring_threshold: DEFAULT_FRAME_ANCHORING_THRESHOLD,
            frame_audit: FrameAuditConfig::default(),
            rpe: RpeConfig::default(),
            health_recovery: HealthRecoveryConfig::default(),
            denial: DenialConfig::default(),
        }
    }
}

/// Configuration for frame-audit signal thresholds and weights.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameAuditConfig {
    pub goal_stagnation_threshold: usize,
    pub monotony_threshold: f64,
    pub correction_threshold: usize,
    pub failure_streak_threshold: usize,
    pub low_confidence_threshold: f64,
    pub weight_goal_stagnation: f64,
    pub weight_tool_monotony: f64,
    pub weight_correction: f64,
    pub weight_low_confidence: f64,
    pub weight_failure_streak: f64,
}

impl Default for FrameAuditConfig {
    fn default() -> Self {
        Self {
            goal_stagnation_threshold: DEFAULT_GOAL_STAGNATION_THRESHOLD,
            monotony_threshold: DEFAULT_MONOTONY_THRESHOLD,
            correction_threshold: DEFAULT_CORRECTION_THRESHOLD,
            failure_streak_threshold: DEFAULT_FAILURE_STREAK_THRESHOLD,
            low_confidence_threshold: DEFAULT_LOW_CONFIDENCE_THRESHOLD,
            weight_goal_stagnation: DEFAULT_WEIGHT_GOAL_STAGNATION,
            weight_tool_monotony: DEFAULT_WEIGHT_TOOL_MONOTONY,
            weight_correction: DEFAULT_WEIGHT_CORRECTION,
            weight_low_confidence: DEFAULT_WEIGHT_LOW_CONFIDENCE,
            weight_failure_streak: DEFAULT_WEIGHT_FAILURE_STREAK,
        }
    }
}

/// Configuration for RPE-based tool utility tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RpeConfig {
    pub low_utility_threshold: f64,
    pub drift_ratio_threshold: f64,
}

impl Default for RpeConfig {
    fn default() -> Self {
        Self {
            low_utility_threshold: DEFAULT_LOW_UTILITY_THRESHOLD,
            drift_ratio_threshold: DEFAULT_DRIFT_RATIO_THRESHOLD,
        }
    }
}

/// Configuration for health recovery dimension thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthRecoveryConfig {
    pub dimension_threshold: f64,
}

impl Default for HealthRecoveryConfig {
    fn default() -> Self {
        Self {
            dimension_threshold: DEFAULT_DIMENSION_THRESHOLD,
        }
    }
}

/// Configuration for permission denial tracking thresholds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct DenialConfig {
    pub consecutive_threshold: usize,
    pub session_threshold: usize,
}

impl Default for DenialConfig {
    fn default() -> Self {
        Self {
            consecutive_threshold: DEFAULT_CONSECUTIVE_DENIAL_THRESHOLD,
            session_threshold: DEFAULT_SESSION_DENIAL_THRESHOLD,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub prompt_symbol: String,
    pub locale: String,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            prompt_symbol: DEFAULT_PROMPT_SYMBOL.into(),
            locale: DEFAULT_LOCALE.into(),
        }
    }
}

// ── Embedding Performance ──

/// Per-model recall performance statistics for embedding model selection.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EmbeddingPerformance {
    pub model: String,
    pub hit_count: u32,
    pub miss_count: u32,
    pub total_similarity: f64,
    pub query_count: u32,
}

impl EmbeddingPerformance {
    #[must_use]
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
            ..Default::default()
        }
    }

    /// Recall precision: hits / (hits + misses). Returns 0.0 if no data.
    #[must_use]
    pub fn precision(&self) -> f64 {
        let total = self.hit_count + self.miss_count;
        if total == 0 {
            return 0.0;
        }
        f64::from(self.hit_count) / f64::from(total)
    }

    /// Average cosine similarity of successful recalls. Returns 0.0 if no hits.
    #[must_use]
    pub fn avg_similarity(&self) -> f64 {
        if self.hit_count == 0 {
            return 0.0;
        }
        self.total_similarity / f64::from(self.hit_count)
    }

    /// Total number of recall attempts (hits + misses).
    #[must_use]
    pub const fn sample_count(&self) -> u32 {
        self.hit_count + self.miss_count
    }
}

// ── Health Types ──

/// Session-level health report with 5-dimensional assessment.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    /// Ratio of deprecated/low-strength memories (0.0 = healthy, 1.0 = heavily fragmented).
    pub memory_fragmentation: f64,
    /// Sliding average of context occupancy (0.0 = low pressure, 1.0 = sustained overload).
    pub context_pressure_trend: f64,
    /// Recall precision trend indicator (0.0 = no degradation, 1.0 = severe degradation).
    pub recall_degradation: f64,
    /// Fatigue level relative to threshold (0.0 = fresh, 1.0 = exhausted).
    pub fatigue_trend: f64,
    /// Weighted combination of all dimensions (0.0 = critical, 1.0 = excellent).
    pub overall_health: f64,
}

// ── Health Config ──

/// Configuration for periodic session health self-checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HealthConfig {
    /// Run health check every N turns.
    pub check_interval_turns: usize,
    /// Overall health score below this triggers `HealthDegraded` alert.
    pub degraded_threshold: f64,
    /// Weights for [`memory_fragmentation`, `context_pressure`, `recall_degradation`, `fatigue`].
    pub weights: Vec<f64>,
}

impl Default for HealthConfig {
    fn default() -> Self {
        Self {
            check_interval_turns: DEFAULT_HEALTH_CHECK_INTERVAL_TURNS,
            degraded_threshold: DEFAULT_HEALTH_DEGRADED_THRESHOLD,
            weights: vec![
                DEFAULT_HEALTH_WEIGHT,
                DEFAULT_HEALTH_WEIGHT,
                DEFAULT_HEALTH_WEIGHT,
                DEFAULT_HEALTH_WEIGHT,
            ],
        }
    }
}

// ── Evolution Config ──

/// Configuration for self-evolution capabilities.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EvolutionConfig {
    /// Allow modifying Rust source files (.rs). When false, only prompt
    /// templates (prompts/system/*.md) can be self-modified. Default: false.
    pub source_modify_enabled: bool,
    /// Signal weight: user correction detected (default 1.0).
    #[serde(default = "default_correction_weight")]
    pub correction_weight: f64,
    /// Signal weight: explicit preference stated (default 0.8).
    #[serde(default = "default_preference_weight")]
    pub preference_weight: f64,
    /// Signal weight: new domain detected (default 0.6).
    #[serde(default = "default_new_domain_weight")]
    pub new_domain_weight: f64,
    /// Signal weight: first turn of session (default 0.5).
    #[serde(default = "default_first_session_weight")]
    pub first_session_weight: f64,
    /// Signal weight: tool-intensive turn (default 0.4).
    #[serde(default = "default_tool_intensive_weight")]
    pub tool_intensive_weight: f64,
    /// Signal weight: long user input (default 0.3).
    #[serde(default = "default_long_input_weight")]
    pub long_input_weight: f64,
}

impl Default for EvolutionConfig {
    fn default() -> Self {
        Self {
            source_modify_enabled: false,
            correction_weight: DEFAULT_CORRECTION_WEIGHT,
            preference_weight: DEFAULT_PREFERENCE_WEIGHT,
            new_domain_weight: DEFAULT_NEW_DOMAIN_WEIGHT,
            first_session_weight: DEFAULT_FIRST_SESSION_WEIGHT,
            tool_intensive_weight: DEFAULT_TOOL_INTENSIVE_WEIGHT,
            long_input_weight: DEFAULT_LONG_INPUT_WEIGHT,
        }
    }
}

impl EvolutionConfig {
    /// Return the six signal weights as an ordered array.
    #[must_use]
    pub const fn signal_weights(&self) -> [f64; 6] {
        [
            self.correction_weight,
            self.preference_weight,
            self.new_domain_weight,
            self.first_session_weight,
            self.tool_intensive_weight,
            self.long_input_weight,
        ]
    }
}

const fn default_correction_weight() -> f64 {
    DEFAULT_CORRECTION_WEIGHT
}
const fn default_preference_weight() -> f64 {
    DEFAULT_PREFERENCE_WEIGHT
}
const fn default_new_domain_weight() -> f64 {
    DEFAULT_NEW_DOMAIN_WEIGHT
}
const fn default_first_session_weight() -> f64 {
    DEFAULT_FIRST_SESSION_WEIGHT
}
const fn default_tool_intensive_weight() -> f64 {
    DEFAULT_TOOL_INTENSIVE_WEIGHT
}
const fn default_long_input_weight() -> f64 {
    DEFAULT_LONG_INPUT_WEIGHT
}

// ── TLS Config ──

/// TLS configuration for HTTPS transport.
///
/// When `enabled` is true, the server loads PEM-encoded certificate and key
/// files from `cert_path` and `key_path` and serves HTTPS.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TlsConfig {
    /// Enable TLS (HTTPS). Default: false.
    pub enabled: bool,
    /// Path to PEM-encoded certificate chain file.
    pub cert_path: Option<String>,
    /// Path to PEM-encoded private key file.
    pub key_path: Option<String>,
}

// ── Memory Share Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum MemoryShareMode {
    #[default]
    Disabled,
    Readonly,
    Readwrite,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MemoryShareConfig {
    pub mode: MemoryShareMode,
    pub instance_id: String,
}

impl Default for MemoryShareConfig {
    fn default() -> Self {
        Self {
            mode: MemoryShareMode::Disabled,
            instance_id: String::new(),
        }
    }
}

// ── Vision Capability ──

/// Cached result of vision model capability discovery.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionCapability {
    pub supported: bool,
    pub model_id: String,
    pub probed_at: chrono::DateTime<chrono::Utc>,
}

// ── Tools Config ──

/// Global tool configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    /// Tools to disable by name. Disabled tools are not registered and
    /// invisible to the LLM. Example: `["self_modify", "cron_schedule"]`.
    pub disabled: Vec<String>,
}
