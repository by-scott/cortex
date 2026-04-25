use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::provenance::{SourceProvenance, SourceTrust};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    UserInput,
    AssistantOutput,
    RuntimePolicy,
    Goal,
    Memory,
    RetrievalEvidence,
    ToolSchema,
    ToolResult,
    Skill,
    TransportState,
    StatusFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Taint {
    Trusted,
    UserProvided,
    External,
    ToolOutput,
    Retrieved,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Budget {
    pub max_items: usize,
    pub max_input_tokens: usize,
    pub max_evidence_items: usize,
    pub max_tool_schemas: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            max_items: 64,
            max_input_tokens: 32_000,
            max_evidence_items: 12,
            max_tool_schemas: 64,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    pub content: String,
    pub owner_actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub provenance: SourceProvenance,
    pub taint: Taint,
    pub activation: f32,
    pub estimated_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding_group: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<DateTime<Utc>>,
    pub promoted_at: DateTime<Utc>,
    pub promotion_reason: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub budget: Budget,
    pub items: Vec<Item>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    ItemBudgetExceeded { max_items: usize },
    TokenBudgetExceeded { max_tokens: usize },
    EvidenceBudgetExceeded { max_evidence_items: usize },
    ToolSchemaBudgetExceeded { max_tool_schemas: usize },
    ActorMismatch { expected: String, actual: String },
}

impl Item {
    #[must_use]
    pub fn trusted(
        id: impl Into<String>,
        kind: ItemKind,
        content: impl Into<String>,
        owner_actor: impl Into<String>,
        reason: impl Into<String>,
    ) -> Self {
        let owner_actor = owner_actor.into();
        Self {
            id: id.into(),
            kind,
            content: content.into(),
            owner_actor: owner_actor.clone(),
            session_id: None,
            provenance: SourceProvenance::new(owner_actor, SourceTrust::Trusted),
            taint: Taint::Trusted,
            activation: 1.0,
            estimated_tokens: 0,
            evidence_ref: None,
            binding_group: None,
            expires_at: None,
            promoted_at: Utc::now(),
            promotion_reason: reason.into(),
        }
    }

    #[must_use]
    pub fn with_session(mut self, session_id: impl Into<String>) -> Self {
        self.session_id = Some(session_id.into());
        self
    }

    #[must_use]
    pub fn with_provenance(mut self, provenance: SourceProvenance, taint: Taint) -> Self {
        self.provenance = provenance;
        self.taint = taint;
        self
    }

    #[must_use]
    pub const fn with_activation(mut self, activation: f32) -> Self {
        self.activation = activation.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_token_estimate(mut self, estimated_tokens: usize) -> Self {
        self.estimated_tokens = estimated_tokens;
        self
    }

    #[must_use]
    pub fn with_evidence_ref(mut self, evidence_ref: impl Into<String>) -> Self {
        self.evidence_ref = Some(evidence_ref.into());
        self
    }
}

impl Frame {
    #[must_use]
    pub fn new(actor: impl Into<String>, session_id: Option<String>, budget: Budget) -> Self {
        Self {
            actor: actor.into(),
            session_id,
            created_at: Utc::now(),
            budget,
            items: Vec::new(),
        }
    }

    /// # Errors
    /// Returns `FrameError` when the candidate violates actor ownership or a
    /// configured frame budget.
    pub fn promote(&mut self, item: Item) -> Result<(), FrameError> {
        self.validate_candidate(&item)?;
        self.items.push(item);
        Ok(())
    }

    #[must_use]
    pub fn total_estimated_tokens(&self) -> usize {
        self.items.iter().map(|item| item.estimated_tokens).sum()
    }

    #[must_use]
    pub fn evidence_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.kind == ItemKind::RetrievalEvidence)
            .count()
    }

    #[must_use]
    pub fn tool_schema_count(&self) -> usize {
        self.items
            .iter()
            .filter(|item| item.kind == ItemKind::ToolSchema)
            .count()
    }

    /// # Errors
    /// Returns `FrameError` when `item` cannot be promoted into this frame.
    pub fn validate_candidate(&self, item: &Item) -> Result<(), FrameError> {
        if item.owner_actor != self.actor {
            return Err(FrameError::ActorMismatch {
                expected: self.actor.clone(),
                actual: item.owner_actor.clone(),
            });
        }
        if self.items.len() >= self.budget.max_items {
            return Err(FrameError::ItemBudgetExceeded {
                max_items: self.budget.max_items,
            });
        }
        let next_tokens = self
            .total_estimated_tokens()
            .saturating_add(item.estimated_tokens);
        if next_tokens > self.budget.max_input_tokens {
            return Err(FrameError::TokenBudgetExceeded {
                max_tokens: self.budget.max_input_tokens,
            });
        }
        if item.kind == ItemKind::RetrievalEvidence
            && self.evidence_count() >= self.budget.max_evidence_items
        {
            return Err(FrameError::EvidenceBudgetExceeded {
                max_evidence_items: self.budget.max_evidence_items,
            });
        }
        if item.kind == ItemKind::ToolSchema
            && self.tool_schema_count() >= self.budget.max_tool_schemas
        {
            return Err(FrameError::ToolSchemaBudgetExceeded {
                max_tool_schemas: self.budget.max_tool_schemas,
            });
        }
        Ok(())
    }
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ItemBudgetExceeded { max_items } => {
                write!(f, "workspace item budget exceeded: {max_items}")
            }
            Self::TokenBudgetExceeded { max_tokens } => {
                write!(f, "workspace token budget exceeded: {max_tokens}")
            }
            Self::EvidenceBudgetExceeded { max_evidence_items } => {
                write!(
                    f,
                    "workspace evidence budget exceeded: {max_evidence_items}"
                )
            }
            Self::ToolSchemaBudgetExceeded { max_tool_schemas } => {
                write!(
                    f,
                    "workspace tool schema budget exceeded: {max_tool_schemas}"
                )
            }
            Self::ActorMismatch { expected, actual } => {
                write!(
                    f,
                    "workspace actor mismatch: expected {expected}, got {actual}"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}
