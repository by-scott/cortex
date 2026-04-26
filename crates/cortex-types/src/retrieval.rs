use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{CorpusId, OwnedScope};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccessClass {
    Public,
    Tenant,
    Actor,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceTaint {
    TrustedCorpus,
    ExternalCorpus,
    ToolOutput,
    Web,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HybridScores {
    pub lexical: f32,
    pub dense: f32,
    pub rerank: f32,
    pub citation: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueryPlan {
    pub query: String,
    pub scope: OwnedScope,
    pub corpus_id: CorpusId,
    pub active_retrieval: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub id: String,
    pub scope: OwnedScope,
    pub corpus_id: CorpusId,
    pub source_uri: String,
    pub text: String,
    pub access: AccessClass,
    pub taint: EvidenceTaint,
    pub scores: HybridScores,
    pub retrieved_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalDecision {
    Sufficient,
    NeedsMoreEvidence,
    BlockedByAccess,
    BlockedByTaint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementStrategy {
    FrontloadBest,
    Sandwich,
}

impl HybridScores {
    #[must_use]
    pub fn support(self) -> f32 {
        self.citation
            .mul_add(
                0.15,
                self.rerank
                    .mul_add(0.35, self.lexical.mul_add(0.25, self.dense * 0.25)),
            )
            .clamp(0.0, 1.0)
    }
}

impl Evidence {
    #[must_use]
    pub fn new(
        id: impl Into<String>,
        scope: OwnedScope,
        corpus_id: CorpusId,
        source_uri: impl Into<String>,
        text: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            scope,
            corpus_id,
            source_uri: source_uri.into(),
            text: text.into(),
            access: AccessClass::Actor,
            taint: EvidenceTaint::ExternalCorpus,
            scores: HybridScores {
                lexical: 0.0,
                dense: 0.0,
                rerank: 0.0,
                citation: 0.0,
            },
            retrieved_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn with_scores(mut self, scores: HybridScores) -> Self {
        self.scores = scores;
        self
    }

    #[must_use]
    pub const fn with_access(mut self, access: AccessClass) -> Self {
        self.access = access;
        self
    }

    #[must_use]
    pub const fn with_taint(mut self, taint: EvidenceTaint) -> Self {
        self.taint = taint;
        self
    }

    #[must_use]
    pub fn looks_instructional(&self) -> bool {
        let lower = self.text.to_ascii_lowercase();
        [
            "ignore previous",
            "system prompt",
            "developer message",
            "exfiltrate",
        ]
        .iter()
        .any(|pattern| lower.contains(pattern))
    }
}

#[must_use]
pub fn decide(evidence: &[Evidence], threshold: f32) -> RetrievalDecision {
    if evidence.iter().any(Evidence::looks_instructional) {
        return RetrievalDecision::BlockedByTaint;
    }
    let support = evidence
        .iter()
        .map(|item| item.scores.support())
        .fold(0.0_f32, f32::max);
    if support >= threshold {
        RetrievalDecision::Sufficient
    } else {
        RetrievalDecision::NeedsMoreEvidence
    }
}

#[must_use]
pub fn place(mut evidence: Vec<Evidence>, strategy: PlacementStrategy) -> Vec<Evidence> {
    evidence.sort_by(|left, right| right.scores.support().total_cmp(&left.scores.support()));
    match strategy {
        PlacementStrategy::FrontloadBest => evidence,
        PlacementStrategy::Sandwich => sandwich(evidence),
    }
}

fn sandwich(evidence: Vec<Evidence>) -> Vec<Evidence> {
    let mut front = Vec::new();
    let mut back = Vec::new();
    for (index, item) in evidence.into_iter().enumerate() {
        if index % 2 == 0 {
            front.push(item);
        } else {
            back.push(item);
        }
    }
    back.reverse();
    front.extend(back);
    front
}
