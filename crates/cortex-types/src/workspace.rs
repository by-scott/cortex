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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    User,
    Assistant,
    Policy,
    Goal,
    Memory,
    Evidence,
    Tool,
    Skill,
    Transport,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Volatility {
    Stable,
    Session,
    Turn,
    #[default]
    Ephemeral,
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

impl Default for AdmissionPolicy {
    fn default() -> Self {
        Self {
            min_marginal_utility: 0.05,
            allow_eviction: true,
        }
    }
}

impl Lane {
    #[must_use]
    pub const fn for_kind(kind: ItemKind) -> Self {
        match kind {
            ItemKind::UserInput => Self::User,
            ItemKind::AssistantOutput => Self::Assistant,
            ItemKind::RuntimePolicy => Self::Policy,
            ItemKind::Goal => Self::Goal,
            ItemKind::Memory => Self::Memory,
            ItemKind::RetrievalEvidence => Self::Evidence,
            ItemKind::ToolSchema | ItemKind::ToolResult => Self::Tool,
            ItemKind::Skill => Self::Skill,
            ItemKind::TransportState => Self::Transport,
            ItemKind::StatusFact => Self::Status,
        }
    }

    #[must_use]
    pub const fn is_protected(self) -> bool {
        matches!(self, Self::Policy | Self::User | Self::Transport)
    }
}

impl Taint {
    #[must_use]
    pub const fn allowed_in_lane(self, lane: Lane) -> bool {
        match lane {
            Lane::Policy | Lane::Skill => matches!(self, Self::Trusted),
            Lane::Tool => matches!(self, Self::Trusted | Self::ToolOutput),
            Lane::Memory => matches!(self, Self::Trusted | Self::UserProvided),
            Lane::Evidence => true,
            Lane::User => matches!(self, Self::Trusted | Self::UserProvided),
            Lane::Assistant | Lane::Goal | Lane::Transport | Lane::Status => {
                !matches!(self, Self::External | Self::Retrieved)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub kind: ItemKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lane: Option<Lane>,
    pub content: String,
    pub owner_actor: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    pub provenance: SourceProvenance,
    pub taint: Taint,
    pub activation: f32,
    #[serde(default)]
    pub utility: f32,
    #[serde(default)]
    pub risk: f32,
    #[serde(default)]
    pub volatility: Volatility,
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

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AdmissionPolicy {
    pub min_marginal_utility: f32,
    pub allow_eviction: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdmissionDisposition {
    Admitted,
    Rejected,
    AdmittedAfterEviction,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EvictionRecord {
    pub item_id: String,
    pub reason: String,
    pub marginal_utility: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdmissionOutcome {
    pub disposition: AdmissionDisposition,
    pub item_id: String,
    pub marginal_utility: f32,
    pub reason: String,
    pub evicted: Vec<EvictionRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    ItemBudgetExceeded {
        max_items: usize,
    },
    TokenBudgetExceeded {
        max_tokens: usize,
    },
    EvidenceBudgetExceeded {
        max_evidence_items: usize,
    },
    ToolSchemaBudgetExceeded {
        max_tool_schemas: usize,
    },
    ActorMismatch {
        expected: String,
        actual: String,
    },
    ContaminationBarrier {
        item_id: String,
        lane: Lane,
        taint: Taint,
    },
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
            lane: Some(Lane::for_kind(kind)),
            content: content.into(),
            owner_actor: owner_actor.clone(),
            session_id: None,
            provenance: SourceProvenance::new(owner_actor, SourceTrust::Trusted),
            taint: Taint::Trusted,
            activation: 1.0,
            utility: 1.0,
            risk: 0.0,
            volatility: Volatility::Session,
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
    pub const fn with_lane(mut self, lane: Lane) -> Self {
        self.lane = Some(lane);
        self
    }

    #[must_use]
    pub const fn with_utility(mut self, utility: f32) -> Self {
        self.utility = utility.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_risk(mut self, risk: f32) -> Self {
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_volatility(mut self, volatility: Volatility) -> Self {
        self.volatility = volatility;
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

    #[must_use]
    pub fn effective_lane(&self) -> Lane {
        self.lane.unwrap_or_else(|| Lane::for_kind(self.kind))
    }

    #[must_use]
    pub fn marginal_utility(&self, budget: &Budget) -> f32 {
        let base = self.utility.max(self.activation);
        let token_pressure = token_pressure(self.estimated_tokens, budget.max_input_tokens);
        let taint_penalty = taint_penalty(self.taint);
        let volatility_penalty = volatility_penalty(self.volatility);
        let risk_and_taint = self.risk.mul_add(0.25, taint_penalty);
        let penalty = token_pressure.mul_add(0.30, risk_and_taint);
        (base - penalty - volatility_penalty).clamp(0.0, 1.0)
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

    /// # Errors
    /// Returns `FrameError` when actor ownership or contamination barriers fail.
    pub fn admit(&mut self, item: Item) -> Result<AdmissionOutcome, FrameError> {
        self.admit_with_policy(item, AdmissionPolicy::default())
    }

    /// # Errors
    /// Returns `FrameError` when actor ownership or contamination barriers fail.
    pub fn admit_with_policy(
        &mut self,
        item: Item,
        policy: AdmissionPolicy,
    ) -> Result<AdmissionOutcome, FrameError> {
        self.validate_scope_and_lane(&item)?;
        let marginal_utility = item.marginal_utility(&self.budget);
        if marginal_utility < policy.min_marginal_utility {
            return Ok(admission_rejected(
                &item.id,
                marginal_utility,
                "candidate marginal utility below admission threshold",
            ));
        }
        if self.budget_accepts(&item, &[]).is_ok() {
            let outcome = admission_accepted(&item.id, marginal_utility, Vec::new());
            self.items.push(item);
            return Ok(outcome);
        }
        if !policy.allow_eviction {
            return Ok(admission_rejected(
                &item.id,
                marginal_utility,
                "candidate exceeds workspace budget and eviction is disabled",
            ));
        }
        let evictions = self.eviction_plan(&item, marginal_utility);
        if evictions.is_empty() || self.budget_accepts(&item, &evictions).is_err() {
            return Ok(admission_rejected(
                &item.id,
                marginal_utility,
                "candidate could not displace lower-utility workspace items",
            ));
        }
        let records = self.apply_evictions(&evictions);
        let outcome = admission_accepted(&item.id, marginal_utility, records);
        self.items.push(item);
        Ok(outcome)
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
        self.validate_scope_and_lane(item)?;
        self.budget_accepts(item, &[])
    }

    fn validate_scope_and_lane(&self, item: &Item) -> Result<(), FrameError> {
        if item.owner_actor != self.actor {
            return Err(FrameError::ActorMismatch {
                expected: self.actor.clone(),
                actual: item.owner_actor.clone(),
            });
        }
        let lane = item.effective_lane();
        if !item.taint.allowed_in_lane(lane) {
            return Err(FrameError::ContaminationBarrier {
                item_id: item.id.clone(),
                lane,
                taint: item.taint,
            });
        }
        Ok(())
    }

    fn budget_accepts(&self, item: &Item, evictions: &[usize]) -> Result<(), FrameError> {
        let retained = self.retained_items(evictions);
        let item_count = retained.len().saturating_add(1);
        if item_count > self.budget.max_items {
            return Err(FrameError::ItemBudgetExceeded {
                max_items: self.budget.max_items,
            });
        }
        let next_tokens = retained
            .iter()
            .map(|retained_item| retained_item.estimated_tokens)
            .sum::<usize>()
            .saturating_add(item.estimated_tokens);
        if next_tokens > self.budget.max_input_tokens {
            return Err(FrameError::TokenBudgetExceeded {
                max_tokens: self.budget.max_input_tokens,
            });
        }
        if item.kind == ItemKind::RetrievalEvidence
            && count_kind(&retained, ItemKind::RetrievalEvidence) >= self.budget.max_evidence_items
        {
            return Err(FrameError::EvidenceBudgetExceeded {
                max_evidence_items: self.budget.max_evidence_items,
            });
        }
        if item.kind == ItemKind::ToolSchema
            && count_kind(&retained, ItemKind::ToolSchema) >= self.budget.max_tool_schemas
        {
            return Err(FrameError::ToolSchemaBudgetExceeded {
                max_tool_schemas: self.budget.max_tool_schemas,
            });
        }
        Ok(())
    }

    fn retained_items(&self, evictions: &[usize]) -> Vec<&Item> {
        self.items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| (!evictions.contains(&index)).then_some(item))
            .collect()
    }

    fn eviction_plan(&self, item: &Item, incoming_utility: f32) -> Vec<usize> {
        let mut candidates = self.eviction_candidates(incoming_utility);
        let mut evictions = Vec::new();
        while self.budget_accepts(item, &evictions).is_err() {
            let Some((index, _utility)) = candidates.first().copied() else {
                return Vec::new();
            };
            evictions.push(index);
            candidates.remove(0);
        }
        evictions
    }

    fn eviction_candidates(&self, incoming_utility: f32) -> Vec<(usize, f32)> {
        let mut candidates: Vec<(usize, f32)> = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(index, item)| {
                let utility = item.marginal_utility(&self.budget);
                (!item.effective_lane().is_protected() && utility < incoming_utility)
                    .then_some((index, utility))
            })
            .collect();
        candidates.sort_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| self.items[left.0].id.cmp(&self.items[right.0].id))
        });
        candidates
    }

    fn apply_evictions(&mut self, evictions: &[usize]) -> Vec<EvictionRecord> {
        let mut records = Vec::new();
        for index in evictions.iter().rev().copied() {
            let item = self.items.remove(index);
            let marginal_utility = item.marginal_utility(&self.budget);
            records.push(EvictionRecord {
                item_id: item.id,
                reason: "lower marginal utility than incoming workspace item".to_string(),
                marginal_utility,
            });
        }
        records.reverse();
        records
    }
}

fn admission_accepted(
    item_id: &str,
    marginal_utility: f32,
    evicted: Vec<EvictionRecord>,
) -> AdmissionOutcome {
    let disposition = if evicted.is_empty() {
        AdmissionDisposition::Admitted
    } else {
        AdmissionDisposition::AdmittedAfterEviction
    };
    let reason = if evicted.is_empty() {
        "candidate admitted within workspace budget"
    } else {
        "candidate admitted after evicting lower-utility workspace items"
    };
    AdmissionOutcome {
        disposition,
        item_id: item_id.to_owned(),
        marginal_utility,
        reason: reason.to_string(),
        evicted,
    }
}

fn admission_rejected(item_id: &str, marginal_utility: f32, reason: &str) -> AdmissionOutcome {
    AdmissionOutcome {
        disposition: AdmissionDisposition::Rejected,
        item_id: item_id.to_owned(),
        marginal_utility,
        reason: reason.to_string(),
        evicted: Vec::new(),
    }
}

fn count_kind(items: &[&Item], kind: ItemKind) -> usize {
    items.iter().filter(|item| item.kind == kind).count()
}

fn token_pressure(tokens: usize, max_tokens: usize) -> f32 {
    if max_tokens == 0 {
        return 1.0;
    }
    usize_to_f32(tokens) / usize_to_f32(max_tokens)
}

const fn taint_penalty(taint: Taint) -> f32 {
    match taint {
        Taint::Trusted => 0.0,
        Taint::UserProvided => 0.03,
        Taint::Retrieved => 0.06,
        Taint::ToolOutput => 0.08,
        Taint::External => 0.12,
    }
}

const fn volatility_penalty(volatility: Volatility) -> f32 {
    match volatility {
        Volatility::Stable => 0.0,
        Volatility::Session => 0.02,
        Volatility::Turn => 0.04,
        Volatility::Ephemeral => 0.06,
    }
}

fn usize_to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
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
            Self::ContaminationBarrier {
                item_id,
                lane,
                taint,
            } => {
                write!(
                    f,
                    "workspace contamination barrier rejected {item_id}: {taint:?} cannot enter {lane:?}"
                )
            }
        }
    }
}

impl std::error::Error for FrameError {}
