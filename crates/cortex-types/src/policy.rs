use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{OwnedScope, PermissionRequestId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    Strict,
    Balanced,
    Open,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActionRisk {
    pub data_access: f32,
    pub side_effect: f32,
    pub cross_owner: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionDecision {
    Allow,
    RequireConfirmation,
    Deny,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionRequest {
    pub id: PermissionRequestId,
    pub scope: OwnedScope,
    pub tool_name: String,
    pub action: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionResolution {
    pub request_id: PermissionRequestId,
    pub scope: OwnedScope,
    pub decision: PermissionDecision,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionResolutionError {
    WrongOwner,
    WrongRequest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionStatus {
    Pending,
    Approved,
    Denied,
    TimedOut,
    Cancelled,
    Superseded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLifecycleError {
    NotPending,
    Resolution(PermissionResolutionError),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionTicket {
    pub request: PermissionRequest,
    pub status: PermissionStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PolicyMode {
    #[must_use]
    pub fn decide(self, risk: ActionRisk) -> PermissionDecision {
        if risk.cross_owner {
            return PermissionDecision::Deny;
        }
        let score = risk.data_access.mul_add(0.45, risk.side_effect * 0.55);
        match self {
            Self::Balanced if score >= 0.5 => PermissionDecision::RequireConfirmation,
            Self::Open if score >= 0.9 => PermissionDecision::RequireConfirmation,
            Self::Strict if score > 0.0 => PermissionDecision::RequireConfirmation,
            Self::Strict | Self::Balanced | Self::Open => PermissionDecision::Allow,
        }
    }
}

impl PermissionRequest {
    #[must_use]
    pub fn new(scope: OwnedScope, tool_name: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            id: PermissionRequestId::new(),
            scope,
            tool_name: tool_name.into(),
            action: action.into(),
        }
    }

    #[must_use]
    pub fn can_be_resolved_by(&self, scope: &OwnedScope) -> bool {
        if self.scope.tenant_id != scope.tenant_id || self.scope.actor_id != scope.actor_id {
            return false;
        }
        match (&self.scope.client_id, &scope.client_id) {
            (Some(expected), Some(actual)) => expected == actual,
            (Some(_), None) => false,
            (None, _) => true,
        }
    }

    /// # Errors
    /// Returns an error when the resolution references another request or owner.
    pub fn resolve(
        &self,
        resolution: &PermissionResolution,
    ) -> Result<PermissionDecision, PermissionResolutionError> {
        if resolution.request_id != self.id {
            return Err(PermissionResolutionError::WrongRequest);
        }
        if !self.can_be_resolved_by(&resolution.scope) {
            return Err(PermissionResolutionError::WrongOwner);
        }
        Ok(resolution.decision)
    }
}

impl PermissionResolution {
    #[must_use]
    pub const fn new(
        request_id: PermissionRequestId,
        scope: OwnedScope,
        decision: PermissionDecision,
    ) -> Self {
        Self {
            request_id,
            scope,
            decision,
        }
    }
}

impl PermissionTicket {
    #[must_use]
    pub fn new(request: PermissionRequest) -> Self {
        Self::new_at(request, Utc::now())
    }

    #[must_use]
    pub const fn new_at(request: PermissionRequest, now: DateTime<Utc>) -> Self {
        Self {
            request,
            status: PermissionStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    /// # Errors
    /// Returns an error when the ticket is no longer pending or the resolution
    /// does not match the request owner and id.
    pub fn resolve(
        &mut self,
        resolution: &PermissionResolution,
        now: DateTime<Utc>,
    ) -> Result<PermissionStatus, PermissionLifecycleError> {
        self.ensure_pending()?;
        let decision = self
            .request
            .resolve(resolution)
            .map_err(PermissionLifecycleError::Resolution)?;
        self.status = match decision {
            PermissionDecision::Allow => PermissionStatus::Approved,
            PermissionDecision::Deny => PermissionStatus::Denied,
            PermissionDecision::RequireConfirmation => PermissionStatus::Pending,
        };
        self.updated_at = now;
        Ok(self.status)
    }

    /// # Errors
    /// Returns an error when the ticket is no longer pending.
    pub fn time_out(&mut self, now: DateTime<Utc>) -> Result<(), PermissionLifecycleError> {
        self.finish(PermissionStatus::TimedOut, now)
    }

    /// # Errors
    /// Returns an error when the ticket is no longer pending.
    pub fn cancel(&mut self, now: DateTime<Utc>) -> Result<(), PermissionLifecycleError> {
        self.finish(PermissionStatus::Cancelled, now)
    }

    /// # Errors
    /// Returns an error when the ticket is no longer pending.
    pub fn supersede(&mut self, now: DateTime<Utc>) -> Result<(), PermissionLifecycleError> {
        self.finish(PermissionStatus::Superseded, now)
    }

    fn finish(
        &mut self,
        status: PermissionStatus,
        now: DateTime<Utc>,
    ) -> Result<(), PermissionLifecycleError> {
        self.ensure_pending()?;
        self.status = status;
        self.updated_at = now;
        Ok(())
    }

    fn ensure_pending(&self) -> Result<(), PermissionLifecycleError> {
        if self.status == PermissionStatus::Pending {
            Ok(())
        } else {
            Err(PermissionLifecycleError::NotPending)
        }
    }
}
