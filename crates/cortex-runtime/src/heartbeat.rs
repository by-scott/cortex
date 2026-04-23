//! Heartbeat-driven idle cognition engine.
//!
//! Replaces the fixed 30-minute maintenance cycle with a lightweight
//! tick-based evaluation system. Most ticks are zero-cost. Actions
//! only fire when accumulated state exceeds configured thresholds.

use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Instant;

use cortex_types::config::{AutonomousConfig, AutonomousLimits, AutonomousThresholds};

/// Actions the heartbeat can request, ordered by salience (urgency).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeartbeatAction {
    /// Deprecate expired memories (no LLM).
    DeprecateExpired,
    /// Generate embeddings for un-embedded memories (embedding API, no LLM).
    EmbedPending,
    /// Consolidate accumulated memories (no LLM).
    ConsolidateMemories,
    /// Trigger Skill evolution from tool patterns (no LLM).
    EvolveSkills,
    /// Trigger prompt self-update (requires LLM).
    SelfUpdate,
    /// Trigger deep reflection (requires LLM).
    DeepReflection,
    /// Execute a due cron task (requires LLM).
    CronDue(String),
    /// Create journal checkpoint (no LLM).
    Checkpoint,
}

impl HeartbeatAction {
    /// Whether this action requires an LLM call.
    #[must_use]
    pub const fn requires_llm(&self) -> bool {
        matches!(
            self,
            Self::SelfUpdate | Self::DeepReflection | Self::CronDue(_)
        )
    }

    /// Salience score (higher = more urgent). Used for priority sorting.
    #[must_use]
    pub const fn salience(&self) -> u32 {
        match self {
            Self::DeprecateExpired => 10,
            Self::EmbedPending => 20,
            Self::ConsolidateMemories => 30,
            Self::EvolveSkills => 40,
            Self::Checkpoint => 50,
            Self::CronDue(_) => 60,
            Self::SelfUpdate => 70,
            Self::DeepReflection => 80,
        }
    }
}

/// Accumulated state counters evaluated each heartbeat tick.
pub struct HeartbeatState {
    /// Number of memories pending consolidation.
    pub pending_consolidation: AtomicU32,
    /// Number of memories without embeddings.
    pub pending_embeddings: AtomicU32,
    /// Number of user corrections since last self-update.
    pub correction_count: AtomicU32,
    /// Total tool calls since last Skill evolution check.
    pub tool_calls_since_evolve: AtomicU32,
    /// Whether a Turn is currently executing (foreground busy).
    pub foreground_busy: AtomicBool,
    /// Timestamp of last user interaction (seconds since engine start).
    pub last_interaction_secs: AtomicU64,
    /// Autonomous LLM calls this hour.
    pub llm_calls_this_hour: AtomicU32,
    /// Timestamp of last autonomous LLM call (seconds since engine start).
    pub last_llm_call_secs: AtomicU64,
    /// Engine start instant (for computing elapsed seconds).
    start: Instant,
    /// Optional cron queue for checking due tasks.
    pub cron_queue: Option<std::sync::Arc<cortex_turn::tools::cron::CronQueue>>,
}

