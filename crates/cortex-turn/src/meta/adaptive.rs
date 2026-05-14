use super::monitor::AlertKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertOutcome {
    Helpful,
    FalseAlarm,
    Missed,
    Harmful,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertIntervention {
    StrategyChanged,
    RetrievedEvidence,
    AskedHuman,
    CompactedContext,
    Rested,
    NoAction,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AlertFeedback {
    pub kind: AlertKind,
    pub outcome: AlertOutcome,
    pub intervention: AlertIntervention,
    pub confidence_before: f64,
    pub confidence_after: f64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CalibrationSnapshot {
    pub kind: AlertKind,
    pub confirmed: usize,
    pub false_positives: usize,
    pub missed: usize,
    pub harmful: usize,
    pub intervention_total: usize,
    pub intervention_successes: usize,
    pub precision: Option<f64>,
    pub intervention_success_rate: Option<f64>,
    pub confidence_delta_avg: f64,
    pub current_threshold: f64,
}

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
    missed: usize,
    harmful: usize,
    intervention_total: usize,
    intervention_successes: usize,
    confidence_delta_sum: f64,
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
            missed: 0,
            harmful: 0,
            intervention_total: 0,
            intervention_successes: 0,
            confidence_delta_sum: 0.0,
            total_since_adjust: 0,
        }
    }

    fn record(&mut self, feedback: AlertFeedback) {
        match feedback.outcome {
            AlertOutcome::Helpful => self.confirmed += 1,
            AlertOutcome::FalseAlarm => self.false_positives += 1,
            AlertOutcome::Missed => self.missed += 1,
            AlertOutcome::Harmful => self.harmful += 1,
        }
        if feedback.intervention != AlertIntervention::NoAction {
            self.intervention_total += 1;
            if feedback.confidence_after >= feedback.confidence_before
                && feedback.outcome == AlertOutcome::Helpful
            {
                self.intervention_successes += 1;
            }
        }
        self.confidence_delta_sum += feedback.confidence_after - feedback.confidence_before;
        self.total_since_adjust += 1;
    }

    fn precision(&self) -> Option<f64> {
        let total = self.confirmed + self.false_positives + self.harmful;
        if total == 0 {
            return None;
        }
        let confirmed = u32::try_from(self.confirmed).unwrap_or(u32::MAX);
        let total = u32::try_from(total).unwrap_or(1);
        Some(f64::from(confirmed) / f64::from(total))
    }

    fn intervention_success_rate(&self) -> Option<f64> {
        if self.intervention_total == 0 {
            return None;
        }
        let successes = u32::try_from(self.intervention_successes).unwrap_or(u32::MAX);
        let total = u32::try_from(self.intervention_total).unwrap_or(1);
        Some(f64::from(successes) / f64::from(total))
    }

    fn confidence_delta_avg(&self) -> f64 {
        let total = self.confirmed + self.false_positives + self.missed + self.harmful;
        if total == 0 {
            return 0.0;
        }
        let total = u32::try_from(total).unwrap_or(1);
        self.confidence_delta_sum / f64::from(total)
    }

    fn maybe_adjust(&mut self) {
        if self.total_since_adjust < ADJUST_INTERVAL {
            return;
        }

        if let Some(p) = self.precision() {
            let missed_pressure = self.missed > self.false_positives;
            if p < LOW_PRECISION || self.harmful > self.confirmed {
                self.current *= RELAX_FACTOR;
            } else if p > HIGH_PRECISION || missed_pressure {
                self.current *= TIGHTEN_FACTOR;
            }

            let lower = self.initial * (1.0 - BOUND_FACTOR);
            let upper = self.initial * (1.0 + BOUND_FACTOR);
            self.current = self.current.clamp(lower, upper);
        }

        self.total_since_adjust = 0;
    }

    fn snapshot(&self, kind: AlertKind) -> CalibrationSnapshot {
        CalibrationSnapshot {
            kind,
            confirmed: self.confirmed,
            false_positives: self.false_positives,
            missed: self.missed,
            harmful: self.harmful,
            intervention_total: self.intervention_total,
            intervention_successes: self.intervention_successes,
            precision: self.precision(),
            intervention_success_rate: self.intervention_success_rate(),
            confidence_delta_avg: self.confidence_delta_avg(),
            current_threshold: self.current,
        }
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
        let outcome = if is_true_positive {
            AlertOutcome::Helpful
        } else {
            AlertOutcome::FalseAlarm
        };
        self.record_feedback(AlertFeedback::new(
            *kind,
            outcome,
            AlertIntervention::NoAction,
        ));
    }

    /// Record a rich alert outcome for threshold and intervention calibration.
    pub fn record_feedback(&mut self, feedback: AlertFeedback) {
        let state = match feedback.kind {
            AlertKind::DoomLoop => &mut self.doom_loop,
            AlertKind::Fatigue => &mut self.fatigue,
            AlertKind::FrameAnchoring => &mut self.frame_anchoring,
            AlertKind::Duration | AlertKind::HealthDegraded => return,
        };
        state.record(feedback);
        state.maybe_adjust();
    }

    #[must_use]
    pub fn calibration_snapshot(&self, kind: AlertKind) -> Option<CalibrationSnapshot> {
        match kind {
            AlertKind::DoomLoop => Some(self.doom_loop.snapshot(kind)),
            AlertKind::Fatigue => Some(self.fatigue.snapshot(kind)),
            AlertKind::FrameAnchoring => Some(self.frame_anchoring.snapshot(kind)),
            AlertKind::Duration | AlertKind::HealthDegraded => None,
        }
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

impl AlertFeedback {
    #[must_use]
    pub const fn new(
        kind: AlertKind,
        outcome: AlertOutcome,
        intervention: AlertIntervention,
    ) -> Self {
        Self {
            kind,
            outcome,
            intervention,
            confidence_before: 0.0,
            confidence_after: 0.0,
        }
    }

    #[must_use]
    pub const fn with_confidence(mut self, before: f64, after: f64) -> Self {
        self.confidence_before = before.clamp(0.0, 1.0);
        self.confidence_after = after.clamp(0.0, 1.0);
        self
    }
}
