use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::RiskLevel;

// ── Named Constants ──

/// Default output `max_tokens` fallback when neither endpoint nor parent specifies a value.
pub const DEFAULT_MAX_TOKENS_FALLBACK: usize = 300_000;

/// Safe default output token cap for multimodal/vision requests.
pub const DEFAULT_VISION_MAX_OUTPUT_TOKENS: usize = 8192;

/// Default context window size (input tokens).
pub const DEFAULT_CONTEXT_MAX_TOKENS: usize = 200_000;

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

// ── Provider Registry ──

/// Provider registry: maps provider name to its configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderRegistry(HashMap<String, ProviderConfig>);

impl ProviderRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self(HashMap::new())
    }

    pub fn insert(&mut self, name: String, config: ProviderConfig) {
        self.0.insert(name, config);
    }

    #[must_use]
    pub fn get(&self, name: &str) -> Option<&ProviderConfig> {
        self.0.get(name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut ProviderConfig> {
        self.0.get_mut(name)
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Find a provider whose `base_url` contains the given URL (or vice versa).
    #[must_use]
    pub fn find_by_url(&self, url: &str) -> Option<(String, &ProviderConfig)> {
        let normalized = url.trim_end_matches('/');
        self.0.iter().find_map(|(name, cfg)| {
            let cfg_url = cfg.base_url.trim_end_matches('/');
            if cfg_url == normalized
                || normalized.starts_with(cfg_url)
                || cfg_url.starts_with(normalized)
            {
                Some((name.clone(), cfg))
            } else {
                None
            }
        })
    }

    #[must_use = "iterators are lazy and do nothing unless consumed"]
    pub fn iter(&self) -> impl Iterator<Item = (&String, &ProviderConfig)> {
        self.0.iter()
    }
}

// ── Provider Registry Types ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub name: String,
    pub protocol: ProviderProtocol,
    pub base_url: String,
    pub auth_type: AuthType,
    pub models: Vec<String>,
    /// Provider used for vision requests. Empty = use this provider.
    ///
    /// This keeps multimodal routing explicit. Some gateways expose a good
    /// text endpoint and a separate OpenAI-compatible vision endpoint.
    pub vision_provider: String,
    /// Default vision model for this provider. Empty = auto-discovery.
    pub vision_model: String,
    /// How OpenAI-compatible multimodal image blocks should be sent.
    pub image_input_mode: OpenAiImageInputMode,
    /// Optional base URL used for file upload/content URLs when
    /// `image_input_mode = "upload-then-url"`.
    pub files_base_url: String,
    /// Whether this OpenAI-compatible endpoint accepts `stream_options`.
    ///
    /// Many gateways implement chat-completions streaming but reject `OpenAI`'s
    /// optional usage extension. Keep this explicit instead of hard-coding
    /// provider names in the LLM client.
    pub openai_stream_options: bool,
    /// Provider-specific maximum output tokens for multimodal/vision requests.
    /// `0` means use [`DEFAULT_VISION_MAX_OUTPUT_TOKENS`].
    pub vision_max_output_tokens: usize,
    /// Capability cache TTL in hours. `0` means use the runtime default.
    pub capability_cache_ttl_hours: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OpenAiImageInputMode {
    #[default]
    DataUrl,
    UploadThenUrl,
    RemoteUrlOnly,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ProviderProtocol {
    Anthropic,
    #[default]
    #[serde(rename = "openai")]
    OpenAI,
    Ollama,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum AuthType {
    XApiKey,
    #[default]
    Bearer,
    None,
}

/// Fully resolved API endpoint after fallback chain.
#[derive(Debug, Clone)]
pub struct ResolvedEndpoint {
    pub provider: String,
    pub base_url: String,
    pub protocol: ProviderProtocol,
    pub auth_type: AuthType,
    pub api_key: String,
    pub model: String,
    pub max_tokens: usize,
    pub vision_max_output_tokens: usize,
    pub image_input_mode: OpenAiImageInputMode,
    pub files_base_url: String,
    pub openai_stream_options: bool,
    pub capability_cache_path: String,
    pub capability_cache_ttl_hours: u64,
}

impl ResolvedEndpoint {
    fn from_provider(
        provider_name: &str,
        provider: &ProviderConfig,
        api_key: String,
        model: String,
        max_tokens: usize,
    ) -> Self {
        Self {
            provider: provider_name.to_string(),
            base_url: provider.base_url.clone(),
            protocol: provider.protocol.clone(),
            auth_type: provider.auth_type.clone(),
            api_key,
            model,
            max_tokens,
            vision_max_output_tokens: provider.vision_max_output_tokens,
            image_input_mode: provider.image_input_mode.clone(),
            files_base_url: provider.files_base_url.clone(),
            openai_stream_options: provider.openai_stream_options,
            capability_cache_path: String::new(),
            capability_cache_ttl_hours: provider.capability_cache_ttl_hours,
        }
    }

    #[must_use]
    pub fn with_capability_cache_path(mut self, path: String) -> Self {
        self.capability_cache_path = path;
        self
    }

    #[must_use]
    pub const fn supports_direct_image_input(&self) -> bool {
        match self.protocol {
            ProviderProtocol::Anthropic | ProviderProtocol::Ollama => true,
            ProviderProtocol::OpenAI => matches!(
                self.image_input_mode,
                OpenAiImageInputMode::DataUrl | OpenAiImageInputMode::UploadThenUrl
            ),
        }
    }

    #[must_use]
    pub const fn requires_remote_image_url(&self) -> bool {
        matches!(self.image_input_mode, OpenAiImageInputMode::RemoteUrlOnly)
    }
}

impl ResolvedEndpoint {
    /// Resolve a sub-config endpoint by filling empty fields from parent `[api]`,
    /// then looking up provider in the registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve(
        endpoint: &ApiEndpointConfig,
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Self, String> {
        Self::resolve_with_groups(endpoint, parent, providers, &HashMap::new())
    }

    /// Resolve an endpoint with group inheritance.
    ///
    /// Priority: endpoint field > group field > parent [api] field > default.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_with_groups(
        endpoint: &ApiEndpointConfig,
        parent: &ApiConfig,
        providers: &ProviderRegistry,
        groups: &HashMap<String, LlmGroupConfig>,
    ) -> Result<Self, String> {
        // Resolve group inheritance: group fills empty fields
        let group = if endpoint.group.is_empty() {
            None
        } else {
            groups.get(&endpoint.group)
        };

        let provider_name = if !endpoint.provider.is_empty() {
            &endpoint.provider
        } else if let Some(g) = group
            && !g.provider.is_empty()
        {
            &g.provider
        } else {
            &parent.provider
        };
        let api_key = if !endpoint.api_key.is_empty() {
            endpoint.api_key.clone()
        } else if let Some(g) = group
            && !g.api_key.is_empty()
        {
            g.api_key.clone()
        } else {
            parent.api_key.clone()
        };

        let model = if !endpoint.model.is_empty() {
            endpoint.model.clone()
        } else if let Some(g) = group
            && !g.model.is_empty()
        {
            g.model.clone()
        } else {
            parent.model.clone()
        };

        let max_tokens = if endpoint.max_tokens > 0 {
            endpoint.max_tokens
        } else if let Some(g) = group
            && g.max_tokens > 0
        {
            g.max_tokens
        } else if parent.max_tokens > 0 {
            parent.max_tokens
        } else {
            DEFAULT_MAX_TOKENS_FALLBACK
        };
        let provider = providers
            .get(provider_name)
            .ok_or_else(|| format!("provider not found: {provider_name}"))?;
        Ok(Self::from_provider(
            provider_name,
            provider,
            api_key,
            model,
            max_tokens,
        ))
    }

    /// Resolve the vision model override, returning `None` if not configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_vision(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Option<Self>, String> {
        if parent.vision.model.is_empty() && parent.vision.provider.is_empty() {
            return Ok(None);
        }
        // Convert VisionOverride to ApiEndpointConfig for resolution
        let endpoint = ApiEndpointConfig {
            provider: parent.vision.provider.clone(),
            model: parent.vision.model.clone(),
            ..ApiEndpointConfig::default()
        };
        Self::resolve(&endpoint, parent, providers).map(Some)
    }

    /// Resolve the primary `[api]` config directly.
    ///
    /// # Errors
    ///
    /// Returns an error if the provider name is not found in the registry.
    pub fn resolve_primary(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Self, String> {
        let api_key = parent.api_key.clone();
        let provider = providers
            .get(&parent.provider)
            .ok_or_else(|| format!("provider not found: {}", parent.provider))?;
        Ok(Self::from_provider(
            &parent.provider,
            provider,
            api_key,
            parent.model.clone(),
            if parent.max_tokens == 0 {
                DEFAULT_MAX_TOKENS_FALLBACK
            } else {
                parent.max_tokens
            },
        ))
    }

    /// Resolve the best vision endpoint for the primary API route.
    ///
    /// Priority:
    /// 1. Explicit `[api.vision]`.
    /// 2. Provider-declared `vision_provider`.
    /// 3. Provider-declared `vision_model` on the primary provider.
    ///
    /// # Errors
    ///
    /// Returns an error when an explicitly referenced provider is missing.
    pub fn resolve_vision_endpoint(
        parent: &ApiConfig,
        providers: &ProviderRegistry,
    ) -> Result<Option<Self>, String> {
        if let Some(endpoint) = Self::resolve_vision(parent, providers)? {
            return Ok(Some(endpoint));
        }

        let primary_provider = providers
            .get(&parent.provider)
            .ok_or_else(|| format!("provider not found: {}", parent.provider))?;

        if !primary_provider.vision_provider.is_empty() {
            let vision_provider_name = &primary_provider.vision_provider;
            let vision_provider = providers
                .get(vision_provider_name)
                .ok_or_else(|| format!("provider not found: {vision_provider_name}"))?;
            if !vision_provider.vision_model.is_empty() {
                return Ok(Some(Self::from_provider(
                    vision_provider_name,
                    vision_provider,
                    parent.api_key.clone(),
                    vision_provider.vision_model.clone(),
                    if parent.max_tokens == 0 {
                        DEFAULT_MAX_TOKENS_FALLBACK
                    } else {
                        parent.max_tokens
                    },
                )));
            }
        }

        if !primary_provider.vision_model.is_empty() {
            return Ok(Some(Self::from_provider(
                &parent.provider,
                primary_provider,
                parent.api_key.clone(),
                primary_provider.vision_model.clone(),
                if parent.max_tokens == 0 {
                    DEFAULT_MAX_TOKENS_FALLBACK
                } else {
                    parent.max_tokens
                },
            )));
        }

        Ok(None)
    }
}

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
    /// Whether to strip `<think>…</think>` tags from LLM output.
    /// Defaults to `true`.  Can be toggled per-session via `/think` command.
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
    RiskLevel::Allow
}

const fn default_confirmation_timeout_secs() -> u64 {
    300
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
    /// How long interactive confirmations may wait before denial.
    #[serde(default = "default_confirmation_timeout_secs")]
    pub confirmation_timeout_secs: u64,
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            tools: HashMap::new(),
            allow: Vec::new(),
            deny: Vec::new(),
            auto_approve_up_to: default_auto_approve_up_to(),
            confirmation_timeout_secs: default_confirmation_timeout_secs(),
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

// ── Media Config ──

/// Configuration for media capabilities with provider-based dispatch.
///
/// Each capability (`stt`, `tts`, `image_gen`, `image_understand`, `video_gen`,
/// `video_understand`) specifies a provider name.  An empty string disables
/// that capability (or uses a built-in fallback where applicable).
///
/// Provider names:
/// - STT: `"local"` (whisper CLI), `"openai"`, `"zai"`
/// - TTS: `"edge"` (edge-tts CLI), `"openai"`, `"zai"`
/// - Image gen: `"zai"`, `"openai"`, `""` (disabled)
/// - Image understand: `"zai"`, `"openai"`, `""` (default = main LLM vision)
/// - Video gen: `"zai"`, `""` (disabled)
/// - Video understand: `"zai"`, `"gemini"`, `""` (disabled)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaConfig {
    /// STT provider: `"local"` (whisper CLI), `"openai"`, `"zai"`.
    pub stt: String,
    /// TTS provider: `"edge"` (edge-tts CLI), `"openai"`, `"zai"`.
    pub tts: String,
    /// Image generation provider: `"zai"`, `"openai"`, `""` (disabled).
    pub image_gen: String,
    /// Video generation provider: `"zai"`, `""` (disabled).
    pub video_gen: String,
    /// Image understanding provider: `"zai"`, `"openai"`, `""` (use main LLM vision).
    ///
    /// Default empty = main LLM handles vision natively (recommended).
    /// Only set a provider when you want a dedicated vision model.
    pub image_understand: String,
    /// Image understanding API key override.
    #[serde(default)]
    pub image_understand_api_key: String,
    /// Image understanding API URL override.
    #[serde(default)]
    pub image_understand_api_url: String,
    /// Image understanding model name (empty = provider default).
    pub image_understand_model: String,

    /// Video understanding provider: `"zai"`, `"gemini"`, `""` (disabled).
    pub video_understand: String,

    /// Shared API key for media providers (empty = inherit from `[api].api_key`).
    pub api_key: String,
    /// Shared API URL override (empty = use provider default).
    pub api_url: String,

    // ── Per-capability overrides (empty = inherit shared/[api]) ──
    /// STT API key override.
    #[serde(default)]
    pub stt_api_key: String,
    /// STT API URL override.
    #[serde(default)]
    pub stt_api_url: String,
    /// Local whisper model name (for `stt = "local"`).
    pub whisper_model: String,

    /// TTS API key override.
    #[serde(default)]
    pub tts_api_key: String,
    /// TTS API URL override.
    #[serde(default)]
    pub tts_api_url: String,
    /// TTS voice identifier (e.g. `"zh-CN-XiaoxiaoNeural"` for edge).
    pub tts_voice: String,

    /// Image generation API key override.
    #[serde(default)]
    pub image_gen_api_key: String,
    /// Image generation API URL override.
    #[serde(default)]
    pub image_gen_api_url: String,
    /// Image generation model name.
    pub image_gen_model: String,

    /// Video generation API key override.
    #[serde(default)]
    pub video_gen_api_key: String,
    /// Video generation API URL override.
    #[serde(default)]
    pub video_gen_api_url: String,
    /// Video generation model name (default: `"cogvideox-3"`).
    pub video_gen_model: String,

    /// Video understanding API key override.
    #[serde(default)]
    pub video_understand_api_key: String,
    /// Video understanding API URL override.
    #[serde(default)]
    pub video_understand_api_url: String,
    /// Video understanding model name (default: `"glm-4v-plus"`).
    pub video_understand_model: String,
}

impl Default for MediaConfig {
    fn default() -> Self {
        Self {
            stt: "local".into(),
            tts: "edge".into(),
            image_gen: String::new(),
            image_understand: String::new(),
            image_understand_api_key: String::new(),
            image_understand_api_url: String::new(),
            image_understand_model: String::new(),
            video_gen: String::new(),
            video_understand: String::new(),
            api_key: String::new(),
            api_url: String::new(),
            stt_api_key: String::new(),
            stt_api_url: String::new(),
            whisper_model: "whisper".into(),
            tts_api_key: String::new(),
            tts_api_url: String::new(),
            tts_voice: "zh-CN-XiaoxiaoNeural".into(),
            image_gen_api_key: String::new(),
            image_gen_api_url: String::new(),
            image_gen_model: String::new(),
            video_gen_api_key: String::new(),
            video_gen_api_url: String::new(),
            video_gen_model: "cogvideox-3".into(),
            video_understand_api_key: String::new(),
            video_understand_api_url: String::new(),
            video_understand_model: "glm-4v-plus".into(),
        }
    }
}

impl MediaConfig {
    /// Resolve API key: `capability_key` > `media.api_key` > `global_fallback`.
    #[must_use]
    pub fn resolve_key<'a>(&'a self, capability_key: &'a str, global_fallback: &'a str) -> &'a str {
        first_non_empty(&[capability_key, &self.api_key, global_fallback])
    }

    /// Resolve API URL: `capability_url` > `media.api_url` > `provider_default`.
    #[must_use]
    pub fn resolve_url<'a>(
        &'a self,
        capability_url: &'a str,
        provider_default: &'a str,
    ) -> &'a str {
        first_non_empty(&[capability_url, &self.api_url, provider_default])
    }

    /// Shorthand for STT key resolution.
    #[must_use]
    pub fn stt_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.stt_api_key, global)
    }

    /// Shorthand for STT URL resolution.
    #[must_use]
    pub fn stt_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.stt_api_url, default)
    }

    /// Shorthand for TTS key resolution.
    #[must_use]
    pub fn tts_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.tts_api_key, global)
    }

    /// Shorthand for TTS URL resolution.
    #[must_use]
    pub fn tts_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.tts_api_url, default)
    }

    /// Shorthand for image generation key resolution.
    #[must_use]
    pub fn image_gen_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.image_gen_api_key, global)
    }

    /// Shorthand for image generation URL resolution.
    #[must_use]
    pub fn image_gen_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.image_gen_api_url, default)
    }

    /// Shorthand for video generation key resolution.
    #[must_use]
    pub fn video_gen_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.video_gen_api_key, global)
    }

    /// Shorthand for video generation URL resolution.
    #[must_use]
    pub fn video_gen_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.video_gen_api_url, default)
    }

    /// Shorthand for image understanding key resolution.
    #[must_use]
    pub fn image_understand_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.image_understand_api_key, global)
    }

    /// Shorthand for image understanding URL resolution.
    #[must_use]
    pub fn image_understand_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.image_understand_api_url, default)
    }

    /// Shorthand for video understanding key resolution.
    #[must_use]
    pub fn video_understand_key<'a>(&'a self, global: &'a str) -> &'a str {
        self.resolve_key(&self.video_understand_api_key, global)
    }

    /// Shorthand for video understanding URL resolution.
    #[must_use]
    pub fn video_understand_url<'a>(&'a self, default: &'a str) -> &'a str {
        self.resolve_url(&self.video_understand_api_url, default)
    }

    /// Backward compat: resolve shared key (for callers not yet updated).
    #[must_use]
    pub fn effective_api_key<'a>(&'a self, fallback: &'a str) -> &'a str {
        first_non_empty(&[&self.api_key, fallback])
    }

    /// Backward compat: resolve shared URL.
    #[must_use]
    pub fn effective_api_url<'a>(&'a self, provider_default: &'a str) -> &'a str {
        first_non_empty(&[&self.api_url, provider_default])
    }
}

