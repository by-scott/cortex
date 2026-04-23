use crate::id::SessionId;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    pub id: SessionId,
    pub name: Option<String>,
    #[serde(default = "default_owner_actor")]
    pub owner_actor: String,
    pub created_at: DateTime<Utc>,
    pub ended_at: Option<DateTime<Utc>>,
    pub turn_count: usize,
    pub start_offset: u64,
    pub end_offset: Option<u64>,
}

fn default_owner_actor() -> String {
    "local:default".to_string()
}

impl SessionMetadata {
    #[must_use]
    pub fn new(id: SessionId, start_offset: u64) -> Self {
        Self {
            id,
            name: None,
            owner_actor: default_owner_actor(),
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
