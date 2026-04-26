use serde::{Deserialize, Serialize};

use crate::OwnedScope;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStep {
    Backup,
    Migrate,
    Install,
    SmokeTest,
    Package,
    Publish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentStatus {
    Pending,
    Passed,
    Failed,
    RolledBack,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentRecord {
    pub step: DeploymentStep,
    pub status: DeploymentStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<DeploymentEvidence>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentEvidence {
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rollback: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentArtifact {
    pub step: DeploymentStep,
    pub path: String,
    pub checksum: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeploymentPlan {
    pub scope: OwnedScope,
    pub records: Vec<DeploymentRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeploymentError {
    AlreadyFailed,
    NotFailed { step: DeploymentStep },
    OutOfOrder { step: DeploymentStep },
    UnknownStep { step: DeploymentStep },
}

impl DeploymentPlan {
    #[must_use]
    pub fn production_release(scope: OwnedScope) -> Self {
        Self {
            scope,
            records: [
                DeploymentStep::Backup,
                DeploymentStep::Migrate,
                DeploymentStep::Install,
                DeploymentStep::SmokeTest,
                DeploymentStep::Package,
                DeploymentStep::Publish,
            ]
            .into_iter()
            .map(|step| DeploymentRecord {
                step,
                status: DeploymentStatus::Pending,
                evidence: None,
            })
            .collect(),
        }
    }

    /// # Errors
    /// Returns an error when a previous step is incomplete, the step is
    /// unknown, or the plan has already failed.
    pub fn mark_passed(&mut self, step: DeploymentStep) -> Result<(), DeploymentError> {
        self.mark_passed_with_evidence(step, DeploymentEvidence::new("step passed"))
    }

    /// # Errors
    /// Returns an error when a previous step is incomplete, the step is
    /// unknown, or the plan has already failed.
    pub fn mark_passed_with_evidence(
        &mut self,
        step: DeploymentStep,
        evidence: DeploymentEvidence,
    ) -> Result<(), DeploymentError> {
        if self.rollback_required() {
            return Err(DeploymentError::AlreadyFailed);
        }
        let index = self.index_of(step)?;
        if self.records[..index]
            .iter()
            .any(|record| record.status != DeploymentStatus::Passed)
        {
            return Err(DeploymentError::OutOfOrder { step });
        }
        self.records[index].status = DeploymentStatus::Passed;
        self.records[index].evidence = Some(evidence);
        Ok(())
    }

    /// # Errors
    /// Returns an error when the step is not part of this plan.
    pub fn mark_failed(&mut self, step: DeploymentStep) -> Result<(), DeploymentError> {
        self.mark_failed_with_evidence(
            step,
            DeploymentEvidence::new("step failed").with_rollback("manual rollback required"),
        )
    }

    /// # Errors
    /// Returns an error when the step is not part of this plan.
    pub fn mark_failed_with_evidence(
        &mut self,
        step: DeploymentStep,
        evidence: DeploymentEvidence,
    ) -> Result<(), DeploymentError> {
        let index = self.index_of(step)?;
        self.records[index].status = DeploymentStatus::Failed;
        self.records[index].evidence = Some(evidence);
        Ok(())
    }

    #[must_use]
    pub fn release_ready(&self) -> bool {
        self.records
            .iter()
            .all(|record| record.status == DeploymentStatus::Passed)
    }

    #[must_use]
    pub fn rollback_required(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.status == DeploymentStatus::Failed)
    }

    /// # Errors
    /// Returns an error when the step is unknown or is not currently failed.
    pub fn mark_rolled_back(&mut self, step: DeploymentStep) -> Result<(), DeploymentError> {
        let index = self.index_of(step)?;
        if self.records[index].status != DeploymentStatus::Failed {
            return Err(DeploymentError::NotFailed { step });
        }
        self.records[index].status = DeploymentStatus::RolledBack;
        Ok(())
    }

    #[must_use]
    pub fn rollback_complete(&self) -> bool {
        self.records
            .iter()
            .any(|record| record.status == DeploymentStatus::RolledBack)
            && !self.rollback_required()
    }

    #[must_use]
    pub fn rollback_actions(&self) -> Vec<String> {
        self.records
            .iter()
            .filter(|record| record.status == DeploymentStatus::Failed)
            .filter_map(|record| record.evidence.as_ref())
            .filter_map(|evidence| evidence.rollback.clone())
            .collect()
    }

    #[must_use]
    pub fn artifact_manifest(&self) -> Vec<DeploymentArtifact> {
        self.records
            .iter()
            .filter_map(|record| {
                let evidence = record.evidence.as_ref()?;
                Some(DeploymentArtifact {
                    step: record.step,
                    path: evidence.artifact.clone()?,
                    checksum: evidence.checksum.clone()?,
                })
            })
            .collect()
    }

    fn index_of(&self, step: DeploymentStep) -> Result<usize, DeploymentError> {
        self.records
            .iter()
            .position(|record| record.step == step)
            .ok_or(DeploymentError::UnknownStep { step })
    }
}

impl DeploymentEvidence {
    #[must_use]
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
            artifact: None,
            checksum: None,
            rollback: None,
        }
    }

    #[must_use]
    pub fn with_artifact(mut self, path: impl Into<String>, checksum: impl Into<String>) -> Self {
        self.artifact = Some(path.into());
        self.checksum = Some(checksum.into());
        self
    }

    #[must_use]
    pub fn with_rollback(mut self, rollback: impl Into<String>) -> Self {
        self.rollback = Some(rollback.into());
        self
    }
}