/// Return the first non-empty string from the candidates.
fn first_non_empty<'a>(candidates: &[&'a str]) -> &'a str {
    candidates
        .iter()
        .copied()
        .find(|s| !s.is_empty())
        .unwrap_or("")
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

// ── MCP Config ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct McpConfig {
    pub servers: Vec<McpServerConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    pub transport: McpTransportType,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum McpTransportType {
    Stdio,
    Sse,
}

// ── Autonomous Cognition Config ──

/// Configuration for Cortex's autonomous behavior — the heartbeat-driven
/// idle cognition system. When `enabled = false`, Cortex is purely passive
/// (only responds to user input).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AutonomousConfig {
    /// Master switch. `false` disables all autonomous behavior.
    pub enabled: bool,
    /// Heartbeat interval in seconds. Each tick evaluates accumulated state
    /// against thresholds. Most ticks are zero-cost (no state change).
    pub heartbeat_interval_secs: u64,
    /// Thresholds that determine when idle cognition actions trigger.
    pub thresholds: AutonomousThresholds,
    /// Rate limits for autonomous LLM calls.
    pub limits: AutonomousLimits,
}

impl Default for AutonomousConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            heartbeat_interval_secs: 10,
            thresholds: AutonomousThresholds::default(),
            limits: AutonomousLimits::default(),
        }
    }
}

