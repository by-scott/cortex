//! Long-running stability monitoring infrastructure.
//!
//! Provides resource snapshot collection, leak detection via linear regression,
//! and stability report generation for 48h+ daemon operation validation.

use std::collections::HashMap;
use std::time::Instant;

use serde::Serialize;

/// A single point-in-time resource measurement.
#[derive(Debug, Clone)]
pub struct ResourceSnapshot {
    /// Elapsed seconds since monitor start.
    pub elapsed_secs: f64,
    /// Approximate heap memory usage in bytes.
    pub heap_bytes: u64,
    /// Total journal event count.
    pub event_count: u64,
    /// Active session count.
    pub session_count: u32,
}

/// Collects resource snapshots over time for stability analysis.
pub struct StabilityMonitor {
    start: Instant,
    snapshots: Vec<ResourceSnapshot>,
}

impl Default for StabilityMonitor {
    fn default() -> Self {
        Self::new()
    }
}

impl StabilityMonitor {
    /// Create a new monitor, recording the start time.
    #[must_use]
    pub fn new() -> Self {
        Self {
            start: Instant::now(),
            snapshots: Vec::new(),
        }
    }

    /// Record a resource snapshot at the current time.
    pub fn record_snapshot(&mut self, heap_bytes: u64, event_count: u64, session_count: u32) {
        let elapsed_secs = self.start.elapsed().as_secs_f64();
        self.snapshots.push(ResourceSnapshot {
            elapsed_secs,
            heap_bytes,
            event_count,
            session_count,
        });
    }

    /// Record a snapshot with an explicit elapsed time (for testing).
    pub fn record_snapshot_at(
        &mut self,
        elapsed_secs: f64,
        heap_bytes: u64,
        event_count: u64,
        session_count: u32,
    ) {
        self.snapshots.push(ResourceSnapshot {
            elapsed_secs,
            heap_bytes,
            event_count,
            session_count,
        });
    }

    /// Number of collected snapshots.
    #[must_use]
    pub const fn sample_count(&self) -> usize {
        self.snapshots.len()
    }

    /// Generate a stability report analyzing all collected snapshots.
    #[must_use]
    pub fn generate_report(&self) -> StabilityReport {
        let sample_count = self.snapshots.len();
        let uptime_secs = self.start.elapsed().as_secs_f64();

        if sample_count < 2 {
            return StabilityReport {
                is_stable: true,
                growth_rates: HashMap::new(),
                sample_count,
                uptime_secs,
            };
        }

        let times: Vec<f64> = self.snapshots.iter().map(|s| s.elapsed_secs).collect();
        let heap_values: Vec<f64> = self
            .snapshots
            .iter()
            .map(|s| {
                let v = u32::try_from(s.heap_bytes).unwrap_or(u32::MAX);
                f64::from(v)
            })
            .collect();
        let event_values: Vec<f64> = self
            .snapshots
            .iter()
            .map(|s| {
                let v = u32::try_from(s.event_count).unwrap_or(u32::MAX);
                f64::from(v)
            })
            .collect();
        let session_values: Vec<f64> = self
            .snapshots
            .iter()
            .map(|s| f64::from(s.session_count))
            .collect();

        let heap_trend = detect_trend(&times, &heap_values);
        let event_trend = detect_trend(&times, &event_values);
        let session_trend = detect_trend(&times, &session_values);

        let mut growth_rates = HashMap::new();
        growth_rates.insert("heap_bytes".to_string(), heap_trend.slope);
        growth_rates.insert("event_count".to_string(), event_trend.slope);
        growth_rates.insert("session_count".to_string(), session_trend.slope);

        // System is considered unstable if heap grows > 1 KB/s or sessions leak
        let is_stable = heap_trend.slope < 1024.0 && session_trend.slope < 0.01;

        StabilityReport {
            is_stable,
            growth_rates,
            sample_count,
            uptime_secs,
        }
    }
}

/// Result of linear regression trend detection.
#[derive(Debug, Clone)]
pub struct TrendResult {
    /// Slope of the linear regression (units per second).
    pub slope: f64,
    /// Whether the trend shows sustained growth (slope > threshold).
    pub is_growing: bool,
}

/// Detect a linear trend in time-series data using simple linear regression.
///
/// `times` and `values` must have the same length and at least 2 elements.
/// Returns slope in units-per-second and whether growth is detected.
#[must_use]
pub fn detect_trend(times: &[f64], values: &[f64]) -> TrendResult {
    let n = times.len();
    if n < 2 || n != values.len() {
        return TrendResult {
            slope: 0.0,
            is_growing: false,
        };
    }

    let n_f64 = f64::from(u32::try_from(n).unwrap_or(u32::MAX));
    let total_x: f64 = times.iter().sum();
    let total_y: f64 = values.iter().sum();
    let cross_product: f64 = times.iter().zip(values.iter()).map(|(x, y)| x * y).sum();
    let sq_sum_x: f64 = times.iter().map(|x| x * x).sum();

    let denominator = n_f64.mul_add(sq_sum_x, -(total_x * total_x));
    let slope = if denominator.abs() > f64::EPSILON {
        n_f64.mul_add(cross_product, -(total_x * total_y)) / denominator
    } else {
        0.0
    };

    // Consider "growing" if slope > 0.1 units/sec (adjustable threshold)
    let is_growing = slope > 0.1;

    TrendResult { slope, is_growing }
}

