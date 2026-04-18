//! Performance profiling infrastructure for Turn latency tracking.
//!
//! Provides [`TurnLatencyTracker`] for recording per-phase latencies
//! and [`LatencyReport`] for statistical summaries.

use std::time::{Duration, Instant};

use cortex_types::TurnPhase;
use serde::Serialize;

/// Records per-phase and total Turn latencies.
pub struct TurnLatencyTracker {
    sn_samples: Vec<Duration>,
    tpn_samples: Vec<Duration>,
    dmn_samples: Vec<Duration>,
    turn_totals: Vec<Duration>,
}

impl Default for TurnLatencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl TurnLatencyTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            sn_samples: Vec::new(),
            tpn_samples: Vec::new(),
            dmn_samples: Vec::new(),
            turn_totals: Vec::new(),
        }
    }

    /// Record a phase duration.
    pub fn record_phase(&mut self, phase: TurnPhase, duration: Duration) {
        match phase {
            TurnPhase::Sn => self.sn_samples.push(duration),
            TurnPhase::Tpn => self.tpn_samples.push(duration),
            TurnPhase::Dmn => self.dmn_samples.push(duration),
        }
    }

    /// Record a total turn duration.
    pub fn record_turn(&mut self, duration: Duration) {
        self.turn_totals.push(duration);
    }

    /// Generate a report of all tracked latencies.
    #[must_use]
    pub fn report(&self) -> LatencyReport {
        LatencyReport {
            sn: compute_stats(&self.sn_samples),
            tpn: compute_stats(&self.tpn_samples),
            dmn: compute_stats(&self.dmn_samples),
            total: compute_stats(&self.turn_totals),
        }
    }

    /// Number of recorded turns.
    #[must_use]
    pub const fn turn_count(&self) -> usize {
        self.turn_totals.len()
    }
}

/// Statistical summary of latencies in milliseconds.
#[derive(Debug, Clone, Serialize)]
pub struct LatencyStats {
    pub count: usize,
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
    pub p95_ms: f64,
}

/// Full latency report across all phases.
#[derive(Debug, Clone, Serialize)]
pub struct LatencyReport {
    pub sn: LatencyStats,
    pub tpn: LatencyStats,
    pub dmn: LatencyStats,
    pub total: LatencyStats,
}

/// Compute statistics from a slice of durations.
fn compute_stats(durations: &[Duration]) -> LatencyStats {
    if durations.is_empty() {
        return LatencyStats {
            count: 0,
            min_ms: 0.0,
            max_ms: 0.0,
            avg_ms: 0.0,
            p95_ms: 0.0,
        };
    }

    let mut ms_values: Vec<f64> = durations.iter().map(|d| duration_to_ms(*d)).collect();
    ms_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let count = ms_values.len();
    let min_ms = ms_values[0];
    let max_ms = ms_values[count - 1];
    let sum: f64 = ms_values.iter().sum();
    let count_u32 = u32::try_from(count).unwrap_or(u32::MAX);
    let avg_ms = sum / f64::from(count_u32);

    let p95_idx_u32 = count_u32.saturating_mul(95) / 100;
    let p95_idx = usize::try_from(p95_idx_u32)
        .unwrap_or(count - 1)
        .min(count - 1);
    let p95_ms = ms_values[p95_idx];

    LatencyStats {
        count,
        min_ms,
        max_ms,
        avg_ms,
        p95_ms,
    }
}

fn duration_to_ms(d: Duration) -> f64 {
    let secs_u32 = u32::try_from(d.as_secs()).unwrap_or(u32::MAX);
    let nanos_u32 = d.subsec_nanos();
    f64::from(secs_u32).mul_add(1000.0, f64::from(nanos_u32) / 1_000_000.0)
}