/// Thresholds for heartbeat-driven idle cognition.
/// Each threshold controls when a specific maintenance or cognitive action fires.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct AutonomousThresholds {
    /// Number of pending memories before consolidation triggers (no LLM).
    pub consolidate_count: usize,
    /// Whether to check for expired memories each heartbeat (no LLM).
    pub deprecate_check: bool,
    /// Whether to auto-generate embeddings for un-embedded memories (embedding API, no LLM).
    pub embed_pending: bool,
    /// Tool call accumulation count before Skill evolution triggers (no LLM).
    pub skill_evolve_calls: usize,
    /// Seconds of idle time before deep reflection triggers (requires LLM).
    pub reflection_idle_secs: u64,
    /// Number of accumulated user corrections before prompt self-update triggers (requires LLM).
    pub self_update_corrections: usize,
}

impl Default for AutonomousThresholds {
    fn default() -> Self {
        Self {
            consolidate_count: 5,
            deprecate_check: true,
            embed_pending: true,
            skill_evolve_calls: 100,
            reflection_idle_secs: 3600,
            self_update_corrections: 3,
        }
    }
}

/// Rate limits for autonomous LLM calls to prevent runaway costs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(default)]
pub struct AutonomousLimits {
    /// Maximum autonomous LLM calls per hour.
    pub max_llm_calls_per_hour: u32,
    /// Maximum concurrent autonomous Turns.
    pub max_concurrent: u32,
    /// Cooldown in seconds after an autonomous LLM call before the next one.
    pub cooldown_after_llm_secs: u64,
}

