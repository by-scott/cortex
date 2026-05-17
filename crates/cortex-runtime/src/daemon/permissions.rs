use std::sync::{Arc, Condvar, Mutex};

use cortex_types::{ConfirmationResponse, PermissionDecision, RiskLevel};

use super::{BroadcastEvent, BroadcastMessage, DaemonState, PendingPermissionInfo};

pub(super) struct PendingPermissionEntry {
    pub(super) info: PendingPermissionInfo,
    pub(super) decision: Mutex<Option<ConfirmationResponse>>,
    ready: Condvar,
}

impl PendingPermissionEntry {
    const fn new(info: PendingPermissionInfo) -> Self {
        Self {
            info,
            decision: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    pub(super) fn resolve(&self, response: ConfirmationResponse) -> bool {
        let mut decision = self
            .decision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if decision.is_some() {
            return false;
        }
        *decision = Some(response);
        drop(decision);
        self.ready.notify_all();
        true
    }
}

pub(super) struct RuntimePermissionGate<'a> {
    pub(super) state: &'a DaemonState,
    pub(super) session_id: &'a str,
    pub(super) actor: &'a str,
    pub(super) source: &'a str,
    pub(super) auto_approve_up_to: RiskLevel,
    pub(super) control: Option<&'a cortex_turn::orchestrator::TurnControl>,
    pub(super) on_event:
        Option<&'a (dyn Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync)>,
}

impl RuntimePermissionGate<'_> {
    fn confirmation_id() -> String {
        cortex_types::CorrelationId::new()
            .to_string()
            .chars()
            .take(8)
            .collect()
    }
}

impl cortex_turn::risk::PermissionGate for RuntimePermissionGate<'_> {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        self.check_with_explanation(tool_name, risk_level, "")
    }

    fn check_with_explanation(
        &self,
        tool_name: &str,
        risk_level: RiskLevel,
        explanation: &str,
    ) -> PermissionDecision {
        if risk_level == RiskLevel::Block {
            return PermissionDecision::Denied;
        }
        if risk_level <= self.auto_approve_up_to {
            return PermissionDecision::Approved;
        }

        let id = Self::confirmation_id();
        let expires_at = chrono::Utc::now() + chrono::Duration::days(36_500);
        let info = PendingPermissionInfo {
            id: id.clone(),
            session_id: self.session_id.to_string(),
            actor: self.actor.to_string(),
            source: self.source.to_string(),
            tool_name: tool_name.to_string(),
            risk_level,
            explanation: explanation.to_string(),
            expires_at,
        };
        let entry = Arc::new(PendingPermissionEntry::new(info.clone()));
        self.state
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone(), Arc::clone(&entry));

        let _ = self
            .state
            .session_broadcast(self.session_id)
            .send(BroadcastMessage {
                session_id: self.session_id.to_string(),
                source: "permission".to_string(),
                event: BroadcastEvent::PermissionRequested(info),
            });
        if let Some(on_event) = self.on_event {
            on_event(&cortex_turn::orchestrator::TurnStreamEvent::Text {
                lane: cortex_turn::orchestrator::StreamLane::Observer,
                source: Some("permission".to_string()),
                content: entry.info.prompt_text(),
            });
        }

        let decision = self.wait_for_decision_or_cancel(&entry);

        self.state
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        match decision {
            ConfirmationResponse::Approved => PermissionDecision::Approved,
            ConfirmationResponse::Denied => PermissionDecision::Denied,
        }
    }
}

impl RuntimePermissionGate<'_> {
    fn wait_for_decision_or_cancel(&self, entry: &PendingPermissionEntry) -> ConfirmationResponse {
        let poll_interval = std::time::Duration::from_millis(200);
        loop {
            if self
                .control
                .is_some_and(cortex_turn::orchestrator::TurnControl::is_cancel_requested)
            {
                break ConfirmationResponse::Denied;
            }
            let wait_result = {
                let guard = entry
                    .decision
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                entry.ready.wait_timeout(guard, poll_interval)
            };
            let Ok((guard, wait_result)) = wait_result else {
                break ConfirmationResponse::Denied;
            };
            if let Some(response) = *guard {
                break response;
            }
            if wait_result.timed_out()
                && self
                    .control
                    .is_some_and(cortex_turn::orchestrator::TurnControl::is_cancel_requested)
            {
                break ConfirmationResponse::Denied;
            }
        }
    }
}
