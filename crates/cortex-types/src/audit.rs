use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditTimeRange {
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AuditSummary {
    pub total_events: usize,
    pub event_type_counts: HashMap<String, usize>,
    pub turn_count: usize,
    pub tool_call_count: usize,
    pub avg_confidence: f64,
    pub meta_alert_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPathStep {
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub description: String,
    pub confidence: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionPath {
    pub steps: Vec<DecisionPathStep>,
    pub outcome: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_default() {
        let s = AuditSummary::default();
        assert_eq!(s.total_events, 0);
        assert!(s.event_type_counts.is_empty());
    }

    #[test]
    fn serde_roundtrip() {
        let s = AuditSummary {
            total_events: 42,
            turn_count: 5,
            ..Default::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AuditSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_events, 42);
    }
}