impl HeartbeatState {
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending_consolidation: AtomicU32::new(0),
            pending_embeddings: AtomicU32::new(0),
            correction_count: AtomicU32::new(0),
            tool_calls_since_evolve: AtomicU32::new(0),
            foreground_busy: AtomicBool::new(false),
            last_interaction_secs: AtomicU64::new(0),
            llm_calls_this_hour: AtomicU32::new(0),
            last_llm_call_secs: AtomicU64::new(0),
            start: Instant::now(),
            cron_queue: None,
        }
    }

    /// Elapsed seconds since engine start.
    #[must_use]
    pub fn elapsed_secs(&self) -> u64 {
        self.start.elapsed().as_secs()
    }

    /// Record a user interaction (resets idle timer).
    pub fn touch(&self) {
        self.last_interaction_secs
            .store(self.elapsed_secs(), Ordering::Relaxed);
    }

    /// Seconds since last user interaction.
    #[must_use]
    pub fn idle_secs(&self) -> u64 {
        self.elapsed_secs() - self.last_interaction_secs.load(Ordering::Relaxed)
    }

    /// Record a user correction.
    pub fn record_correction(&self) {
        self.correction_count.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a tool call.
    pub fn record_tool_call(&self) {
        self.tool_calls_since_evolve.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an autonomous LLM call (for rate limiting).
    pub fn record_llm_call(&self) {
        self.llm_calls_this_hour.fetch_add(1, Ordering::Relaxed);
        self.last_llm_call_secs
            .store(self.elapsed_secs(), Ordering::Relaxed);
    }

    /// Reset the hourly LLM call counter (called by the hourly reset logic).
    pub fn reset_hourly_counter(&self) {
        self.llm_calls_this_hour.store(0, Ordering::Relaxed);
    }
}

impl Default for HeartbeatState {
    fn default() -> Self {
        Self::new()
    }
}

/// The heartbeat engine. Evaluates accumulated state against thresholds
/// each tick and returns a prioritized list of actions to execute.
pub struct HeartbeatEngine {
    thresholds: AutonomousThresholds,
    limits: AutonomousLimits,
    /// Ticks since last checkpoint.
    ticks_since_checkpoint: u32,
}

impl HeartbeatEngine {
    #[must_use]
    pub const fn new(config: &AutonomousConfig) -> Self {
        Self {
            thresholds: config.thresholds,
            limits: config.limits,
            ticks_since_checkpoint: 0,
        }
    }

    /// Evaluate accumulated state and return actions to execute.
    ///
    /// Returns an empty vec if:
    /// - Foreground is busy (Turn in progress)
    /// - No thresholds are exceeded
    ///
    /// Actions are sorted by salience (most urgent first).
    pub fn tick(&mut self, state: &HeartbeatState) -> Vec<HeartbeatAction> {
        // Never interrupt foreground work
        if state.foreground_busy.load(Ordering::Relaxed) {
            return Vec::new();
        }

        let mut actions = Vec::new();

        // --- No-LLM actions (always eligible) ---

        if self.thresholds.deprecate_check {
            // Always check — the actual deprecation is cheap
            actions.push(HeartbeatAction::DeprecateExpired);
        }

        if self.thresholds.embed_pending && state.pending_embeddings.load(Ordering::Relaxed) > 0 {
            actions.push(HeartbeatAction::EmbedPending);
        }

        let pending = state.pending_consolidation.load(Ordering::Relaxed);
        if pending >= u32::try_from(self.thresholds.consolidate_count).unwrap_or(u32::MAX) {
            actions.push(HeartbeatAction::ConsolidateMemories);
        }

        let tool_calls = state.tool_calls_since_evolve.load(Ordering::Relaxed);
        if tool_calls >= u32::try_from(self.thresholds.skill_evolve_calls).unwrap_or(u32::MAX) {
            actions.push(HeartbeatAction::EvolveSkills);
        }

        // Periodic checkpoint (~every 180 ticks at 10s interval = ~30 min)
        self.ticks_since_checkpoint += 1;
        if self.ticks_since_checkpoint >= 180 {
            actions.push(HeartbeatAction::Checkpoint);
            self.ticks_since_checkpoint = 0;
        }

        // --- LLM actions (rate-limited) ---

        if self.can_call_llm(state) {
            let corrections = state.correction_count.load(Ordering::Relaxed);
            if corrections
                >= u32::try_from(self.thresholds.self_update_corrections).unwrap_or(u32::MAX)
            {
                actions.push(HeartbeatAction::SelfUpdate);
            }

            let idle = state.idle_secs();
            if idle >= self.thresholds.reflection_idle_secs && idle > 0 {
                actions.push(HeartbeatAction::DeepReflection);
            }

            // Cron tasks: collect all due prompts
            if let Some(ref queue) = state.cron_queue {
                for prompt in queue.collect_due() {
                    actions.push(HeartbeatAction::CronDue(prompt));
                }
            }
        }

        // Sort by salience (highest first) — most urgent action executed first
        actions.sort_by_key(|a| std::cmp::Reverse(a.salience()));
        actions
    }

    /// Check if an autonomous LLM call is allowed by rate limits.
    fn can_call_llm(&self, state: &HeartbeatState) -> bool {
        let calls = state.llm_calls_this_hour.load(Ordering::Relaxed);
        if calls >= self.limits.max_llm_calls_per_hour {
            return false;
        }
        // Never called → always allow
        let last = state.last_llm_call_secs.load(Ordering::Relaxed);
        if calls == 0 && last == 0 {
            return true;
        }
        let since_last = state.elapsed_secs().saturating_sub(last);
        since_last >= self.limits.cooldown_after_llm_secs
    }
}
