use std::sync::Arc;
use std::sync::atomic::Ordering;

use cortex_types::ConfirmationResponse;

use super::foreground::{ForegroundExecution, ForegroundSlotError};
use super::permissions::PendingPermissionEntry;
use super::{DaemonState, InjectMessageResult};

type OnTpnComplete<'a> = &'a (dyn Fn() + Send + Sync);

struct ForegroundWaiter<'a>(&'a std::sync::atomic::AtomicUsize);

impl<'a> ForegroundWaiter<'a> {
    fn new(counter: &'a std::sync::atomic::AtomicUsize) -> Self {
        counter.fetch_add(1, Ordering::Relaxed);
        Self(counter)
    }
}

impl Drop for ForegroundWaiter<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

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

impl DaemonState {
    pub(super) fn control_for_stop(
        &self,
        session_id: Option<&str>,
    ) -> Option<cortex_turn::orchestrator::TurnControl> {
        if let Some(session_id) = session_id {
            return self
                .turn_controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(session_id)
                .cloned();
        }
        let active_session = self
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        active_session.as_deref().and_then(|active_session| {
            self.turn_controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(active_session)
                .cloned()
        })
    }

    pub(super) fn stop_target_session(&self, session_id: Option<&str>) -> Option<String> {
        session_id.map(str::to_owned).or_else(|| {
            self.active_turn_session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    pub(super) fn deny_pending_permissions_for_session(&self, session_id: &str) {
        let pending: Vec<(String, Arc<PendingPermissionEntry>)> = self
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(_, entry)| entry.info.session_id == session_id)
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect();
        if pending.is_empty() {
            return;
        }
        for (_, entry) in &pending {
            let _ = entry.resolve(ConfirmationResponse::Denied);
        }
        let mut permissions = self
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (id, _) in pending {
            permissions.remove(&id);
        }
    }

    pub(super) fn with_registered_turn_control<T>(
        &self,
        session_id: &str,
        execute: impl FnOnce(cortex_turn::orchestrator::TurnControl, OnTpnComplete<'_>) -> T,
    ) -> T {
        let registration = TurnControlRegistration::new(self, session_id);
        let tpn_control = registration.control();
        let release_inbox = move || tpn_control.close_input_window();
        execute(registration.control(), &release_inbox)
    }

    pub(crate) async fn acquire_foreground_execution(
        &self,
        timeout: std::time::Duration,
    ) -> Result<ForegroundExecution, ForegroundSlotError> {
        let waiter = ForegroundWaiter::new(&self.foreground_waiters);
        let result =
            tokio::time::timeout(timeout, self.turn_semaphore.clone().acquire_owned()).await;
        drop(waiter);
        match result {
            Ok(Ok(permit)) => Ok(ForegroundExecution::queued(permit, &self.heartbeat_state)),
            Ok(Err(_)) => Err(ForegroundSlotError::ShuttingDown),
            Err(_) => Err(ForegroundSlotError::Timeout),
        }
    }

    pub(crate) fn begin_foreground_execution(&self) -> ForegroundExecution {
        ForegroundExecution::immediate(&self.heartbeat_state)
    }

    /// Inject a message into a running turn.
    pub(crate) fn inject_message(&self, session_id: &str, text: String) -> InjectMessageResult {
        let control = self
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned();
        control.map_or(InjectMessageResult::NoActiveTurn, |control| {
            if control.inject_message(text) {
                InjectMessageResult::Accepted
            } else {
                InjectMessageResult::InputClosed
            }
        })
    }

    #[must_use]
    pub(crate) fn has_active_turn(&self, session_id: &str) -> bool {
        self.turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(session_id)
    }

    #[must_use]
    pub(crate) fn session_has_recent_user_message(&self, session_id: &str, text: &str) -> bool {
        let in_memory = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .map(|session| session.history.clone());
        let history = in_memory.unwrap_or_else(|| {
            if let Some(meta) = self
                .session_store
                .list()
                .into_iter()
                .find(|meta| meta.id.to_string() == session_id)
            {
                self.session_store.load_history(&meta.id)
            } else {
                Vec::new()
            }
        });

        history
            .iter()
            .rev()
            .filter_map(|message| match message.role {
                cortex_types::Role::User => Some(message.text_content()),
                cortex_types::Role::Assistant => None,
            })
            .take(8)
            .any(|content| content.trim() == text.trim())
    }
}
