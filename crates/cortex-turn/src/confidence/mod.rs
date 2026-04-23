use cortex_types::{ConfidenceLevel, Payload};

const POSITIVE_DELTA: f64 = 0.1;
const NEGATIVE_DELTA: f64 = 0.15;
const DENIAL_DELTA: f64 = 0.2;
const INITIAL_SCORE: f64 = 0.5;

/// Tracks decision confidence through evidence accumulation.
///
/// Based on a simplified drift-diffusion model: positive evidence
/// increases confidence, negative evidence decreases it.
pub struct ConfidenceTracker {
    score: f64,
    positive_count: usize,
    negative_count: usize,
}

impl Default for ConfidenceTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfidenceTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            score: INITIAL_SCORE,
            positive_count: 0,
            negative_count: 0,
        }
    }

    /// Record a successful tool execution (positive evidence).
    pub fn record_success(&mut self) {
        self.positive_count += 1;
        self.score = (self.score + POSITIVE_DELTA).min(1.0);
    }

    /// Record a failed tool execution (negative evidence).
    pub fn record_failure(&mut self) {
        self.negative_count += 1;
        self.score = (self.score - NEGATIVE_DELTA).max(0.0);
    }

    /// Record a permission denial (strong negative evidence).
    pub fn record_denial(&mut self) {
        self.negative_count += 1;
        self.score = (self.score - DENIAL_DELTA).max(0.0);
    }

    /// Assess current confidence and produce journal events.
    #[must_use]
    pub fn assess(&self) -> Vec<Payload> {
        let level = ConfidenceLevel::from_score(self.score);
        let evidence_count = self.positive_count + self.negative_count;

        let mut events = vec![Payload::ConfidenceAssessed {
            level: level.to_string(),
            score: self.score,
            evidence_count,
        }];

        if matches!(level, ConfidenceLevel::Low | ConfidenceLevel::Uncertain) {
            events.push(Payload::ConfidenceLow {
                score: self.score,
                suggestion: format!(
                    "confidence {} ({:.2}): consider additional verification before proceeding",
                    level, self.score
                ),
            });
        }

        events
    }

    #[must_use]
    pub const fn score(&self) -> f64 {
        self.score
    }

    #[must_use]
    pub fn level(&self) -> ConfidenceLevel {
        ConfidenceLevel::from_score(self.score)
    }

    #[must_use]
    pub const fn evidence_count(&self) -> usize {
        self.positive_count + self.negative_count
    }
}
