use std::collections::VecDeque;

use cortex_types::Message;

use super::{TraceCategory, TurnTracer};

#[derive(Default)]
struct TurnControlState {
    cancel_requested: std::sync::atomic::AtomicBool,
    accepting_input: std::sync::atomic::AtomicBool,
    pending_signals: std::sync::Mutex<VecDeque<TurnControlSignal>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TurnControlSignal {
    CancelRequested,
    UserInput(String),
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TurnControlPoll {
    cancel_requested: bool,
    injected_messages: Vec<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum TurnControlAction {
    #[default]
    Continue,
    RestartTurn,
    AbortTurn,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum TurnControlBoundary {
    #[default]
    Continue,
    RestartTurn,
    AbortTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnControlCheckpoint {
    IterationBoundary,
    ToolBatchBoundary,
}

impl TurnControlCheckpoint {
    const fn cancel_trace(self) -> &'static str {
        match self {
            Self::IterationBoundary => "Turn cancelled by user (/stop)",
            Self::ToolBatchBoundary => "Turn cancelled during tool batch",
        }
    }

    const fn input_trace(self) -> &'static str {
        match self {
            Self::IterationBoundary => "Injected mid-turn user message",
            Self::ToolBatchBoundary => "Injected mid-turn user message during tool batch",
        }
    }
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct TurnControlDispatch {
    action: TurnControlAction,
    injected_messages: Vec<String>,
}

impl TurnControlDispatch {
    fn apply_to_history(&self, history: &mut Vec<Message>) {
        for msg in &self.injected_messages {
            history.push(Message::user(msg));
        }
    }

    #[must_use]
    const fn trace_message(&self, checkpoint: TurnControlCheckpoint) -> Option<&'static str> {
        match self.action {
            TurnControlAction::Continue => None,
            TurnControlAction::RestartTurn => Some(checkpoint.input_trace()),
            TurnControlAction::AbortTurn => Some(checkpoint.cancel_trace()),
        }
    }

    #[must_use]
    const fn boundary(&self) -> TurnControlBoundary {
        match self.action {
            TurnControlAction::Continue => TurnControlBoundary::Continue,
            TurnControlAction::RestartTurn => TurnControlBoundary::RestartTurn,
            TurnControlAction::AbortTurn => TurnControlBoundary::AbortTurn,
        }
    }
}

/// Shared control-plane handle for a running turn.
///
/// This separates runtime controls from the turn's data/context payload:
/// cancellation, mid-turn user input, and the answer boundary after TPN.
#[derive(Clone, Default)]
pub struct TurnControl {
    state: std::sync::Arc<TurnControlState>,
}

impl TurnControl {
    #[must_use]
    pub fn new() -> Self {
        let control = Self::default();
        control
            .state
            .accepting_input
            .store(true, std::sync::atomic::Ordering::Relaxed);
        control
    }

    pub fn request_cancel(&self) {
        let was_requested = self
            .state
            .cancel_requested
            .swap(true, std::sync::atomic::Ordering::Relaxed);
        if !was_requested {
            self.state
                .pending_signals
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push_back(TurnControlSignal::CancelRequested);
        }
    }

    #[must_use]
    pub fn is_cancel_requested(&self) -> bool {
        self.state
            .cancel_requested
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    #[must_use]
    pub fn inject_message(&self, text: String) -> bool {
        if !self
            .state
            .accepting_input
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            return false;
        }
        self.state
            .pending_signals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push_back(TurnControlSignal::UserInput(text));
        true
    }

    #[must_use]
    fn poll(&self) -> TurnControlPoll {
        let signals: Vec<_> = self
            .state
            .pending_signals
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .drain(..)
            .collect();
        let mut poll = TurnControlPoll::default();
        for signal in signals {
            match signal {
                TurnControlSignal::CancelRequested => {
                    poll.cancel_requested = true;
                }
                TurnControlSignal::UserInput(text) => {
                    poll.injected_messages.push(text);
                }
            }
        }
        poll.cancel_requested |= self.is_cancel_requested();
        poll
    }

    #[must_use]
    fn dispatch(&self) -> TurnControlDispatch {
        let poll = self.poll();
        let action = if poll.cancel_requested {
            TurnControlAction::AbortTurn
        } else if poll.injected_messages.is_empty() {
            TurnControlAction::Continue
        } else {
            TurnControlAction::RestartTurn
        };
        TurnControlDispatch {
            action,
            injected_messages: poll.injected_messages,
        }
    }

    #[must_use]
    pub(crate) fn execution_boundary(&self) -> TurnControlBoundary {
        if self.is_cancel_requested() {
            TurnControlBoundary::AbortTurn
        } else {
            TurnControlBoundary::Continue
        }
    }

    pub fn close_input_window(&self) {
        self.state
            .accepting_input
            .store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

pub fn dispatch_turn_control(
    control: Option<&TurnControl>,
    history: &mut Vec<Message>,
    tracer: &dyn TurnTracer,
    checkpoint: TurnControlCheckpoint,
) -> TurnControlBoundary {
    let Some(control) = control else {
        return TurnControlBoundary::Continue;
    };
    let dispatch = control.dispatch();
    dispatch.apply_to_history(history);
    if let Some(message) = dispatch.trace_message(checkpoint) {
        tracer.trace_at(
            TraceCategory::Phase,
            cortex_types::TraceLevel::Minimal,
            message,
        );
    }
    dispatch.boundary()
}
