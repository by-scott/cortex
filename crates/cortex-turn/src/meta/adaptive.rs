use super::monitor::AlertKind;

/// Tracks alert outcomes and adjusts thresholds based on precision.
///
/// For each alert kind, tracks:
/// - `confirmed`: true positives (alert led to strategy change)
/// - `false_positives`: false positives (alert was irrelevant)
///
/// Adjusts thresholds every `ADJUST_INTERVAL` alerts:
/// - precision < 0.5: relax threshold (+10%)
/// - precision > 0.8: tighten threshold (-10%)
/// - bounded to +/-50% of initial value
pub struct AdaptiveThresholds {
    doom_loop: ThresholdState,
    fatigue: ThresholdState,
    frame_anchoring: ThresholdState,
}

struct ThresholdState {
    initial: f64,
    current: f64,
    confirmed: usize,
    false_positives: usize,
    total_since_adjust: usize,
}

const ADJUST_INTERVAL: usize = 10;
const RELAX_FACTOR: f64 = 1.10;
const TIGHTEN_FACTOR: f64 = 0.90;
const LOW_PRECISION: f64 = 0.5;
const HIGH_PRECISION: f64 = 0.8;
const BOUND_FACTOR: f64 = 0.5; // +/-50%

impl ThresholdState {
    const fn new(initial: f64) -> Self {
        Self {
            initial,
            current: initial,
            confirmed: 0,
            false_positives: 0,
            total_since_adjust: 0,
        }
    }

    const fn record(&mut self, is_true_positive: bool) {
        if is_true_positive {
            self.confirmed += 1;
        } else {
            self.false_positives += 1;
        }
        self.total_since_adjust += 1;
    }

    fn precision(&self) -> Option<f64> {
        let total = self.confirmed + self.false_positives;
        if total == 0 {
            return None;
        }
        let confirmed = u32::try_from(self.confirmed).unwrap_or(u32::MAX);
        let total = u32::try_from(total).unwrap_or(1);
        Some(f64::from(confirmed) / f64::from(total))
    }

    fn maybe_adjust(&mut self) {
        if self.total_since_adjust < ADJUST_INTERVAL {
            return;
        }

        if let Some(p) = self.precision() {
            if p < LOW_PRECISION {
                // Too many false positives -- relax (increase threshold)
                self.current *= RELAX_FACTOR;
            } else if p > HIGH_PRECISION {
                // High accuracy -- tighten (decrease threshold)
                self.current *= TIGHTEN_FACTOR;
            }

            // Clamp to +/-50% of initial
            let lower = self.initial * (1.0 - BOUND_FACTOR);
            let upper = self.initial * (1.0 + BOUND_FACTOR);
            self.current = self.current.clamp(lower, upper);
        }

        self.total_since_adjust = 0;
    }
}

impl AdaptiveThresholds {
    /// Create adaptive thresholds with initial values.
    ///
    /// - `doom_loop_threshold`: initial repeat count (e.g., 3.0)
    /// - `fatigue_threshold`: initial fatigue level (e.g., 0.8)
    /// - `frame_anchoring_threshold`: initial frame risk threshold (e.g., 0.5)
    #[must_use]
    pub const fn new(
        doom_loop_threshold: f64,
        fatigue_threshold: f64,
        frame_anchoring_threshold: f64,
    ) -> Self {
        Self {
            doom_loop: ThresholdState::new(doom_loop_threshold),
            fatigue: ThresholdState::new(fatigue_threshold),
            frame_anchoring: ThresholdState::new(frame_anchoring_threshold),
        }
    }

    /// Record the outcome of an alert.
    pub fn record_outcome(&mut self, kind: &AlertKind, is_true_positive: bool) {
        let state = match kind {
            AlertKind::DoomLoop => &mut self.doom_loop,
            AlertKind::Fatigue => &mut self.fatigue,
            AlertKind::FrameAnchoring => &mut self.frame_anchoring,
            AlertKind::Duration | AlertKind::HealthDegraded => return,
        };
        state.record(is_true_positive);
        state.maybe_adjust();
    }

    /// Get the current effective doom loop threshold (as `usize`, rounded).
    #[must_use]
    pub fn effective_doom_loop_threshold(&self) -> usize {
        // Value is always positive and bounded to +/-50% of initial (small values like 1.5..4.5).
        // Safe path: clamp to u32 range and convert without float-to-int cast.
        let rounded = self
            .doom_loop
            .current
            .round()
            .clamp(1.0, f64::from(u32::MAX));
        // Compare against integer thresholds to find the value
        let mut result = 1_u32;
        while f64::from(result) < rounded && result < u32::MAX {
            result += 1;
        }
        result as usize
    }

    /// Get the current effective fatigue threshold.
    #[must_use]
    pub const fn effective_fatigue_threshold(&self) -> f64 {
        self.fatigue.current
    }

    /// Get the current effective frame anchoring threshold.
    #[must_use]
    pub const fn effective_frame_threshold(&self) -> f64 {
        self.frame_anchoring.current
    }
}
