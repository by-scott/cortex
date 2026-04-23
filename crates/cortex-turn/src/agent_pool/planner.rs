use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PlanStatus {
    Pending,
    InProgress,
    Completed,
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlanStep {
    pub index: usize,
    pub description: String,
    pub tool_hint: Option<String>,
    pub status: PlanStatus,
}

impl PlanStep {
    pub fn new(index: usize, description: impl Into<String>) -> Self {
        Self {
            index,
            description: description.into(),
            tool_hint: None,
            status: PlanStatus::Pending,
        }
    }

    #[must_use]
    pub fn with_tool_hint(mut self, hint: impl Into<String>) -> Self {
        self.tool_hint = Some(hint.into());
        self
    }

    /// Transition this step to `InProgress`.
    ///
    /// # Errors
    /// Returns `PlanStepError::InvalidTransition` if the step is not `Pending`.
    pub fn start(&mut self) -> Result<(), PlanStepError> {
        if self.status != PlanStatus::Pending {
            return Err(PlanStepError::InvalidTransition(format!(
                "cannot start step {}: status is {:?}",
                self.index, self.status
            )));
        }
        self.status = PlanStatus::InProgress;
        Ok(())
    }

    /// Transition this step to `Completed`.
    ///
    /// # Errors
    /// Returns `PlanStepError::InvalidTransition` if the step is not `InProgress`.
    pub fn complete(&mut self) -> Result<(), PlanStepError> {
        if self.status != PlanStatus::InProgress {
            return Err(PlanStepError::InvalidTransition(format!(
                "cannot complete step {}: status is {:?}",
                self.index, self.status
            )));
        }
        self.status = PlanStatus::Completed;
        Ok(())
    }

    /// Transition this step to `Failed` with a reason.
    ///
    /// # Errors
    /// Returns `PlanStepError::InvalidTransition` if the step is not `InProgress`.
    pub fn fail(&mut self, reason: impl Into<String>) -> Result<(), PlanStepError> {
        if self.status != PlanStatus::InProgress {
            return Err(PlanStepError::InvalidTransition(format!(
                "cannot fail step {}: status is {:?}",
                self.index, self.status
            )));
        }
        self.status = PlanStatus::Failed(reason.into());
        Ok(())
    }

    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self.status, PlanStatus::Completed | PlanStatus::Failed(_))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPlan {
    pub goal: String,
    pub steps: Vec<PlanStep>,
    pub created_at: DateTime<Utc>,
}

impl AgentPlan {
    pub fn new(goal: impl Into<String>, step_descriptions: Vec<String>) -> Self {
        let steps = step_descriptions
            .into_iter()
            .enumerate()
            .map(|(i, desc)| PlanStep::new(i, desc))
            .collect();
        Self {
            goal: goal.into(),
            steps,
            created_at: Utc::now(),
        }
    }

    #[must_use]
    pub fn completed_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == PlanStatus::Completed)
            .count()
    }

    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.steps
            .iter()
            .filter(|s| s.status == PlanStatus::Pending)
            .count()
    }

    #[must_use]
    pub fn current_step(&self) -> Option<&PlanStep> {
        self.steps
            .iter()
            .find(|s| s.status == PlanStatus::InProgress)
            .or_else(|| self.steps.iter().find(|s| s.status == PlanStatus::Pending))
    }

    pub fn current_step_mut(&mut self) -> Option<&mut PlanStep> {
        // First look for InProgress
        if let Some(pos) = self
            .steps
            .iter()
            .position(|s| s.status == PlanStatus::InProgress)
        {
            return Some(&mut self.steps[pos]);
        }
        // Then first Pending
        if let Some(pos) = self
            .steps
            .iter()
            .position(|s| s.status == PlanStatus::Pending)
        {
            return Some(&mut self.steps[pos]);
        }
        None
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.steps.iter().all(PlanStep::is_terminal)
    }

    #[must_use]
    pub fn progress_fraction(&self) -> f64 {
        if self.steps.is_empty() {
            return 1.0;
        }
        let completed = u32::try_from(self.completed_count()).unwrap_or(u32::MAX);
        let total = u32::try_from(self.steps.len()).unwrap_or(1);
        f64::from(completed) / f64::from(total)
    }
}

#[derive(Debug)]
pub enum PlanStepError {
    InvalidTransition(String),
}

impl std::fmt::Display for PlanStepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidTransition(e) => write!(f, "plan step error: {e}"),
        }
    }
}

impl std::error::Error for PlanStepError {}
