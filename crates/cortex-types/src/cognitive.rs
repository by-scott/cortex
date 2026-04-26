use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AuthContext, ControlSignal, OwnedScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlLevel {
    Sensorimotor,
    Contextual,
    Episodic,
    Strategic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalStatus {
    Active,
    Suspended,
    Completed,
    Blocked,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ControlGoal {
    pub id: String,
    pub scope: OwnedScope,
    pub level: ControlLevel,
    pub parent_id: Option<String>,
    pub status: GoalStatus,
    pub statement: String,
    pub tags: BTreeSet<String>,
    pub inhibits: BTreeSet<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalGraphError {
    DuplicateGoal,
    InvalidControlLevel,
    MissingGoal,
    MissingParent,
    NotVisible,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalConflict {
    pub left_id: String,
    pub right_id: String,
    pub level: ControlLevel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalGraph {
    pub goals: Vec<ControlGoal>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoadClass {
    Intrinsic,
    Extraneous,
    Germane,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextLoadItem {
    pub id: String,
    pub class: LoadClass,
    pub tokens: u32,
    pub element_interactivity: f64,
    pub relevance: f64,
    pub age_turns: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoadWeights {
    pub token_saturation: f64,
    pub intrinsic: f64,
    pub extraneous: f64,
    pub germane_credit: f64,
    pub temporal_decay: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LoadProfile {
    pub token_saturation: f64,
    pub intrinsic: f64,
    pub extraneous: f64,
    pub germane: f64,
    pub temporal_decay: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PressureAction {
    Continue,
    CompactExtraneous,
    SummarizeCompleted,
    ReanchorGoal,
    SplitTask,
    AskHuman,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConflictKind {
    GoalConflict,
    LoadPressure,
    FeedbackConflict,
    FrameAnchoring,
    CalibrationDrift,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConflictSignal {
    pub kind: ConflictKind,
    pub intensity: f64,
    pub evidence: String,
    pub control: ControlSignal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecutionTrace {
    pub recent_actions: Vec<String>,
    pub tool_failures: u32,
    pub user_corrections: u32,
    pub contradictions: u32,
    pub progress_delta: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MonitoringThresholds {
    pub high_pressure: f64,
    pub severe_pressure: f64,
    pub frame_repeat_ratio: f64,
    pub low_progress_delta: f64,
    pub minimum_actions_for_frame_check: usize,
    pub correction_limit: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoringReport {
    pub pressure: f64,
    pub pressure_action: PressureAction,
    pub signals: Vec<ConflictSignal>,
    pub recommended_control: ControlSignal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitoringRecord {
    pub id: String,
    pub scope: OwnedScope,
    pub report: MonitoringReport,
    pub recorded_at: DateTime<Utc>,
}

impl ControlLevel {
    #[must_use]
    pub const fn abstraction_rank(self) -> u8 {
        match self {
            Self::Sensorimotor => 0,
            Self::Contextual => 1,
            Self::Episodic => 2,
            Self::Strategic => 3,
        }
    }

    #[must_use]
    pub const fn can_control(self, child: Self) -> bool {
        self.abstraction_rank() > child.abstraction_rank()
    }

    #[must_use]
    pub const fn bias_weight(self) -> f64 {
        match self {
            Self::Sensorimotor => 1.0,
            Self::Contextual => 1.5,
            Self::Episodic => 2.0,
            Self::Strategic => 3.0,
        }
    }
}

impl ControlGoal {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: OwnedScope,
        level: ControlLevel,
        statement: impl Into<String>,
    ) -> Self {
        Self::new_at(id, scope, level, statement, Utc::now())
    }

    #[must_use]
    pub fn new_at(
        id: impl Into<String>,
        scope: OwnedScope,
        level: ControlLevel,
        statement: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            level,
            parent_id: None,
            status: GoalStatus::Active,
            statement: statement.into(),
            tags: BTreeSet::new(),
            inhibits: BTreeSet::new(),
            created_at: now,
            updated_at: now,
        }
    }

    #[must_use]
    pub fn under(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_id = Some(parent_id.into());
        self
    }

    #[must_use]
    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        self.tags.insert(tag.into());
        self
    }

    #[must_use]
    pub fn inhibits(mut self, goal_id: impl Into<String>) -> Self {
        self.inhibits.insert(goal_id.into());
        self
    }

    pub const fn transition(&mut self, status: GoalStatus, now: DateTime<Utc>) {
        self.status = status;
        self.updated_at = now;
    }
}

impl GoalGraph {
    #[must_use]
    pub const fn new() -> Self {
        Self { goals: Vec::new() }
    }

    /// # Errors
    /// Returns an error when the goal is not visible to the actor, duplicates
    /// an existing goal, references a missing parent, or violates the control
    /// hierarchy.
    pub fn insert(
        &mut self,
        context: &AuthContext,
        goal: ControlGoal,
    ) -> Result<(), GoalGraphError> {
        if !goal.scope.is_visible_to(context) {
            return Err(GoalGraphError::NotVisible);
        }
        if self.goals.iter().any(|existing| existing.id == goal.id) {
            return Err(GoalGraphError::DuplicateGoal);
        }
        if let Some(parent_id) = &goal.parent_id {
            let Some(parent) = self
                .goals
                .iter()
                .find(|candidate| &candidate.id == parent_id)
            else {
                return Err(GoalGraphError::MissingParent);
            };
            if !parent.scope.is_visible_to(context) {
                return Err(GoalGraphError::NotVisible);
            }
            if !parent.level.can_control(goal.level) {
                return Err(GoalGraphError::InvalidControlLevel);
            }
        }
        self.goals.push(goal);
        Ok(())
    }

    /// # Errors
    /// Returns an error when the goal does not exist or is not visible to the
    /// actor.
    pub fn transition(
        &mut self,
        context: &AuthContext,
        goal_id: &str,
        status: GoalStatus,
        now: DateTime<Utc>,
    ) -> Result<(), GoalGraphError> {
        let Some(goal) = self
            .goals
            .iter_mut()
            .find(|candidate| candidate.id == goal_id)
        else {
            return Err(GoalGraphError::MissingGoal);
        };
        if !goal.scope.is_visible_to(context) {
            return Err(GoalGraphError::NotVisible);
        }
        goal.transition(status, now);
        Ok(())
    }

    #[must_use]
    pub fn active_by_level(&self, context: &AuthContext, level: ControlLevel) -> Vec<&ControlGoal> {
        self.goals
            .iter()
            .filter(|goal| {
                goal.level == level
                    && goal.status == GoalStatus::Active
                    && goal.scope.is_visible_to(context)
            })
            .collect()
    }

    #[must_use]
    pub fn children_of(&self, context: &AuthContext, parent_id: &str) -> Vec<&ControlGoal> {
        self.goals
            .iter()
            .filter(|goal| {
                goal.parent_id.as_deref() == Some(parent_id) && goal.scope.is_visible_to(context)
            })
            .collect()
    }

    #[must_use]
    pub fn conflicts(&self, context: &AuthContext) -> Vec<GoalConflict> {
        let active = self
            .goals
            .iter()
            .filter(|goal| goal.status == GoalStatus::Active && goal.scope.is_visible_to(context))
            .collect::<Vec<_>>();
        let mut conflicts = Vec::new();
        for (left_index, left) in active.iter().enumerate() {
            for right in active.iter().skip(left_index + 1) {
                if left.inhibits.contains(&right.id) || right.inhibits.contains(&left.id) {
                    conflicts.push(GoalConflict {
                        left_id: left.id.clone(),
                        right_id: right.id.clone(),
                        level: left.level.max(right.level),
                    });
                }
            }
        }
        conflicts
    }

    #[must_use]
    pub fn top_down_bias(&self, context: &AuthContext, candidate_tags: &BTreeSet<String>) -> f64 {
        if candidate_tags.is_empty() {
            return 0.0;
        }
        let mut total = 0.0_f64;
        let mut matched = 0.0_f64;
        for goal in self
            .goals
            .iter()
            .filter(|goal| goal.status == GoalStatus::Active && goal.scope.is_visible_to(context))
        {
            let weight = goal.level.bias_weight();
            total += weight;
            if goal.tags.iter().any(|tag| candidate_tags.contains(tag)) {
                matched += weight;
            }
        }
        if total <= f64::EPSILON {
            0.0
        } else {
            (matched / total).clamp(0.0, 1.0)
        }
    }
}

impl ContextLoadItem {
    #[must_use]
    pub fn new(id: impl Into<String>, class: LoadClass, tokens: u32) -> Self {
        Self {
            id: id.into(),
            class,
            tokens,
            element_interactivity: 1.0,
            relevance: 1.0,
            age_turns: 0,
        }
    }

    #[must_use]
    pub const fn with_interactivity(mut self, element_interactivity: f64) -> Self {
        self.element_interactivity = element_interactivity.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_relevance(mut self, relevance: f64) -> Self {
        self.relevance = relevance.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn aged(mut self, age_turns: u32) -> Self {
        self.age_turns = age_turns;
        self
    }

    #[must_use]
    pub fn weighted_units(&self) -> f64 {
        f64::from(self.tokens) * self.element_interactivity * self.relevance
    }

    #[must_use]
    pub fn temporal_staleness(&self) -> f64 {
        f64::from(self.age_turns) / (f64::from(self.age_turns) + 10.0)
    }
}

impl Default for LoadWeights {
    fn default() -> Self {
        Self {
            token_saturation: 0.35,
            intrinsic: 0.25,
            extraneous: 0.25,
            germane_credit: 0.10,
            temporal_decay: 0.15,
        }
    }
}

impl LoadProfile {
    #[must_use]
    pub fn measure(items: &[ContextLoadItem], max_tokens: u32) -> Self {
        if max_tokens == 0 {
            return Self {
                token_saturation: 1.0,
                intrinsic: 1.0,
                extraneous: 1.0,
                germane: 0.0,
                temporal_decay: 1.0,
            };
        }
        let max_tokens = f64::from(max_tokens);
        let mut total_tokens = 0.0_f64;
        let mut intrinsic = 0.0_f64;
        let mut extraneous = 0.0_f64;
        let mut germane = 0.0_f64;
        let mut temporal_decay = 0.0_f64;
        for item in items {
            total_tokens += f64::from(item.tokens);
            let units = item.weighted_units();
            match item.class {
                LoadClass::Intrinsic => intrinsic += units,
                LoadClass::Extraneous => extraneous += units,
                LoadClass::Germane => germane += units,
            }
            temporal_decay += f64::from(item.tokens) * item.temporal_staleness();
        }
        Self {
            token_saturation: (total_tokens / max_tokens).clamp(0.0, 1.0),
            intrinsic: (intrinsic / max_tokens).clamp(0.0, 1.0),
            extraneous: (extraneous / max_tokens).clamp(0.0, 1.0),
            germane: (germane / max_tokens).clamp(0.0, 1.0),
            temporal_decay: (temporal_decay / max_tokens).clamp(0.0, 1.0),
        }
    }

    #[must_use]
    pub fn pressure(self, weights: LoadWeights) -> f64 {
        let raw = self.germane.mul_add(
            -weights.germane_credit,
            self.temporal_decay.mul_add(
                weights.temporal_decay,
                self.extraneous.mul_add(
                    weights.extraneous,
                    self.intrinsic.mul_add(
                        weights.intrinsic,
                        self.token_saturation * weights.token_saturation,
                    ),
                ),
            ),
        );
        raw.clamp(0.0, 1.0)
    }

    #[must_use]
    pub fn recommended_action(self, thresholds: MonitoringThresholds) -> PressureAction {
        let pressure = self.pressure(LoadWeights::default());
        if pressure >= thresholds.severe_pressure {
            PressureAction::AskHuman
        } else if self.extraneous >= self.intrinsic && pressure >= thresholds.high_pressure {
            PressureAction::CompactExtraneous
        } else if self.temporal_decay >= thresholds.high_pressure {
            PressureAction::SummarizeCompleted
        } else if self.intrinsic >= thresholds.high_pressure {
            PressureAction::SplitTask
        } else if pressure >= thresholds.high_pressure {
            PressureAction::ReanchorGoal
        } else {
            PressureAction::Continue
        }
    }
}

impl ExecutionTrace {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            recent_actions: Vec::new(),
            tool_failures: 0,
            user_corrections: 0,
            contradictions: 0,
            progress_delta: 1.0,
        }
    }

    #[must_use]
    pub fn with_action(mut self, action: impl Into<String>) -> Self {
        self.recent_actions.push(action.into());
        self
    }

    #[must_use]
    pub const fn with_tool_failures(mut self, tool_failures: u32) -> Self {
        self.tool_failures = tool_failures;
        self
    }

    #[must_use]
    pub const fn with_user_corrections(mut self, user_corrections: u32) -> Self {
        self.user_corrections = user_corrections;
        self
    }

    #[must_use]
    pub const fn with_contradictions(mut self, contradictions: u32) -> Self {
        self.contradictions = contradictions;
        self
    }

    #[must_use]
    pub const fn with_progress_delta(mut self, progress_delta: f64) -> Self {
        self.progress_delta = progress_delta.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub fn dominant_action_ratio(&self) -> f64 {
        if self.recent_actions.is_empty() {
            return 0.0;
        }
        let mut best = 0_u32;
        for action in &self.recent_actions {
            let count = self
                .recent_actions
                .iter()
                .filter(|candidate| *candidate == action)
                .count();
            best = best.max(u32::try_from(count).unwrap_or(u32::MAX));
        }
        f64::from(best) / f64::from(u32::try_from(self.recent_actions.len()).unwrap_or(u32::MAX))
    }
}

impl Default for ExecutionTrace {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for MonitoringThresholds {
    fn default() -> Self {
        Self {
            high_pressure: 0.70,
            severe_pressure: 0.90,
            frame_repeat_ratio: 0.70,
            low_progress_delta: 0.05,
            minimum_actions_for_frame_check: 5,
            correction_limit: 3,
        }
    }
}

impl MonitoringReport {
    #[must_use]
    pub fn evaluate(
        context: &AuthContext,
        goals: &GoalGraph,
        load: LoadProfile,
        trace: &ExecutionTrace,
        thresholds: MonitoringThresholds,
    ) -> Self {
        let pressure = load.pressure(LoadWeights::default());
        let pressure_action = load.recommended_action(thresholds);
        let mut signals = Vec::new();
        for conflict in goals.conflicts(context) {
            signals.push(ConflictSignal {
                kind: ConflictKind::GoalConflict,
                intensity: 1.0,
                evidence: format!("{} inhibits {}", conflict.left_id, conflict.right_id),
                control: ControlSignal::AskHuman,
            });
        }
        if pressure_action != PressureAction::Continue {
            signals.push(ConflictSignal {
                kind: ConflictKind::LoadPressure,
                intensity: pressure,
                evidence: format!("pressure={pressure:.3}"),
                control: pressure_action.to_control_signal(),
            });
        }
        if trace.tool_failures > 0 && trace.progress_delta <= thresholds.low_progress_delta {
            signals.push(ConflictSignal {
                kind: ConflictKind::FeedbackConflict,
                intensity: f64::from(trace.tool_failures)
                    .mul_add(0.2, 0.4)
                    .clamp(0.0, 1.0),
                evidence: format!("tool_failures={}", trace.tool_failures),
                control: ControlSignal::AskHuman,
            });
        }
        if trace.recent_actions.len() >= thresholds.minimum_actions_for_frame_check
            && trace.dominant_action_ratio() >= thresholds.frame_repeat_ratio
            && trace.progress_delta <= thresholds.low_progress_delta
            && (trace.contradictions > 0 || trace.user_corrections > 0)
        {
            signals.push(ConflictSignal {
                kind: ConflictKind::FrameAnchoring,
                intensity: trace.dominant_action_ratio(),
                evidence: format!("repeat_ratio={:.3}", trace.dominant_action_ratio()),
                control: ControlSignal::AskHuman,
            });
        }
        if trace.user_corrections >= thresholds.correction_limit {
            signals.push(ConflictSignal {
                kind: ConflictKind::CalibrationDrift,
                intensity: f64::from(trace.user_corrections)
                    / f64::from(thresholds.correction_limit.max(1)),
                evidence: format!("user_corrections={}", trace.user_corrections),
                control: ControlSignal::AskHuman,
            });
        }
        let recommended_control = signals
            .iter()
            .max_by(|left, right| left.intensity.total_cmp(&right.intensity))
            .map_or(ControlSignal::Continue, |signal| signal.control);
        Self {
            pressure,
            pressure_action,
            signals,
            recommended_control,
        }
    }
}

impl MonitoringRecord {
    #[must_use]
    pub fn new(id: impl Into<String>, scope: OwnedScope, report: MonitoringReport) -> Self {
        Self::new_at(id, scope, report, Utc::now())
    }

    #[must_use]
    pub fn new_at(
        id: impl Into<String>,
        scope: OwnedScope,
        report: MonitoringReport,
        recorded_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            report,
            recorded_at,
        }
    }
}

impl PressureAction {
    #[must_use]
    pub const fn to_control_signal(self) -> ControlSignal {
        match self {
            Self::Continue => ControlSignal::Continue,
            Self::CompactExtraneous | Self::SummarizeCompleted => ControlSignal::ConsolidateMemory,
            Self::ReanchorGoal | Self::SplitTask | Self::AskHuman => ControlSignal::AskHuman,
        }
    }
}
