use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{OwnedScope, SideEffectId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectKind {
    ModelCall,
    ToolCall,
    EmbeddingCall,
    DeliverySend,
    ExternalIo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SideEffectStatus {
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffectIntent {
    pub id: SideEffectId,
    pub scope: OwnedScope,
    pub kind: SideEffectKind,
    pub idempotency_key: String,
    pub summary: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffectRecord {
    pub intent_id: SideEffectId,
    pub scope: OwnedScope,
    pub status: SideEffectStatus,
    pub output_digest: Option<String>,
    pub error: Option<String>,
    pub completed_at: DateTime<Utc>,
}

impl SideEffectIntent {
    #[must_use]
    pub fn new(
        scope: OwnedScope,
        kind: SideEffectKind,
        idempotency_key: impl Into<String>,
        summary: impl Into<String>,
    ) -> Self {
        Self::new_at(scope, kind, idempotency_key, summary, Utc::now())
    }

    #[must_use]
    pub fn new_at(
        scope: OwnedScope,
        kind: SideEffectKind,
        idempotency_key: impl Into<String>,
        summary: impl Into<String>,
        created_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: SideEffectId::new(),
            scope,
            kind,
            idempotency_key: idempotency_key.into(),
            summary: summary.into(),
            created_at,
        }
    }
}

impl SideEffectRecord {
    #[must_use]
    pub fn succeeded(
        intent_id: SideEffectId,
        scope: OwnedScope,
        output_digest: impl Into<String>,
        completed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            intent_id,
            scope,
            status: SideEffectStatus::Succeeded,
            output_digest: Some(output_digest.into()),
            error: None,
            completed_at,
        }
    }

    #[must_use]
    pub fn failed(
        intent_id: SideEffectId,
        scope: OwnedScope,
        error: impl Into<String>,
        completed_at: DateTime<Utc>,
    ) -> Self {
        Self {
            intent_id,
            scope,
            status: SideEffectStatus::Failed,
            output_digest: None,
            error: Some(error.into()),
            completed_at,
        }
    }
}
