use crate::id::SessionId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: SessionId,
    pub name: Option<String>,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub turn_count: usize,
    pub start_offset: u64,
    pub end_offset: Option<u64>,
}

impl SessionMetadata {
    #[must_use]
    pub fn new(id: SessionId, start_offset: u64) -> Self {
        Self {
            id,
            name: None,
            created_at: Utc::now(),
            ended_at: None,
            turn_count: 0,
            start_offset,
            end_offset: None,
        }
    }

    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.ended_at.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_session_is_active() {
        let s = SessionMetadata::new(SessionId::new(), 0);
        assert!(s.is_active());
        assert_eq!(s.turn_count, 0);
    }

    #[test]
    fn json_roundtrip() {
        let s = SessionMetadata::new(SessionId::new(), 42);
        let json = serde_json::to_string(&s).unwrap();
        let back: SessionMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(back.start_offset, 42);
        assert!(back.is_active());
    }
}