impl Default for AutonomousLimits {
    fn default() -> Self {
        Self {
            max_llm_calls_per_hour: 10,
            max_concurrent: 1,
            cooldown_after_llm_secs: 300,
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_toml_uses_defaults() {
        let config: CortexConfig = toml::from_str("").unwrap();
        assert_eq!(config.api.provider, "anthropic");
        assert!(config.api.model.is_empty());
        assert_eq!(
            config.context.pressure_thresholds,
            vec![0.60, 0.75, 0.85, 0.95]
        );
        assert_eq!(config.memory.max_recall, 10);
        assert!((config.memory.consolidation_similarity_threshold - 0.85).abs() < f64::EPSILON);
        assert!((config.memory.semantic_upgrade_similarity_threshold - 0.90).abs() < f64::EPSILON);
        assert_eq!(config.metacognition.doom_loop_threshold, 3);
        assert_eq!(config.ui.prompt_symbol, "cortex> ");
        assert_eq!(config.embedding.provider, "ollama");
    }

    #[test]
    fn partial_override() {
        let toml_str = r#"
[api]
model = "gpt-5.4"

[memory]
max_recall = 20
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.api.model, "gpt-5.4");
        assert_eq!(config.api.provider, "anthropic"); // default preserved
        assert_eq!(config.memory.max_recall, 20);
        assert!((config.memory.decay_rate - 0.05).abs() < f64::EPSILON); // default preserved
        assert_eq!(
            config.turn.llm_transient_retries,
            DEFAULT_LLM_TRANSIENT_RETRIES
        );
    }

    #[test]
    fn turn_llm_transient_retries_is_configurable() {
        let config: CortexConfig = toml::from_str(
            r"
[turn]
llm_transient_retries = 0
",
        )
        .unwrap();

        assert_eq!(config.turn.llm_transient_retries, 0);
    }

    #[test]
    fn risk_tool_policy_is_configurable() {
        let config: CortexConfig = toml::from_str(
            r#"
[risk.tools.word_count]
tool_risk = 0.1
blast_radius = 0.0
irreversibility = 0.0
allow_background = true

[risk]
allow = ["word_*", "deploy"]
deny = ["blocked_*"]
auto_approve_up_to = "Review"
confirmation_timeout_secs = 30

[risk.tools.deploy]
require_confirmation = true
block = false
"#,
        )
        .unwrap();

        let word_count = config.risk.tools.get("word_count").unwrap();
        assert_eq!(word_count.tool_risk, Some(0.1));
        assert_eq!(word_count.blast_radius, Some(0.0));
        assert!(word_count.allow_background);

        let deploy = config.risk.tools.get("deploy").unwrap();
        assert!(deploy.require_confirmation);
        assert!(!deploy.block);
        assert_eq!(config.risk.allow, vec!["word_*", "deploy"]);
        assert_eq!(config.risk.deny, vec!["blocked_*"]);
        assert_eq!(config.risk.auto_approve_up_to, RiskLevel::Review);
        assert_eq!(config.risk.confirmation_timeout_secs, 30);
    }

    fn test_providers() -> ProviderRegistry {
        let mut m = ProviderRegistry::new();
        m.insert(
            "anthropic".into(),
            ProviderConfig {
                name: "Anthropic".into(),
                protocol: ProviderProtocol::Anthropic,
                base_url: "https://api.anthropic.com".into(),
                auth_type: AuthType::XApiKey,
                models: vec![],
                vision_provider: String::new(),
                vision_model: String::new(),
                image_input_mode: OpenAiImageInputMode::default(),
                files_base_url: String::new(),
                openai_stream_options: false,
                vision_max_output_tokens: 0,
                capability_cache_ttl_hours: 0,
            },
        );
        m.insert(
            "ollama".into(),
            ProviderConfig {
                name: "Ollama".into(),
                protocol: ProviderProtocol::Ollama,
                base_url: "http://localhost:11434".into(),
                auth_type: AuthType::None,
                models: vec![],
                vision_provider: String::new(),
                vision_model: String::new(),
                image_input_mode: OpenAiImageInputMode::default(),
                files_base_url: String::new(),
                openai_stream_options: false,
                vision_max_output_tokens: 0,
                capability_cache_ttl_hours: 0,
            },
        );
        m
    }

    #[test]
    fn resolve_primary_endpoint() {
        let api = ApiConfig {
            provider: "anthropic".into(),
            api_key: "key123".into(),
            model: "claude-sonnet-4-6".into(),
            max_tokens: 8192,
            ..Default::default()
        };
        let ep = ResolvedEndpoint::resolve_primary(&api, &test_providers()).unwrap();
        assert_eq!(ep.base_url, "https://api.anthropic.com");
        assert_eq!(ep.protocol, ProviderProtocol::Anthropic);
        assert_eq!(ep.api_key, "key123");
        assert_eq!(ep.max_tokens, 8192);
    }

    #[test]
    fn resolve_sub_inherits_empty() {
        let api = ApiConfig {
            provider: "anthropic".into(),
            api_key: "parent-key".into(),
            model: "claude-sonnet-4-6".into(),
            ..Default::default()
        };
        let sub = ApiEndpointConfig::default();
        let ep = ResolvedEndpoint::resolve(&sub, &api, &test_providers()).unwrap();
        assert_eq!(ep.api_key, "parent-key");
        assert_eq!(ep.model, "claude-sonnet-4-6");
    }

    #[test]
    fn resolve_sub_overrides_parent() {
        let api = ApiConfig {
            provider: "anthropic".into(),
            api_key: "parent-key".into(),
            model: "claude-sonnet-4-6".into(),
            ..Default::default()
        };
        let sub = ApiEndpointConfig {
            provider: "ollama".into(),
            model: "llama3".into(),
            max_tokens: 2048,
            ..Default::default()
        };
        let ep = ResolvedEndpoint::resolve(&sub, &api, &test_providers()).unwrap();
        assert_eq!(ep.protocol, ProviderProtocol::Ollama);
        assert_eq!(ep.model, "llama3");
        assert_eq!(ep.api_key, "parent-key"); // empty sub inherits
        assert_eq!(ep.max_tokens, 2048);
    }

    #[test]
    fn resolve_vision_uses_declared_provider() {
        let mut providers = test_providers();
        providers.insert(
            "anthropic-vision".into(),
            ProviderConfig {
                name: "Anthropic Vision".into(),
                protocol: ProviderProtocol::OpenAI,
                base_url: "https://vision.example.com/v1".into(),
                auth_type: AuthType::Bearer,
                models: vec![],
                vision_provider: String::new(),
                vision_model: "vision-model".into(),
                image_input_mode: OpenAiImageInputMode::DataUrl,
                files_base_url: String::new(),
                openai_stream_options: false,
                vision_max_output_tokens: 4096,
                capability_cache_ttl_hours: 12,
            },
        );
        providers.get_mut("anthropic").unwrap().vision_provider = "anthropic-vision".into();
        let api = ApiConfig {
            provider: "anthropic".into(),
            api_key: "key".into(),
            model: "text-model".into(),
            ..Default::default()
        };

        let ep = ResolvedEndpoint::resolve_vision_endpoint(&api, &providers)
            .unwrap()
            .unwrap();

        assert_eq!(ep.provider, "anthropic-vision");
        assert_eq!(ep.protocol, ProviderProtocol::OpenAI);
        assert_eq!(ep.model, "vision-model");
        assert_eq!(ep.vision_max_output_tokens, 4096);
        assert_eq!(ep.capability_cache_ttl_hours, 12);
    }

    #[test]
    fn resolve_unknown_provider_errors() {
        let api = ApiConfig {
            provider: "nonexistent".into(),
            ..Default::default()
        };
        assert!(ResolvedEndpoint::resolve_primary(&api, &test_providers()).is_err());
    }

    #[test]
    fn mcp_config_empty_defaults() {
        let config: CortexConfig = toml::from_str("").unwrap();
        assert!(config.mcp.servers.is_empty());
    }

    #[test]
    fn mcp_config_stdio_server() {
        let toml_str = r#"
[[mcp.servers]]
name = "fs"
transport = "stdio"
command = "npx"
args = ["-y", "@mcp/server-fs"]
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.servers.len(), 1);
        assert_eq!(config.mcp.servers[0].name, "fs");
        assert_eq!(config.mcp.servers[0].transport, McpTransportType::Stdio);
        assert_eq!(config.mcp.servers[0].command, "npx");
        assert_eq!(config.mcp.servers[0].args, vec!["-y", "@mcp/server-fs"]);
    }

