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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(1_000_000);

    fn make_event(payload: Payload) -> StoredEvent {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        StoredEvent {
            offset: id,
            event_id: format!("evt-{id}"),
            turn_id: "turn-1".into(),
            correlation_id: "corr-1".into(),
            timestamp: Utc::now(),
            event_type: format!("{payload:?}")
                .split('{')
                .next()
                .unwrap_or("")
                .trim()
                .to_string(),
            payload,
            execution_version: String::new(),
        }
    }

    fn make_stored_event(
        event_type: &str,
        correlation_id: &str,
        payload: Payload,
        timestamp: chrono::DateTime<Utc>,
    ) -> StoredEvent {
        let id = COUNTER.fetch_add(1, Ordering::Relaxed);
        StoredEvent {
            offset: id,
            event_id: format!("evt-{id}"),
            turn_id: "turn-1".into(),
            correlation_id: correlation_id.into(),
            timestamp,
            event_type: event_type.into(),
            payload,
            execution_version: String::new(),
        }
    }

    // ── AlertEngine tests ───────────────────────────────────

    #[test]
    fn rule_fires_greater_than() {
        let rule = AlertRule::new("test", "metric", 0.5, Comparison::GreaterThan);
        assert!(rule.fires(0.6));
        assert!(!rule.fires(0.5));
        assert!(!rule.fires(0.4));
    }

    #[test]
    fn rule_fires_less_than() {
        let rule = AlertRule::new("test", "metric", 0.3, Comparison::LessThan);
        assert!(rule.fires(0.2));
        assert!(!rule.fires(0.3));
        assert!(!rule.fires(0.4));
    }

    #[test]
    fn evaluate_fires_matching_rules() {
        let rules = vec![
            AlertRule::new("low_conf", "confidence", 0.3, Comparison::LessThan),
            AlertRule::new("high_turns", "turn_count", 100.0, Comparison::GreaterThan),
        ];
        let mut metrics = HashMap::new();
        metrics.insert("confidence".to_string(), 0.2);
        metrics.insert("turn_count".to_string(), 50.0);

        let events = AlertEngine::evaluate(&rules, &metrics);
        assert_eq!(events.len(), 1);
        match &events[0] {
            Payload::AlertFired {
                rule_name,
                current_value,
                ..
            } => {
                assert_eq!(rule_name, "low_conf");
                assert!((current_value - 0.2).abs() < f64::EPSILON);
            }
            _ => panic!("expected AlertFired"),
        }
    }

    #[test]
    fn evaluate_no_fires_when_all_ok() {
        let rules = vec![AlertRule::new(
            "test",
            "metric",
            0.5,
            Comparison::GreaterThan,
        )];
        let mut metrics = HashMap::new();
        metrics.insert("metric".to_string(), 0.3);

        let events = AlertEngine::evaluate(&rules, &metrics);
        assert!(events.is_empty());
    }

    #[test]
    fn evaluate_missing_metric_skipped() {
        let rules = vec![AlertRule::new(
            "test",
            "missing",
            0.5,
            Comparison::GreaterThan,
        )];
        let metrics = HashMap::new();

        let events = AlertEngine::evaluate(&rules, &metrics);
        assert!(events.is_empty());
    }

    #[test]
    fn default_rules_non_empty() {
        let rules = default_rules();
        assert_eq!(rules.len(), 3);
    }

    // ── MetricsSnapshot tests ───────────────────────────────

    #[test]
    fn aggregate_empty_events() {
        let snap = aggregate_from_events(&[]);
        assert_eq!(snap.turn_count, 0);
        assert!(snap.tool_success_rate.abs() < f64::EPSILON);
        assert!(snap.avg_confidence.abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_tool_success_rate() {
        let mut events = Vec::new();
        for _ in 0..8 {
            events.push(make_event(Payload::ToolInvocationResult {
                tool_name: "read".into(),
                output: "ok".into(),
                is_error: false,
            }));
        }
        for _ in 0..2 {
            events.push(make_event(Payload::ToolInvocationResult {
                tool_name: "bash".into(),
                output: "err".into(),
                is_error: true,
            }));
        }
        let snap = aggregate_from_events(&events);
        assert_eq!(snap.tool_success_count, 8);
        assert_eq!(snap.tool_error_count, 2);
        assert!((snap.tool_success_rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn aggregate_avg_confidence() {
        let events = vec![
            make_event(Payload::ConfidenceAssessed {
                level: "high".into(),
                score: 0.6,
                evidence_count: 1,
            }),
            make_event(Payload::ConfidenceAssessed {
                level: "high".into(),
                score: 0.8,
                evidence_count: 2,
            }),
            make_event(Payload::ConfidenceAssessed {
                level: "high".into(),
                score: 0.9,
                evidence_count: 3,
            }),
        ];
        let snap = aggregate_from_events(&events);
        let expected = (0.6 + 0.8 + 0.9) / 3.0;
        assert!((snap.avg_confidence - expected).abs() < 0.001);
    }

    #[test]
    fn aggregate_memory_ops() {
        let events = vec![
            make_event(Payload::MemoryCaptured {
                memory_id: "m1".into(),
                memory_type: "Episodic".into(),
            }),
            make_event(Payload::MemoryCaptured {
                memory_id: "m2".into(),
                memory_type: "Semantic".into(),
            }),
            make_event(Payload::MemoryCaptured {
                memory_id: "m3".into(),
                memory_type: "Episodic".into(),
            }),
            make_event(Payload::MemoryStabilized {
                memory_id: "m1".into(),
            }),
            make_event(Payload::MemoryStabilized {
                memory_id: "m2".into(),
            }),
        ];
        let snap = aggregate_from_events(&events);
        assert_eq!(snap.memory_captures, 3);
        assert_eq!(snap.memory_stabilizations, 2);
    }

    #[test]
    fn aggregate_mixed_events() {
        let events = vec![
            make_event(Payload::TurnStarted),
            make_event(Payload::TurnStarted),
            make_event(Payload::ReasoningStarted {
                mode: "CoT".into(),
                input_summary: "test".into(),
            }),
            make_event(Payload::AlertFired {
                rule_name: "test".into(),
                metric_name: "m".into(),
                threshold: 0.5,
                current_value: 0.6,
            }),
            make_event(Payload::ImpasseDetected {
                detector: "doom_loop".into(),
                details: "stuck".into(),
            }),
        ];
        let snap = aggregate_from_events(&events);
        assert_eq!(snap.turn_count, 2);
        assert_eq!(snap.reasoning_chain_count, 1);
        assert_eq!(snap.alerts_fired, 1);
        assert_eq!(snap.impasse_count, 1);
    }

    #[test]
    fn metrics_snapshot_empty_is_zeroed() {
        let snap = MetricsSnapshot::empty();
        assert_eq!(snap.turn_count, 0);
        assert_eq!(snap.tool_success_count, 0);
        assert_eq!(snap.tool_error_count, 0);
        assert!(snap.tool_success_rate.abs() < f64::EPSILON);
        assert!(snap.avg_confidence.abs() < f64::EPSILON);
        assert_eq!(snap.memory_captures, 0);
        assert_eq!(snap.memory_stabilizations, 0);
        assert_eq!(snap.reasoning_chain_count, 0);
        assert_eq!(snap.alerts_fired, 0);
        assert_eq!(snap.impasse_count, 0);
    }

    // ── AuditAggregator tests ───────────────────────────────

    #[test]
    fn summarize_mixed_events() {
        let now = Utc::now();
        let events = vec![
            make_stored_event("TurnStarted", "c1", Payload::TurnStarted, now),
            make_stored_event(
                "ToolInvocationResult",
                "c1",
                Payload::ToolInvocationResult {
                    tool_name: "read".into(),
                    output: "ok".into(),
                    is_error: false,
                },
                now,
            ),
            make_stored_event(
                "ConfidenceAssessed",
                "c1",
                Payload::ConfidenceAssessed {
                    level: "high".into(),
                    score: 0.9,
                    evidence_count: 3,
                },
                now,
            ),
            make_stored_event(
                "ImpasseDetected",
                "c1",
                Payload::ImpasseDetected {
                    detector: "doom_loop".into(),
                    details: "stuck".into(),
                },
                now,
            ),
        ];

        let summary = AuditAggregator::summarize(&events);
        assert_eq!(summary.total_events, 4);
        assert_eq!(summary.turn_count, 1);
        assert_eq!(summary.tool_call_count, 1);
        assert!((summary.avg_confidence - 0.9).abs() < f64::EPSILON);
        assert_eq!(summary.meta_alert_count, 1);
    }

    #[test]
    fn summarize_empty() {
        let summary = AuditAggregator::summarize(&[]);
        assert_eq!(summary.total_events, 0);
        assert!(summary.avg_confidence.abs() < f64::EPSILON);
    }

    #[test]
    fn extract_decision_path_by_correlation() {
        let now = Utc::now();
        let events = vec![
            make_stored_event("TurnStarted", "c1", Payload::TurnStarted, now),
            make_stored_event("TurnStarted", "c2", Payload::TurnStarted, now),
            make_stored_event(
                "ConfidenceAssessed",
                "c1",
                Payload::ConfidenceAssessed {
                    level: "high".into(),
                    score: 0.85,
                    evidence_count: 2,
                },
                now,
            ),
            make_stored_event("TurnCompleted", "c1", Payload::TurnCompleted, now),
        ];

        let path = AuditAggregator::extract_decision_path(&events, "c1");
        assert_eq!(path.steps.len(), 3);
        assert_eq!(path.outcome, "TurnCompleted");
        let conf_step = path
            .steps
            .iter()
            .find(|s| s.event_type == "ConfidenceAssessed")
            .unwrap();
        assert_eq!(conf_step.confidence, Some(0.85));
    }

    #[test]
    fn filter_by_time_range() {
        let t1 = Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap();
        let t2 = Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap();
        let t3 = Utc.with_ymd_and_hms(2026, 1, 3, 0, 0, 0).unwrap();

        let events = vec![
            make_stored_event("TurnStarted", "c1", Payload::TurnStarted, t1),
            make_stored_event("TurnStarted", "c1", Payload::TurnStarted, t2),
            make_stored_event("TurnStarted", "c1", Payload::TurnStarted, t3),
        ];

        let range = AuditTimeRange { start: t1, end: t2 };
        let filtered = AuditAggregator::filter_by_time_range(&events, &range);
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn extract_decision_path_empty() {
        let path = AuditAggregator::extract_decision_path(&[], "c1");
        assert!(path.steps.is_empty());
        assert!(path.outcome.is_empty());
    }
}
