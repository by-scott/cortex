use std::collections::HashMap;

use serde::Serialize;

#[derive(Debug, Clone, Default)]
struct SignalRecord {
    triggers: u32,
    improvements: u32,
}

/// Tracks precision of each evolution signal.
#[derive(Debug, Clone, Default)]
pub struct SignalPrecisionTracker {
    signals: HashMap<String, SignalRecord>,
}

impl SignalPrecisionTracker {
    pub fn record_trigger(&mut self, signal_name: &str) {
        self.signals
            .entry(signal_name.to_string())
            .or_default()
            .triggers += 1;
    }

    pub fn record_improvement(&mut self, signal_name: &str) {
        self.signals
            .entry(signal_name.to_string())
            .or_default()
            .improvements += 1;
    }

    #[must_use]
    pub fn precision(&self, signal_name: &str) -> f64 {
        self.signals.get(signal_name).map_or(0.0, |r| {
            if r.triggers == 0 {
                0.0
            } else {
                f64::from(r.improvements) / f64::from(r.triggers)
            }
        })
    }

    #[must_use]
    pub fn all_precisions(&self) -> HashMap<String, f64> {
        self.signals
            .keys()
            .map(|k| (k.clone(), self.precision(k)))
            .collect()
    }
}

#[derive(Debug, Clone, Default)]
struct UpdateRecord {
    pre_value: f64,
    post_value: f64,
}

/// Tracks quality of prompt/memory updates.
#[derive(Debug, Clone, Default)]
pub struct UpdateQualityScorer {
    records: Vec<UpdateRecord>,
}

impl UpdateQualityScorer {
    pub fn record_update(&mut self, pre_value: f64, post_value: f64) {
        self.records.push(UpdateRecord {
            pre_value,
            post_value,
        });
    }

    #[must_use]
    pub fn avg_quality_delta(&self) -> f64 {
        if self.records.is_empty() {
            return 0.0;
        }
        let sum: f64 = self
            .records
            .iter()
            .map(|r| r.post_value - r.pre_value)
            .sum();
        sum / f64::from(u32::try_from(self.records.len()).unwrap_or(u32::MAX))
    }

    #[must_use]
    pub const fn update_count(&self) -> usize {
        self.records.len()
    }
}

/// Tracks reconsolidation effectiveness (Nader 2000 model).
#[derive(Debug, Clone, Default)]
pub struct ReconsolidationTracker {
    total_marked: u32,
    updated_in_window: u32,
    degraded: u32,
    expired_unused: u32,
}

impl ReconsolidationTracker {
    pub const fn record_marked(&mut self) {
        self.total_marked = self.total_marked.saturating_add(1);
    }

    pub const fn record_updated(&mut self) {
        self.updated_in_window = self.updated_in_window.saturating_add(1);
    }

    pub const fn record_degraded(&mut self) {
        self.degraded = self.degraded.saturating_add(1);
    }

    pub const fn record_expired(&mut self) {
        self.expired_unused = self.expired_unused.saturating_add(1);
    }

    #[must_use]
    pub fn effectiveness_score(&self) -> f64 {
        if self.total_marked == 0 {
            return 0.0;
        }
        f64::from(self.updated_in_window) / f64::from(self.total_marked)
    }

    #[must_use]
    pub fn degradation_rate(&self) -> f64 {
        if self.total_marked == 0 {
            return 0.0;
        }
        f64::from(self.degraded) / f64::from(self.total_marked)
    }

    #[must_use]
    pub fn snapshot(&self) -> ReconsolidationSnapshot {
        ReconsolidationSnapshot {
            total_marked: self.total_marked,
            updated_in_window: self.updated_in_window,
            degraded: self.degraded,
            expired_unused: self.expired_unused,
            effectiveness_score: self.effectiveness_score(),
            degradation_rate: self.degradation_rate(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ReconsolidationSnapshot {
    pub total_marked: u32,
    pub updated_in_window: u32,
    pub degraded: u32,
    pub expired_unused: u32,
    pub effectiveness_score: f64,
    pub degradation_rate: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalibrationSnapshot {
    pub signal_precisions: HashMap<String, f64>,
    pub avg_update_quality: f64,
    pub update_count: usize,
    pub overall_score: f64,
}

/// Take a calibration snapshot.
///
/// Overall score = `clamp(mean_precision * (1 + clamp(quality_delta, -0.5, 0.5)), 0, 1)`
#[must_use]
pub fn take_snapshot(
    tracker: &SignalPrecisionTracker,
    scorer: &UpdateQualityScorer,
) -> CalibrationSnapshot {
    let precisions = tracker.all_precisions();
    let mean_precision = if precisions.is_empty() {
        0.0
    } else {
        precisions.values().sum::<f64>()
            / f64::from(u32::try_from(precisions.len()).unwrap_or(u32::MAX))
    };
    let quality_delta = scorer.avg_quality_delta().clamp(-0.5, 0.5);
    let quality_factor = 1.0 + quality_delta;
    let overall_score = (mean_precision * quality_factor).clamp(0.0, 1.0);

    CalibrationSnapshot {
        signal_precisions: precisions,
        avg_update_quality: scorer.avg_quality_delta(),
        update_count: scorer.update_count(),
        overall_score,
    }
}
