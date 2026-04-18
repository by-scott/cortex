use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WorkingMemoryItem {
    pub tag: String,
    pub relevance: f64,
    pub activated_at: DateTime<Utc>,
    pub last_rehearsed: DateTime<Utc>,
}

impl WorkingMemoryItem {
    #[must_use]
    pub fn new(tag: impl Into<String>, relevance: f64) -> Self {
        let now = Utc::now();
        Self {
            tag: tag.into(),
            relevance: relevance.clamp(0.0, 1.0),
            activated_at: now,
            last_rehearsed: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relevance_clamped() {
        let item = WorkingMemoryItem::new("test", 1.5);
        assert!((item.relevance - 1.0).abs() < f64::EPSILON);

        let item2 = WorkingMemoryItem::new("test", -0.5);
        assert!(item2.relevance.abs() < f64::EPSILON);
    }
}
