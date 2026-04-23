use std::sync::Arc;

use cortex_types::{
    ConfirmationCallback, ConfirmationRequest, ConfirmationResponse, PermissionDecision, RiskLevel,
};

/// Determines whether a tool invocation is permitted.
pub trait PermissionGate: Send + Sync {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision;
}

/// Gate that approves all non-block risk levels up to a configured threshold.
pub struct ThresholdPermissionGate {
    auto_approve_up_to: RiskLevel,
}

impl ThresholdPermissionGate {
    #[must_use]
    pub const fn new(auto_approve_up_to: RiskLevel) -> Self {
        Self { auto_approve_up_to }
    }
}

impl PermissionGate for ThresholdPermissionGate {
    fn check(&self, _tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        match risk_level {
            RiskLevel::Block => PermissionDecision::Denied,
            level if level <= self.auto_approve_up_to => PermissionDecision::Approved,
            RiskLevel::Review | RiskLevel::RequireConfirmation => PermissionDecision::Pending,
            RiskLevel::Allow => PermissionDecision::Approved,
        }
    }
}

/// Default gate: Allow → Approved, Review/RequireConfirmation → Pending, Block → Denied.
pub struct DefaultPermissionGate;

impl PermissionGate for DefaultPermissionGate {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        ThresholdPermissionGate::new(RiskLevel::Allow).check(tool_name, risk_level)
    }
}

/// Gate that delegates reviewable risk levels to a user-facing callback.
///
/// - `Allow` → `Approved` (low risk, proceed)
/// - `Review` / `RequireConfirmation` → ask the callback → map response
/// - `Block` → `Denied` (unconditionally)
pub struct ConfirmableGate {
    callback: Arc<dyn ConfirmationCallback>,
}

impl ConfirmableGate {
    pub fn new(callback: Arc<dyn ConfirmationCallback>) -> Self {
        Self { callback }
    }
}

impl PermissionGate for ConfirmableGate {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        match risk_level {
            RiskLevel::Allow => PermissionDecision::Approved,
            RiskLevel::Review | RiskLevel::RequireConfirmation => {
                let request = ConfirmationRequest {
                    tool_name: tool_name.to_string(),
                    risk_level,
                    description: format!(
                        "Tool '{tool_name}' requires confirmation (risk: {risk_level:?})"
                    ),
                };
                match self.callback.confirm(&request) {
                    ConfirmationResponse::Approved => PermissionDecision::Approved,
                    ConfirmationResponse::Denied => PermissionDecision::Denied,
                }
            }
            RiskLevel::Block => PermissionDecision::Denied,
        }
    }
}

/// Testing gate that always approves.
pub struct AutoApproveGate;

impl PermissionGate for AutoApproveGate {
    fn check(&self, _tool_name: &str, _risk_level: RiskLevel) -> PermissionDecision {
        PermissionDecision::Approved
    }
}
