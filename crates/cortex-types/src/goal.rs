use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalLevel {
    Strategic,
    Tactical,
    Immediate,
}

impl fmt::Display for GoalLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    Proposed,
    Active,
    Blocked,
    Completed,
    Abandoned,
}

impl GoalStatus {
    /// # Errors
    ///
    /// Returns `GoalTransitionError` if the transition is invalid.
    pub const fn try_transition(self, to: Self) -> Result<Self, GoalTransitionError> {
        let valid = matches!(
            (self, to),
            (Self::Proposed, Self::Active | Self::Abandoned)
                | (
                    Self::Active,
                    Self::Blocked | Self::Completed | Self::Abandoned
                )
                | (
                    Self::Blocked,
                    Self::Active | Self::Completed | Self::Abandoned
                )
        );
        if valid
            || matches!(
                (self, to),
                (Self::Active, Self::Active) | (Self::Blocked, Self::Blocked)
            )
        {
            Ok(to)
        } else {
            Err(GoalTransitionError { from: self, to })
        }
    }

    #[must_use]
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Proposed | Self::Active | Self::Blocked)
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Abandoned)
    }
}

impl fmt::Display for GoalStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone)]
pub struct GoalTransitionError {
    pub from: GoalStatus,
    pub to: GoalStatus,
}

impl fmt::Display for GoalTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid goal transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for GoalTransitionError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalSource {
    User,
    Operator,
    Runtime,
    Memory,
    Imported,
}

impl fmt::Display for GoalSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    #[serde(default = "default_owner_actor")]
    pub owner_actor: String,
    pub parent_goal_id: Option<String>,
    pub linked_task_id: Option<String>,
    pub level: GoalLevel,
    pub description: String,
    pub success_criteria: String,
    pub source: GoalSource,
    pub status: GoalStatus,
    pub priority: u8,
    #[serde(default)]
    pub evidence_refs: Vec<String>,
    #[serde(default)]
    pub memory_refs: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

fn default_owner_actor() -> String {
    "local:default".into()
}

const DEFAULT_PRIORITY: u8 = 5;

impl Goal {
    #[must_use]
    pub fn new(description: impl Into<String>, level: GoalLevel) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            owner_actor: default_owner_actor(),
            parent_goal_id: None,
            linked_task_id: None,
            level,
            description: description.into(),
            success_criteria: String::new(),
            source: GoalSource::User,
            status: GoalStatus::Active,
            priority: DEFAULT_PRIORITY,
            evidence_refs: Vec::new(),
            memory_refs: Vec::new(),
            created_at: now,
            updated_at: now,
            deadline: None,
            completed_at: None,
        }
    }

    #[must_use]
    pub fn context_line(&self) -> String {
        let mut line = format!("[{}] {}", self.level, self.description);
        if !self.success_criteria.is_empty() {
            line.push_str(" | success: ");
            line.push_str(&self.success_criteria);
        }
        if let Some(deadline) = self.deadline {
            line.push_str(" | due: ");
            line.push_str(&deadline.to_rfc3339());
        }
        if let Some(task_id) = &self.linked_task_id {
            line.push_str(" | task: ");
            line.push_str(task_id);
        }
        line
    }
}
