use super::DaemonState;

pub(super) struct TurnControlRegistration<'a> {
    state: &'a DaemonState,
    session_id: String,
    control: cortex_turn::orchestrator::TurnControl,
}

impl<'a> TurnControlRegistration<'a> {
    pub(super) fn new(state: &'a DaemonState, session_id: &str) -> Self {
        let control = cortex_turn::orchestrator::TurnControl::new();
        state
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string(), control.clone());
        *state
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session_id.to_string());
        Self {
            state,
            session_id: session_id.to_string(),
            control,
        }
    }

    pub(super) fn control(&self) -> cortex_turn::orchestrator::TurnControl {
        self.control.clone()
    }
}

impl Drop for TurnControlRegistration<'_> {
    fn drop(&mut self) {
        self.state
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_id);
        let mut active = self
            .state
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_deref() == Some(self.session_id.as_str()) {
            *active = None;
        }
    }
}
