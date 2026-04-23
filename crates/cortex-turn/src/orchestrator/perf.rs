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
