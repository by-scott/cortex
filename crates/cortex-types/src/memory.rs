use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::OwnedScope;

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
