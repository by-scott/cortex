use serde::{Deserialize, Serialize};

/// Configuration for Cortex's autonomous behavior - the heartbeat-driven idle
/// cognition system. When `enabled = false`, Cortex is purely passive and only
/// responds to user input.
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
