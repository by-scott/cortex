use cortex_types::{Message, Payload};

/// Signal-driven evolution trigger replacing hardcoded thresholds.
///
/// Six weighted signals determine whether prompt self-update should run:
/// - `correction_detected` (1.0): system response contains self-correction markers
/// - `explicit_preference` (0.8): user input contains preference expressions
/// - `new_domain` (0.6): user mentions domains absent from user profile
/// - `first_session_turn` (0.5): first turn in this session's history
/// - `tool_intensive` (0.4): 3+ tool calls this turn
/// - `long_input` (0.3): input > 500 chars
///
/// Threshold: 0.5 (any single high-weight signal suffices).
#[derive(Clone, Copy)]
pub struct EvolutionSignal {
    /// Bitfield: bit 0 = `correction_detected`, 1 = `explicit_preference`,
    /// 2 = `new_domain_detected`, 3 = `first_session_turn`, 4 = `tool_intensive`,
    /// 5 = `long_input`.
    flags: u8,
}

impl EvolutionSignal {
    pub(super) const CORRECTION_DETECTED: u8 = 1 << 0;
    pub(super) const EXPLICIT_PREFERENCE: u8 = 1 << 1;
    pub(super) const NEW_DOMAIN_DETECTED: u8 = 1 << 2;
    pub(super) const FIRST_SESSION_TURN: u8 = 1 << 3;
    pub(super) const TOOL_INTENSIVE: u8 = 1 << 4;
    pub(super) const LONG_INPUT: u8 = 1 << 5;

    pub(super) const fn new() -> Self {
        Self { flags: 0 }
    }

    pub(super) const fn set_if(&mut self, flag: u8, condition: bool) {
        if condition {
            self.flags |= flag;
        }
    }

    const fn has(self, flag: u8) -> bool {
        self.flags & flag != 0
    }

    fn score(self) -> f64 {
        const WEIGHTS: [(u8, f64); 6] = [
            (EvolutionSignal::CORRECTION_DETECTED, 1.0),
            (EvolutionSignal::EXPLICIT_PREFERENCE, 0.8),
            (EvolutionSignal::NEW_DOMAIN_DETECTED, 0.6),
            (EvolutionSignal::FIRST_SESSION_TURN, 0.5),
            (EvolutionSignal::TOOL_INTENSIVE, 0.4),
            (EvolutionSignal::LONG_INPUT, 0.3),
        ];
        WEIGHTS
            .iter()
            .filter(|(flag, _)| self.has(*flag))
            .map(|(_, weight)| weight)
            .sum()
    }

    /// Compute score using provided weights (ordered same as signal constants).
    fn score_with_weights(self, weights: &[f64; 6]) -> f64 {
        const FLAGS: [u8; 6] = [
            EvolutionSignal::CORRECTION_DETECTED,
            EvolutionSignal::EXPLICIT_PREFERENCE,
            EvolutionSignal::NEW_DOMAIN_DETECTED,
            EvolutionSignal::FIRST_SESSION_TURN,
            EvolutionSignal::TOOL_INTENSIVE,
            EvolutionSignal::LONG_INPUT,
        ];
        FLAGS
            .iter()
            .zip(weights.iter())
            .filter(|(flag, _)| self.has(**flag))
            .map(|(_, weight)| weight)
            .sum()
    }

    fn should_trigger(self) -> bool {
        self.score() >= 0.5
    }

    pub(super) fn should_trigger_with_weights(self, weights: &[f64; 6]) -> bool {
        self.score_with_weights(weights) >= 0.5
    }
}

/// Check whether the evolution signal warrants prompt self-update.
#[must_use]
pub fn should_evolve_prompts(
    prompt_manager: Option<&cortex_kernel::PromptManager>,
    events_log: &[Payload],
    input: &str,
    final_text: Option<&String>,
    history: &[Message],
) -> bool {
    prompt_manager.is_some_and(|pm| {
        if !pm.is_initialized() {
            return true;
        }
        let tool_call_count = events_log
            .iter()
            .filter(|e| matches!(e, Payload::ToolInvocationResult { .. }))
            .count();
        let response_text = final_text.map_or("", String::as_str);
        let user_profile = pm.get(cortex_types::PromptLayer::User).unwrap_or_default();
        let mut signal = EvolutionSignal::new();
        signal.set_if(
            EvolutionSignal::CORRECTION_DETECTED,
            crate::memory::user_signal::detect_correction(response_text),
        );
        signal.set_if(
            EvolutionSignal::EXPLICIT_PREFERENCE,
            crate::memory::user_signal::detect_preference(input),
        );
        signal.set_if(
            EvolutionSignal::NEW_DOMAIN_DETECTED,
            crate::memory::user_signal::detect_new_domain(input, &user_profile),
        );
        signal.set_if(EvolutionSignal::FIRST_SESSION_TURN, history.is_empty());
        signal.set_if(EvolutionSignal::TOOL_INTENSIVE, tool_call_count >= 3);
        signal.set_if(EvolutionSignal::LONG_INPUT, input.len() > 500);
        signal.should_trigger()
    })
}