    #[test]
    fn mcp_config_sse_server() {
        let toml_str = r#"
[[mcp.servers]]
name = "remote"
transport = "sse"
url = "https://api.example.com/mcp"

[mcp.servers.headers]
Authorization = "Bearer token123"
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.mcp.servers.len(), 1);
        assert_eq!(config.mcp.servers[0].name, "remote");
        assert_eq!(config.mcp.servers[0].transport, McpTransportType::Sse);
        assert_eq!(config.mcp.servers[0].url, "https://api.example.com/mcp");
        assert_eq!(
            config.mcp.servers[0].headers.get("Authorization").unwrap(),
            "Bearer token123"
        );
    }

    #[test]
    fn trace_level_ordering() {
        assert!(TraceLevel::Off < TraceLevel::Minimal);
        assert!(TraceLevel::Minimal < TraceLevel::Basic);
        assert!(TraceLevel::Basic < TraceLevel::Summary);
        assert!(TraceLevel::Summary < TraceLevel::Full);
        assert!(TraceLevel::Full < TraceLevel::Debug);
    }

    #[test]
    fn trace_config_default_level() {
        let tc = TurnTraceConfig::default();
        assert_eq!(tc.level, TraceLevel::Off);
        assert!(!tc.phase()); // Off (global) < Minimal
        assert!(!tc.llm()); // Off (global) < Minimal
        assert!(tc.tool()); // Minimal >= Minimal
        assert!(!tc.meta()); // Off (global) < Minimal
        assert!(!tc.memory()); // Off (global) < Minimal
        assert!(!tc.context()); // Off (global) < Minimal
    }

    #[test]
    fn trace_config_per_category_override() {
        let tc = TurnTraceConfig {
            level: TraceLevel::Off,
            meta: Some(TraceLevel::Off),
            llm: Some(TraceLevel::Full),
            ..Default::default()
        };
        assert_eq!(tc.level_for("meta"), TraceLevel::Off);
        assert_eq!(tc.level_for("llm"), TraceLevel::Full);
        assert_eq!(tc.level_for("phase"), TraceLevel::Off); // global Off
        assert!(!tc.meta()); // Off < Minimal
        assert!(tc.is_enabled_at("llm", TraceLevel::Full));
        assert!(!tc.is_enabled_at("meta", TraceLevel::Minimal));
    }

    #[test]
    fn trace_config_toml_roundtrip() {
        let toml_str = r#"
[turn.trace]
level = "full"
llm = "debug"
meta = "off"
"#;
        let config: CortexConfig = toml::from_str(toml_str).unwrap();
        assert_eq!(config.turn.trace.level, TraceLevel::Full);
        assert_eq!(config.turn.trace.llm, Some(TraceLevel::Debug));
        assert_eq!(config.turn.trace.meta, Some(TraceLevel::Off));
        assert!(config.turn.trace.phase.is_none());
    }
}
