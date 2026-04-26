use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{AuthContext, OwnedScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Episodic,
    Semantic,
    Procedural,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FastCapture {
    pub id: String,
    pub scope: OwnedScope,
    pub text: String,
    pub captured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SemanticMemory {
    pub id: String,
    pub scope: OwnedScope,
    pub kind: MemoryKind,
    pub text: String,
    pub sources: Vec<String>,
    pub stabilized_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterferenceReport {
    pub overlap: f32,
    pub threshold: f32,
    pub conflicting_memory_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConsolidationDecision {
    Promote,
    Merge,
    RejectInterference,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConsolidationJob {
    pub capture: FastCapture,
    pub candidates: Vec<SemanticMemory>,
    pub interference: InterferenceReport,
    pub decision: ConsolidationDecision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkingMemoryBudget {
    pub focus_capacity: usize,
    pub activated_capacity: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingMemoryChunk {
    pub id: String,
    pub scope: OwnedScope,
    pub content: String,
    pub token_estimate: usize,
    pub salience: f32,
    pub last_rehearsed_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OffloadedChunk {
    pub chunk: WorkingMemoryChunk,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkingMemoryError {
    NotVisible,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkingMemory {
    pub budget: WorkingMemoryBudget,
    pub focus: Vec<WorkingMemoryChunk>,
    pub activated: Vec<WorkingMemoryChunk>,
    pub rehearsal_queue: Vec<String>,
    pub offloaded: Vec<OffloadedChunk>,
}

impl FastCapture {
    #[must_use]
    pub fn new(id: impl Into<String>, scope: OwnedScope, text: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            scope,
            text: text.into(),
            captured_at: Utc::now(),
        }
    }
}

impl SemanticMemory {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: OwnedScope,
        kind: MemoryKind,
        text: impl Into<String>,
        sources: Vec<String>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            kind,
            text: text.into(),
            sources,
            stabilized_at: Utc::now(),
        }
    }
}

impl ConsolidationJob {
    #[must_use]
    pub fn evaluate(capture: FastCapture, candidates: Vec<SemanticMemory>, threshold: f32) -> Self {
        let conflicting: Vec<SemanticMemory> = candidates
            .iter()
            .filter(|memory| memory.scope == capture.scope)
            .filter(|memory| lexical_overlap(&capture.text, &memory.text) >= threshold)
            .cloned()
            .collect();
        let overlap = conflicting
            .iter()
            .map(|memory| lexical_overlap(&capture.text, &memory.text))
            .fold(0.0_f32, f32::max);
        let conflicting_memory_ids = conflicting.iter().map(|memory| memory.id.clone()).collect();
        let decision = if conflicting.is_empty() {
            ConsolidationDecision::Promote
        } else if overlap >= threshold {
            ConsolidationDecision::Merge
        } else {
            ConsolidationDecision::RejectInterference
        };

        Self {
            capture,
            candidates,
            interference: InterferenceReport {
                overlap,
                threshold,
                conflicting_memory_ids,
            },
            decision,
        }
    }
}

impl Default for WorkingMemoryBudget {
    fn default() -> Self {
        Self {
            focus_capacity: 4,
            activated_capacity: 16,
        }
    }
}

impl WorkingMemoryChunk {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: OwnedScope,
        content: impl Into<String>,
        now: DateTime<Utc>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            content: content.into(),
            token_estimate: 0,
            salience: 0.0,
            last_rehearsed_at: now,
        }
    }

    #[must_use]
    pub const fn with_salience(mut self, salience: f32) -> Self {
        self.salience = salience.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_tokens(mut self, token_estimate: usize) -> Self {
        self.token_estimate = token_estimate;
        self
    }
}

impl WorkingMemory {
    #[must_use]
    pub const fn new(budget: WorkingMemoryBudget) -> Self {
        Self {
            budget,
            focus: Vec::new(),
            activated: Vec::new(),
            rehearsal_queue: Vec::new(),
            offloaded: Vec::new(),
        }
    }

    /// # Errors
    /// Returns an error when a chunk is not visible to the current actor.
    pub fn admit(
        &mut self,
        context: &AuthContext,
        chunk: WorkingMemoryChunk,
    ) -> Result<(), WorkingMemoryError> {
        if !chunk.scope.is_visible_to(context) {
            return Err(WorkingMemoryError::NotVisible);
        }
        self.rehearsal_queue.push(chunk.id.clone());
        self.focus.push(chunk);
        self.focus.sort_by(priority_order);
        self.enforce_budget();
        Ok(())
    }

    pub fn rehearse_at(&mut self, now: DateTime<Utc>) {
        let Some(chunk_id) = self.rehearsal_queue.first().cloned() else {
            return;
        };
        self.rehearsal_queue.rotate_left(1);
        if let Some(chunk) = self
            .focus
            .iter_mut()
            .chain(self.activated.iter_mut())
            .find(|chunk| chunk.id == chunk_id)
        {
            chunk.last_rehearsed_at = now;
            chunk.salience = (chunk.salience + 0.05).clamp(0.0, 1.0);
        }
        self.focus.sort_by(priority_order);
        self.activated.sort_by(priority_order);
    }

    #[must_use]
    pub fn select_for_context(&self, context: &AuthContext) -> Vec<WorkingMemoryChunk> {
        self.focus
            .iter()
            .chain(self.activated.iter())
            .filter(|chunk| chunk.scope.is_visible_to(context))
            .cloned()
            .collect()
    }

    fn enforce_budget(&mut self) {
        while self.focus.len() > self.budget.focus_capacity {
            let Some(chunk) = self.focus.pop() else {
                break;
            };
            self.activated.push(chunk);
            self.activated.sort_by(priority_order);
        }
        while self.activated.len() > self.budget.activated_capacity {
            let Some(chunk) = self.activated.pop() else {
                break;
            };
            self.offloaded.push(OffloadedChunk {
                chunk,
                reason: "working_memory_capacity".to_string(),
            });
        }
    }
}

fn lexical_overlap(left: &str, right: &str) -> f32 {
    let left_terms = terms(left);
    let right_terms = terms(right);
    if left_terms.is_empty() || right_terms.is_empty() {
        return 0.0;
    }
    let shared = left_terms
        .iter()
        .filter(|term| right_terms.contains(term))
        .count();
    ratio(shared, left_terms.len().max(right_terms.len()))
}

fn terms(text: &str) -> Vec<String> {
    let mut terms: Vec<String> = text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|term| !term.is_empty())
        .map(str::to_ascii_lowercase)
        .collect();
    terms.sort();
    terms.dedup();
    terms
}

fn ratio(numerator: usize, denominator: usize) -> f32 {
    let numerator = u16::try_from(numerator).unwrap_or(u16::MAX);
    let denominator = u16::try_from(denominator).unwrap_or(u16::MAX).max(1);
    f32::from(numerator) / f32::from(denominator)
}

fn priority_order(left: &WorkingMemoryChunk, right: &WorkingMemoryChunk) -> std::cmp::Ordering {
    right
        .salience
        .total_cmp(&left.salience)
        .then_with(|| left.token_estimate.cmp(&right.token_estimate))
        .then_with(|| left.id.cmp(&right.id))
}