/// Serializable stability report for 48h+ daemon operation validation.
#[derive(Debug, Serialize)]
pub struct StabilityReport {
    /// Whether the system is considered stable (no resource leaks).
    pub is_stable: bool,
    /// Per-metric growth rates (units per second).
    pub growth_rates: HashMap<String, f64>,
    /// Number of snapshots collected.
    pub sample_count: usize,
    /// Total uptime in seconds.
    pub uptime_secs: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_trend_increasing() {
        let times = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let values = vec![100.0, 200.0, 300.0, 400.0, 500.0];
        let result = detect_trend(&times, &values);
        assert!(result.is_growing);
        assert!((result.slope - 100.0).abs() < 0.01);
    }

    #[test]
    fn detect_trend_stable() {
        let times = vec![0.0, 1.0, 2.0, 3.0, 4.0];
        let values = vec![100.0, 102.0, 99.0, 101.0, 100.0];
        let result = detect_trend(&times, &values);
        assert!(!result.is_growing);
        assert!(result.slope.abs() < 1.0);
    }

    #[test]
    fn detect_trend_insufficient_data() {
        let result = detect_trend(&[1.0], &[100.0]);
        assert!(!result.is_growing);
        assert!(result.slope.abs() < f64::EPSILON);
    }

    #[test]
    fn detect_trend_decreasing() {
        let times = vec![0.0, 1.0, 2.0, 3.0];
        let values = vec![400.0, 300.0, 200.0, 100.0];
        let result = detect_trend(&times, &values);
        assert!(!result.is_growing);
        assert!(result.slope < 0.0);
    }

    #[test]
    fn stability_monitor_record_and_report() {
        let mut monitor = StabilityMonitor::new();
        // Simulate stable system
        monitor.record_snapshot_at(0.0, 1_000_000, 100, 2);
        monitor.record_snapshot_at(1.0, 1_000_100, 110, 2);
        monitor.record_snapshot_at(2.0, 1_000_050, 120, 2);
        monitor.record_snapshot_at(3.0, 1_000_200, 130, 2);

        let report = monitor.generate_report();
        assert!(report.is_stable);
        assert_eq!(report.sample_count, 4);
        assert!(report.growth_rates.contains_key("heap_bytes"));
    }

    #[test]
    fn stability_monitor_detects_leak() {
        let mut monitor = StabilityMonitor::new();
        // Simulate memory leak: 10MB/sec growth
        for i in 0_u32..10 {
            let i_f64 = f64::from(i);
            let heap = 1_000_000_u64 + u64::from(i) * 10_000_000;
            monitor.record_snapshot_at(i_f64, heap, 100, 1);
        }

        let report = monitor.generate_report();
        assert!(!report.is_stable);
        let heap_rate = report
            .growth_rates
            .get("heap_bytes")
            .copied()
            .unwrap_or(0.0);
        assert!(
            heap_rate > 1024.0,
            "heap growth rate should exceed 1KB/s threshold"
        );
    }

    #[test]
    fn stability_report_serializes() {
        let mut monitor = StabilityMonitor::new();
        monitor.record_snapshot_at(0.0, 100, 10, 1);
        monitor.record_snapshot_at(1.0, 100, 20, 1);
        let report = monitor.generate_report();
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("is_stable"));
        assert!(json.contains("growth_rates"));
        assert!(json.contains("sample_count"));
    }

    #[test]
    fn stability_monitor_empty() {
        let monitor = StabilityMonitor::new();
        let report = monitor.generate_report();
        assert!(report.is_stable);
        assert_eq!(report.sample_count, 0);
    }

    #[test]
    fn stability_monitor_single_sample() {
        let mut monitor = StabilityMonitor::new();
        monitor.record_snapshot_at(0.0, 100, 10, 1);
        let report = monitor.generate_report();
        assert!(report.is_stable);
        assert_eq!(report.sample_count, 1);
    }

    #[test]
    fn sample_count_increments() {
        let mut monitor = StabilityMonitor::new();
        assert_eq!(monitor.sample_count(), 0);
        monitor.record_snapshot_at(0.0, 100, 10, 1);
        assert_eq!(monitor.sample_count(), 1);
        monitor.record_snapshot_at(1.0, 200, 20, 2);
        assert_eq!(monitor.sample_count(), 2);
    }
}
