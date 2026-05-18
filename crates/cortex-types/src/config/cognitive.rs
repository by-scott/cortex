use serde::{Deserialize, Serialize};

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

/// Default health check interval in turns.
const DEFAULT_HEALTH_CHECK_INTERVAL_TURNS: usize = 10;

/// Default health degraded threshold.
const DEFAULT_HEALTH_DEGRADED_THRESHOLD: f64 = 0.3;

/// Default health dimension weight.
const DEFAULT_HEALTH_WEIGHT: f64 = 0.25;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ContextConfig {
    pub max_tokens: usize,
    pub pressure_thresholds: Vec<f64>,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            max_tokens: super::DEFAULT_CONTEXT_MAX_TOKENS,
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
