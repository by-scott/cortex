use std::sync::Arc;

use cortex_types::{
    ConfirmationCallback, ConfirmationRequest, ConfirmationResponse, PermissionDecision, RiskLevel,
};

/// Determines whether a tool invocation is permitted.
pub trait PermissionGate: Send + Sync {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision;
}

/// Default gate: Allow/Review → Approved, `RequireConfirmation` → Pending, Block → Denied.
pub struct DefaultPermissionGate;

impl PermissionGate for DefaultPermissionGate {
    fn check(&self, _tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        match risk_level {
            RiskLevel::Allow | RiskLevel::Review => PermissionDecision::Approved,
            RiskLevel::RequireConfirmation => PermissionDecision::Pending,
            RiskLevel::Block => PermissionDecision::Denied,
        }
    }
}

/// Gate that delegates `RequireConfirmation` to a user-facing callback.
///
/// - `Allow` / `Review` → `Approved` (low risk, proceed)
/// - `RequireConfirmation` → ask the callback → map response
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
            RiskLevel::Allow | RiskLevel::Review => PermissionDecision::Approved,
            RiskLevel::RequireConfirmation => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_gate_mappings() {
        let gate = DefaultPermissionGate;
        assert_eq!(
            gate.check("x", RiskLevel::Allow),
            PermissionDecision::Approved
        );
        assert_eq!(
            gate.check("x", RiskLevel::Review),
            PermissionDecision::Approved
        );
        assert_eq!(
            gate.check("x", RiskLevel::RequireConfirmation),
            PermissionDecision::Pending
        );
        assert_eq!(
            gate.check("x", RiskLevel::Block),
            PermissionDecision::Denied
        );
    }

    #[test]
    fn auto_approve_always() {
        let gate = AutoApproveGate;
        assert_eq!(
            gate.check("bash", RiskLevel::Block),
            PermissionDecision::Approved
        );
    }

    struct AlwaysApproveCallback;
    impl ConfirmationCallback for AlwaysApproveCallback {
        fn confirm(&self, _: &ConfirmationRequest) -> ConfirmationResponse {
            ConfirmationResponse::Approved
        }
    }

    struct AlwaysDenyCallback;
    impl ConfirmationCallback for AlwaysDenyCallback {
        fn confirm(&self, _: &ConfirmationRequest) -> ConfirmationResponse {
            ConfirmationResponse::Denied
        }
    }

    #[test]
    fn confirmable_gate_approves_low_risk() {
        let gate = ConfirmableGate::new(Arc::new(AlwaysDenyCallback));
        assert_eq!(
            gate.check("read", RiskLevel::Allow),
            PermissionDecision::Approved
        );
        assert_eq!(
            gate.check("read", RiskLevel::Review),
            PermissionDecision::Approved
        );
    }

    #[test]
    fn confirmable_gate_delegates_confirmation() {
        let approve = ConfirmableGate::new(Arc::new(AlwaysApproveCallback));
        assert_eq!(
            approve.check("bash", RiskLevel::RequireConfirmation),
            PermissionDecision::Approved
        );

        let deny = ConfirmableGate::new(Arc::new(AlwaysDenyCallback));
        assert_eq!(
            deny.check("bash", RiskLevel::RequireConfirmation),
            PermissionDecision::Denied
        );
    }

    #[test]
    fn confirmable_gate_blocks_unconditionally() {
        let gate = ConfirmableGate::new(Arc::new(AlwaysApproveCallback));
        assert_eq!(
            gate.check("bash", RiskLevel::Block),
            PermissionDecision::Denied
        );
    }
}
