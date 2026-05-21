use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ForegroundSlotError {
    ShuttingDown,
    Timeout,
}

impl ForegroundSlotError {
    pub const fn operator_detail(self) -> &'static str {
        match self {
            Self::ShuttingDown => "service shutting down",
            Self::Timeout => "another turn is in progress -- timed out after 30s",
        }
    }

    pub const fn user_message(self) -> &'static str {
        match self {
            Self::ShuttingDown => "Turn queue unavailable.",
            Self::Timeout => "Another turn is in progress. Please wait.",
        }
    }
}

/// RAII guard that marks the foreground runtime as busy for the duration of an
/// active foreground execution.
struct ForegroundInner {
    state: Arc<crate::heartbeat::HeartbeatState>,
    permit: std::sync::Mutex<Option<tokio::sync::OwnedSemaphorePermit>>,
    active: AtomicBool,
}

impl ForegroundInner {
    fn acquire(
        state: &Arc<crate::heartbeat::HeartbeatState>,
        permit: Option<tokio::sync::OwnedSemaphorePermit>,
    ) -> Self {
        state
            .foreground_busy
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self {
            state: Arc::clone(state),
            permit: std::sync::Mutex::new(permit),
            active: AtomicBool::new(true),
        }
    }

    fn finish(&self) {
        if self.active.swap(false, Ordering::Relaxed) {
            self.state
                .foreground_busy
                .store(false, std::sync::atomic::Ordering::Relaxed);
            self.state.touch();
        }
        self.permit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
    }
}

#[derive(Clone)]
pub struct ForegroundReleaseHandle {
    inner: Arc<ForegroundInner>,
}

impl ForegroundReleaseHandle {
    pub fn finish_visible(&self) {
        self.inner.finish();
    }
}

/// Unified foreground execution scope that keeps queue ownership and heartbeat
/// busy-state aligned for the lifetime of one user-visible turn.
pub struct ForegroundExecution {
    inner: Arc<ForegroundInner>,
}

impl ForegroundExecution {
    pub fn queued(
        permit: tokio::sync::OwnedSemaphorePermit,
        state: &Arc<crate::heartbeat::HeartbeatState>,
    ) -> Self {
        Self {
            inner: Arc::new(ForegroundInner::acquire(state, Some(permit))),
        }
    }

    pub fn immediate(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        Self {
            inner: Arc::new(ForegroundInner::acquire(state, None)),
        }
    }

    pub fn release_handle(&self) -> ForegroundReleaseHandle {
        ForegroundReleaseHandle {
            inner: Arc::clone(&self.inner),
        }
    }

    pub fn finish_visible(&self) {
        self.inner.finish();
    }
}

impl Drop for ForegroundExecution {
    fn drop(&mut self) {
        self.inner.finish();
    }
}
