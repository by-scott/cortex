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
