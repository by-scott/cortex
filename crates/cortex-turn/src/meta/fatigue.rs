use std::f64::consts::LN_2;

const HALF_LIFE_MINUTES: f64 = 30.0;
const IDLE_RESET_MINUTES: f64 = 5.0;
const TURN_SCALE_DIVISOR: f64 = 20.0;

pub struct FatigueAccumulator {
    value: f64,
    threshold: f64,
    consecutive_turns: usize,
}

impl FatigueAccumulator {
    #[must_use]
    pub const fn new(threshold: f64) -> Self {
        Self {
            value: 0.0,
            threshold,
            consecutive_turns: 0,
        }
    }

    /// Accumulate fatigue from a turn. Formula:
    /// `value = min(1.0, complexity * (1 + turns/20) + value)`
    pub fn accumulate(&mut self, turn_complexity: f64) {
        self.consecutive_turns += 1;
        let turns = f64::from(u32::try_from(self.consecutive_turns).unwrap_or(u32::MAX));
        let scale = 1.0 + turns / TURN_SCALE_DIVISOR;
        self.value = turn_complexity.mul_add(scale, self.value).min(1.0);
    }

    #[must_use]
    pub fn should_rest(&self) -> bool {
        self.value >= self.threshold
    }

    #[must_use]
    pub const fn value(&self) -> f64 {
        self.value
    }

    /// Exponential decay with 30-minute half-life.
    /// Resets consecutive turns after 5 minutes idle.
    pub fn decay(&mut self, minutes_idle: f64) {
        let decay_factor = (-LN_2 * minutes_idle / HALF_LIFE_MINUTES).exp();
        self.value *= decay_factor;
        if minutes_idle > IDLE_RESET_MINUTES {
            self.consecutive_turns = 0;
        }
    }

    pub const fn reset(&mut self) {
        self.value = 0.0;
        self.consecutive_turns = 0;
    }
}
