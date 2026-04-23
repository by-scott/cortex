use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub content: String,
    pub description: String,
    pub memory_type: MemoryType,
    pub kind: MemoryKind,
    pub status: MemoryStatus,
    pub strength: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub access_count: u32,
    #[serde(default = "default_memory_owner_actor")]
    pub owner_actor: String,
    #[serde(default)]
    pub instance_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reconsolidation_until: Option<DateTime<Utc>>,
    #[serde(default)]
    pub source: MemorySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryType {
    #[serde(alias = "user")]
    User,
    #[serde(alias = "feedback")]
    Feedback,
    #[serde(alias = "project")]
    Project,
    #[serde(alias = "reference")]
    Reference,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryKind {
    Episodic,
    Semantic,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MemorySource {
    UserInput,
    ToolOutput,
    #[default]
    LlmGenerated,
    Network,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TrustLevel {
    Untrusted,
    Verified,
    Trusted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemoryStatus {
    Captured,
    Materialized,
    Stabilized,
    Deprecated,
}

fn default_memory_owner_actor() -> String {
    "local:default".to_string()
}

#[derive(Debug, Clone)]
pub struct MemoryStatusError {
    pub from: MemoryStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRelation {
    pub source_id: String,
    pub target_id: String,
    pub relation_type: String,
    pub metadata: Option<String>,
}

impl MemoryEntry {
    #[must_use]
    pub fn new(
        content: impl Into<String>,
        description: impl Into<String>,
        memory_type: MemoryType,
        kind: MemoryKind,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            content: content.into(),
            description: description.into(),
            memory_type,
            kind,
            status: MemoryStatus::Captured,
            strength: 1.0,
            created_at: now,
            updated_at: now,
            access_count: 0,
            owner_actor: default_memory_owner_actor(),
            instance_id: String::new(),
            reconsolidation_until: None,
            source: MemorySource::LlmGenerated,
        }
    }
}

impl fmt::Display for MemoryType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::User => f.write_str("user"),
            Self::Feedback => f.write_str("feedback"),
            Self::Project => f.write_str("project"),
            Self::Reference => f.write_str("ref"),
        }
    }
}

impl MemorySource {
    #[must_use]
    pub const fn trust_level(self) -> TrustLevel {
        match self {
            Self::UserInput => TrustLevel::Trusted,
            Self::ToolOutput | Self::LlmGenerated => TrustLevel::Verified,
            Self::Network => TrustLevel::Untrusted,
        }
    }
}

impl MemoryStatus {
    /// # Errors
    /// Returns `MemoryStatusError` if the status cannot advance (terminal states).
    pub const fn try_advance(self) -> Result<Self, MemoryStatusError> {
        match self {
            Self::Captured => Ok(Self::Materialized),
            Self::Materialized => Ok(Self::Stabilized),
            Self::Stabilized | Self::Deprecated => Err(MemoryStatusError { from: self }),
        }
    }

    #[must_use]
    pub const fn deprecate(self) -> Self {
        Self::Deprecated
    }
}

impl fmt::Display for MemoryStatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "cannot advance memory status from {:?}", self.from)
    }
}

impl std::error::Error for MemoryStatusError {}

impl MemoryRelation {
    #[must_use]
    pub fn new(
        source_id: impl Into<String>,
        target_id: impl Into<String>,
        relation_type: impl Into<String>,
    ) -> Self {
        Self {
            source_id: source_id.into(),
            target_id: target_id.into(),
            relation_type: relation_type.into(),
            metadata: None,
        }
    }

    #[must_use]
    pub fn with_metadata(mut self, metadata: impl Into<String>) -> Self {
        self.metadata = Some(metadata.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_advance() {
        assert_eq!(
            MemoryStatus::Captured.try_advance().unwrap(),
            MemoryStatus::Materialized
        );
        assert_eq!(
            MemoryStatus::Materialized.try_advance().unwrap(),
            MemoryStatus::Stabilized
        );
        assert!(MemoryStatus::Stabilized.try_advance().is_err());
        assert!(MemoryStatus::Deprecated.try_advance().is_err());
    }

    #[test]
    fn deprecate_from_any() {
        assert_eq!(MemoryStatus::Captured.deprecate(), MemoryStatus::Deprecated);
        assert_eq!(
            MemoryStatus::Stabilized.deprecate(),
            MemoryStatus::Deprecated
        );
    }

    #[test]
    fn trust_ordering() {
        assert!(TrustLevel::Trusted > TrustLevel::Verified);
        assert!(TrustLevel::Verified > TrustLevel::Untrusted);
    }

    #[test]
    fn entry_json_roundtrip() {
        let e = MemoryEntry::new("content", "desc", MemoryType::User, MemoryKind::Episodic);
        let json = serde_json::to_string(&e).unwrap();
        let back: MemoryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.content, "content");
        assert_eq!(back.status, MemoryStatus::Captured);
    }
}
