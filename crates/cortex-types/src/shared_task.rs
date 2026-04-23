use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SharedTaskStatus {
    Pending,
    Assigned,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AggregationStrategy {
    Concatenate,
    BestResult,
    Summarize,
}

#[derive(Debug, Clone)]
pub struct SharedTaskTransitionError {
    pub from: SharedTaskStatus,
    pub to: SharedTaskStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskAssignment {
    pub task_id: String,
    pub target_instance: String,
    pub assigned_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedTask {
    pub id: String,
    #[serde(default = "default_owner_actor")]
    pub owner_actor: String,
    pub parent_task_id: Option<String>,
    pub description: String,
    pub status: SharedTaskStatus,
    pub assigned_instance: Option<String>,
    pub priority: u8,
    pub result: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub deadline: Option<DateTime<Utc>>,
}

fn default_owner_actor() -> String {
    "local:default".into()
}

impl SharedTaskStatus {
    /// # Errors
    /// Returns `SharedTaskTransitionError` if the transition is invalid.
    pub const fn try_transition(self, to: Self) -> Result<Self, SharedTaskTransitionError> {
        let valid = matches!(
            (self, to),
            (Self::Pending, Self::Assigned)
                | (Self::Assigned, Self::InProgress)
                | (Self::InProgress, Self::Completed | Self::Failed)
                | (_, Self::Cancelled)
        );
        if valid {
            Ok(to)
        } else {
            Err(SharedTaskTransitionError { from: self, to })
        }
    }

    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for SharedTaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Display for SharedTaskTransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid task transition: {:?} -> {:?}",
            self.from, self.to
        )
    }
}

impl std::error::Error for SharedTaskTransitionError {}

impl fmt::Display for AggregationStrategy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl TaskAssignment {
    #[must_use]
    pub fn new(task_id: impl Into<String>, target_instance: impl Into<String>) -> Self {
        Self {
            task_id: task_id.into(),
            target_instance: target_instance.into(),
            assigned_at: Utc::now(),
            deadline: None,
        }
    }

    #[must_use]
    pub const fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

const DEFAULT_PRIORITY: u8 = 5;

impl SharedTask {
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            owner_actor: default_owner_actor(),
            parent_task_id: None,
            description: description.into(),
            status: SharedTaskStatus::Pending,
            assigned_instance: None,
            priority: DEFAULT_PRIORITY,
            result: None,
            created_at: now,
            updated_at: now,
            deadline: None,
        }
    }

    #[must_use]
    pub fn with_parent(mut self, parent_id: impl Into<String>) -> Self {
        self.parent_task_id = Some(parent_id.into());
        self
    }

    #[must_use]
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    #[must_use]
    pub const fn with_deadline(mut self, deadline: DateTime<Utc>) -> Self {
        self.deadline = Some(deadline);
        self
    }
}
