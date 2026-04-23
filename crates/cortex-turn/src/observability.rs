use std::collections::HashMap;

use cortex_kernel::StoredEvent;
use cortex_types::{AuditSummary, AuditTimeRange, DecisionPath, DecisionPathStep, Payload};
use serde::{Deserialize, Serialize};

// ── Alert Engine ────────────────────────────────────────────

/// Comparison operator for alert rule evaluation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    GreaterThan,
    LessThan,
}

/// A rule that fires when a named metric crosses a threshold.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub name: String,
    pub metric_name: String,
    pub threshold: f64,
    pub operator: Comparison,
}

impl AlertRule {
    #[must_use]
    pub fn new(
        name: impl Into<String>,
        metric_name: impl Into<String>,
        threshold: f64,
        operator: Comparison,
    ) -> Self {
        Self {
            name: name.into(),
            metric_name: metric_name.into(),
            threshold,
            operator,
        }
    }

    /// Check if this rule fires for a given metric value.
    #[must_use]
    pub fn fires(&self, value: f64) -> bool {
        match self.operator {
            Comparison::GreaterThan => value > self.threshold,
            Comparison::LessThan => value < self.threshold,
        }
    }
}

/// Engine that evaluates alert rules against current metrics.
pub struct AlertEngine;

impl AlertEngine {
    /// Evaluate rules against a metrics map. Returns `AlertFired` events for triggered rules.
    #[must_use]
    pub fn evaluate(rules: &[AlertRule], metrics: &HashMap<String, f64>) -> Vec<Payload> {
        rules
            .iter()
            .filter_map(|rule| {
                let value = metrics.get(&rule.metric_name)?;
                if rule.fires(*value) {
                    Some(Payload::AlertFired {
                        rule_name: rule.name.clone(),
                        metric_name: rule.metric_name.clone(),
                        threshold: rule.threshold,
                        current_value: *value,
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

/// Default set of alert rules for common operational concerns.
#[must_use]
pub fn default_rules() -> Vec<AlertRule> {
    vec![
        AlertRule::new(
            "low_confidence",
            "avg_confidence",
            0.3,
            Comparison::LessThan,
        ),
        AlertRule::new(
            "high_alert_rate",
            "meta_alert_ratio",
            0.5,
            Comparison::GreaterThan,
        ),
        AlertRule::new(
            "high_error_rate",
            "tool_error_ratio",
            0.3,
            Comparison::GreaterThan,
        ),
    ]
}

// ── Metrics Snapshot ────────────────────────────────────────

/// Aggregated operational metrics computed from journal events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSnapshot {
    /// Number of turns observed.
    pub turn_count: usize,
    /// Number of successful tool invocations.
    pub tool_success_count: usize,
    /// Number of failed tool invocations.
    pub tool_error_count: usize,
    /// Tool success rate (0.0..1.0). Zero if no tool invocations.
    pub tool_success_rate: f64,
    /// Average confidence score across all `ConfidenceAssessed` events.
    pub avg_confidence: f64,
    /// Number of `MemoryCaptured` events.
    pub memory_captures: usize,
    /// Number of `MemoryStabilized` events.
    pub memory_stabilizations: usize,
    /// Number of reasoning chains started.
    pub reasoning_chain_count: usize,
    /// Number of alerts fired.
    pub alerts_fired: usize,
    /// Number of metacognition impasse detections.
    pub impasse_count: usize,
}

impl MetricsSnapshot {
    /// A snapshot with all fields at zero/default values.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            turn_count: 0,
            tool_success_count: 0,
            tool_error_count: 0,
            tool_success_rate: 0.0,
            avg_confidence: 0.0,
            memory_captures: 0,
            memory_stabilizations: 0,
            reasoning_chain_count: 0,
            alerts_fired: 0,
            impasse_count: 0,
        }
    }
}

/// Aggregate structured metrics from a slice of stored journal events.
#[must_use]
pub fn aggregate_from_events(events: &[StoredEvent]) -> MetricsSnapshot {
    let mut snap = MetricsSnapshot::empty();
    let mut confidence_sum = 0.0_f64;
    let mut confidence_count = 0_u32;

    for event in events {
        match &event.payload {
            Payload::TurnStarted => snap.turn_count += 1,
            Payload::ToolInvocationResult { is_error, .. } => {
                if *is_error {
                    snap.tool_error_count += 1;
                } else {
                    snap.tool_success_count += 1;
                }
            }
            Payload::ConfidenceAssessed { score, .. } => {
                confidence_sum += score;
                confidence_count = confidence_count.saturating_add(1);
            }
            Payload::MemoryCaptured { .. } => snap.memory_captures += 1,
            Payload::MemoryStabilized { .. } => snap.memory_stabilizations += 1,
            Payload::ReasoningStarted { .. } => snap.reasoning_chain_count += 1,
            Payload::AlertFired { .. } => snap.alerts_fired += 1,
            Payload::ImpasseDetected { .. } => snap.impasse_count += 1,
            _ => {}
        }
    }

    let total_tool = snap.tool_success_count + snap.tool_error_count;
    let success_u32 = u32::try_from(snap.tool_success_count).unwrap_or(u32::MAX);
    let total_u32 = u32::try_from(total_tool).unwrap_or(u32::MAX);
    snap.tool_success_rate = if total_tool > 0 {
        f64::from(success_u32) / f64::from(total_u32)
    } else {
        0.0
    };

    snap.avg_confidence = if confidence_count > 0 {
        confidence_sum / f64::from(confidence_count)
    } else {
        0.0
    };

    snap
}

// ── Audit Aggregator ────────────────────────────────────────

/// Aggregator for audit dashboard queries over stored events.
pub struct AuditAggregator;

impl AuditAggregator {
    /// Summarize a slice of events into aggregate metrics.
    #[must_use]
    pub fn summarize(events: &[StoredEvent]) -> AuditSummary {
        let total_events = events.len();
        let mut event_type_counts: HashMap<String, usize> = HashMap::new();
        let mut turn_count = 0_usize;
        let mut tool_call_count = 0_usize;
        let mut confidence_sum = 0.0_f64;
        let mut confidence_count = 0_u32;
        let mut meta_alert_count = 0_usize;

        for event in events {
            *event_type_counts
                .entry(event.event_type.clone())
                .or_insert(0) += 1;

            match &event.payload {
                Payload::TurnStarted => turn_count += 1,
                Payload::ToolInvocationResult { .. } => tool_call_count += 1,
                Payload::ConfidenceAssessed { score, .. } => {
                    confidence_sum += score;
                    confidence_count = confidence_count.saturating_add(1);
                }
                Payload::ImpasseDetected { .. } => meta_alert_count += 1,
                _ => {}
            }
        }

        let avg_confidence = if confidence_count > 0 {
            confidence_sum / f64::from(confidence_count)
        } else {
            0.0
        };

        AuditSummary {
            total_events,
            event_type_counts,
            turn_count,
            tool_call_count,
            avg_confidence,
            meta_alert_count,
        }
    }

    /// Extract a decision path from events filtered by `correlation_id`.
    #[must_use]
    pub fn extract_decision_path(events: &[StoredEvent], correlation_id: &str) -> DecisionPath {
        let mut steps: Vec<DecisionPathStep> = events
            .iter()
            .filter(|e| e.correlation_id == correlation_id)
            .map(|e| {
                let confidence = match &e.payload {
                    Payload::ConfidenceAssessed { score, .. } => Some(*score),
                    _ => None,
                };

                DecisionPathStep {
                    timestamp: e.timestamp,
                    event_type: e.event_type.clone(),
                    description: e.event_type.clone(),
                    confidence,
                }
            })
            .collect();

        steps.sort_by_key(|s| s.timestamp);

        let outcome = steps
            .last()
            .map(|s| s.event_type.clone())
            .unwrap_or_default();

        DecisionPath { steps, outcome }
    }

    /// Filter events by a time range (inclusive).
    #[must_use]
    pub fn filter_by_time_range<'a>(
        events: &'a [StoredEvent],
        range: &AuditTimeRange,
    ) -> Vec<&'a StoredEvent> {
        events
            .iter()
            .filter(|e| e.timestamp >= range.start && e.timestamp <= range.end)
            .collect()
    }
}
