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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_transitions() {
        let mut step = PlanStep::new(0, "do something");
        assert_eq!(step.status, PlanStatus::Pending);

        step.start().unwrap();
        assert_eq!(step.status, PlanStatus::InProgress);

        step.complete().unwrap();
        assert_eq!(step.status, PlanStatus::Completed);
        assert!(step.is_terminal());
    }

    #[test]
    fn step_fail_transition() {
        let mut step = PlanStep::new(0, "risky op");
        step.start().unwrap();
        step.fail("network error").unwrap();
        assert!(matches!(step.status, PlanStatus::Failed(_)));
        assert!(step.is_terminal());
    }

    #[test]
    fn step_cannot_start_twice() {
        let mut step = PlanStep::new(0, "x");
        step.start().unwrap();
        assert!(step.start().is_err());
    }

    #[test]
    fn step_cannot_complete_pending() {
        let mut step = PlanStep::new(0, "x");
        assert!(step.complete().is_err());
    }

    #[test]
    fn step_cannot_fail_pending() {
        let mut step = PlanStep::new(0, "x");
        assert!(step.fail("reason").is_err());
    }

    #[test]
    fn step_with_tool_hint() {
        let step = PlanStep::new(0, "read config").with_tool_hint("read");
        assert_eq!(step.tool_hint.as_deref(), Some("read"));
    }

    #[test]
    fn plan_new() {
        let plan = AgentPlan::new(
            "deploy",
            vec!["build".into(), "test".into(), "deploy".into()],
        );
        assert_eq!(plan.goal, "deploy");
        assert_eq!(plan.steps.len(), 3);
        assert_eq!(plan.pending_count(), 3);
        assert_eq!(plan.completed_count(), 0);
        assert!(!plan.is_finished());
    }

    #[test]
    fn plan_progress() {
        let mut plan = AgentPlan::new("task", vec!["a".into(), "b".into()]);

        plan.steps[0].start().unwrap();
        assert_eq!(plan.current_step().unwrap().index, 0);

        plan.steps[0].complete().unwrap();
        assert_eq!(plan.completed_count(), 1);
        assert_eq!(plan.pending_count(), 1);
        assert!(!plan.is_finished());

        plan.steps[1].start().unwrap();
        plan.steps[1].complete().unwrap();
        assert!(plan.is_finished());
        assert!((plan.progress_fraction() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_finished_with_failures() {
        let mut plan = AgentPlan::new("task", vec!["a".into(), "b".into()]);
        plan.steps[0].start().unwrap();
        plan.steps[0].complete().unwrap();
        plan.steps[1].start().unwrap();
        plan.steps[1].fail("boom").unwrap();
        assert!(plan.is_finished());
    }

    #[test]
    fn plan_current_step_mut() {
        let mut plan = AgentPlan::new("task", vec!["a".into(), "b".into()]);
        {
            let step = plan.current_step_mut().unwrap();
            step.start().unwrap();
        }
        assert_eq!(plan.steps[0].status, PlanStatus::InProgress);
    }

    #[test]
    fn plan_empty_is_finished() {
        let plan = AgentPlan::new("empty", vec![]);
        assert!(plan.is_finished());
        assert!((plan.progress_fraction() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn plan_json_roundtrip() {
        let plan = AgentPlan::new("test", vec!["step1".into(), "step2".into()]);
        let json = serde_json::to_string(&plan).unwrap();
        let back: AgentPlan = serde_json::from_str(&json).unwrap();
        assert_eq!(back.goal, "test");
        assert_eq!(back.steps.len(), 2);
    }
}