/// Helper to time a closure and return its duration.
#[must_use]
pub fn time_fn<F: FnOnce()>(f: F) -> Duration {
    let start = Instant::now();
    f();
    start.elapsed()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latency_stats_basic() {
        let durations: Vec<Duration> = (1..=5).map(|i| Duration::from_millis(i * 10)).collect();
        let stats = compute_stats(&durations);
        assert_eq!(stats.count, 5);
        assert!((stats.min_ms - 10.0).abs() < 0.01);
        assert!((stats.max_ms - 50.0).abs() < 0.01);
        assert!((stats.avg_ms - 30.0).abs() < 0.01);
        assert!(stats.p95_ms >= 40.0);
    }

    #[test]
    fn latency_stats_empty() {
        let stats = compute_stats(&[]);
        assert_eq!(stats.count, 0);
        assert!(stats.avg_ms.abs() < f64::EPSILON);
    }

    #[test]
    fn latency_stats_single() {
        let stats = compute_stats(&[Duration::from_millis(42)]);
        assert_eq!(stats.count, 1);
        assert!((stats.min_ms - 42.0).abs() < 0.01);
        assert!((stats.max_ms - 42.0).abs() < 0.01);
        assert!((stats.avg_ms - 42.0).abs() < 0.01);
    }

    #[test]
    fn tracker_record_and_report() {
        let mut tracker = TurnLatencyTracker::new();
        tracker.record_phase(TurnPhase::Sn, Duration::from_millis(5));
        tracker.record_phase(TurnPhase::Tpn, Duration::from_millis(50));
        tracker.record_phase(TurnPhase::Dmn, Duration::from_millis(10));
        tracker.record_turn(Duration::from_millis(65));

        let report = tracker.report();
        assert_eq!(report.sn.count, 1);
        assert_eq!(report.tpn.count, 1);
        assert_eq!(report.dmn.count, 1);
        assert_eq!(report.total.count, 1);
        assert!((report.total.avg_ms - 65.0).abs() < 0.01);
    }

    #[test]
    fn tracker_multiple_turns() {
        let mut tracker = TurnLatencyTracker::new();
        for i in 1..=10 {
            tracker.record_turn(Duration::from_millis(i * 10));
        }

        let report = tracker.report();
        assert_eq!(report.total.count, 10);
        assert!((report.total.min_ms - 10.0).abs() < 0.01);
        assert!((report.total.max_ms - 100.0).abs() < 0.01);
        assert!((report.total.avg_ms - 55.0).abs() < 0.01);
    }

    #[test]
    fn turn_count_tracked() {
        let mut tracker = TurnLatencyTracker::new();
        assert_eq!(tracker.turn_count(), 0);
        tracker.record_turn(Duration::from_millis(10));
        tracker.record_turn(Duration::from_millis(20));
        assert_eq!(tracker.turn_count(), 2);
    }

    #[test]
    fn report_serializes() {
        let mut tracker = TurnLatencyTracker::new();
        tracker.record_turn(Duration::from_millis(100));
        let report = tracker.report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("avg_ms"));
        assert!(json.contains("p95_ms"));
    }

    #[test]
    fn phase_display() {
        assert_eq!(TurnPhase::Sn.to_string(), "SN");
        assert_eq!(TurnPhase::Tpn.to_string(), "TPN");
        assert_eq!(TurnPhase::Dmn.to_string(), "DMN");
    }

    #[test]
    fn time_fn_measures() {
        let d = time_fn(|| {
            std::thread::sleep(Duration::from_millis(10));
        });
        assert!(d >= Duration::from_millis(5));
    }

    #[test]
    fn default_tracker_is_empty() {
        let tracker = TurnLatencyTracker::default();
        assert_eq!(tracker.turn_count(), 0);
        let report = tracker.report();
        assert_eq!(report.total.count, 0);
    }

    #[test]
    fn duration_to_ms_converts() {
        let ms = duration_to_ms(Duration::from_millis(1500));
        assert!((ms - 1500.0).abs() < 0.01);
    }
}
