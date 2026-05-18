use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Utc};
use cortex_types::{
    EvidenceAccessClass, EvidenceItem, EvidenceTaint, RetrievalDecision, RetrievalScores,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub id: String,
    pub corpus_id: String,
    pub source_uri: String,
    pub title: Option<String>,
    pub body: String,
    pub visibility_actor: String,
    pub access: EvidenceAccessClass,
    pub taint: EvidenceTaint,
    pub license: Option<String>,
    pub metadata: BTreeMap<String, String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chunk {
    pub id: String,
    pub document_id: String,
    pub corpus_id: String,
    pub source_uri: String,
    pub source_title: Option<String>,
    pub text: String,
    pub span: String,
    pub visibility_actor: String,
    pub access: EvidenceAccessClass,
    pub taint: EvidenceTaint,
    pub license: Option<String>,
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkingPolicy {
    pub max_chars: usize,
    pub overlap_chars: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RerankPolicy {
    pub top_k: usize,
    pub min_hybrid_score: f32,
    pub max_evidence_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    pub chunk: Chunk,
    pub scores: RetrievalScores,
    pub flags: BTreeSet<CandidateFlag>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeightedTerm {
    pub term: String,
    pub weight: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum CandidateFlag {
    InstructionalText,
    LowScore,
    ActorRestricted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DroppedReason {
    BelowThreshold,
    HiddenFromActor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroppedCandidate {
    pub chunk_id: String,
    pub reason: DroppedReason,
    pub hybrid_score: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub decision: RetrievalDecision,
    pub evidence: Vec<EvidenceItem>,
    pub dropped: Vec<DroppedCandidate>,
    pub metrics: Metrics,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClaimSupportStatus {
    Supported,
    Contradicted,
    Unsupported,
    InsufficientEvidence,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ClaimSupport {
    pub claim: String,
    pub status: ClaimSupportStatus,
    pub supporting_evidence: Vec<String>,
    pub contradicting_evidence: Vec<String>,
    pub contextual_evidence: Vec<String>,
    pub support_score: f32,
    pub contradiction_score: f32,
    pub confidence: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SupportReport {
    pub claims: Vec<ClaimSupport>,
    pub supported_count: usize,
    pub contradicted_count: usize,
    pub unsupported_count: usize,
    pub insufficient_count: usize,
    pub coverage_score: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Metrics {
    pub candidate_count: usize,
    pub evidence_count: usize,
    pub best_score: f32,
    pub recall_at_k: Option<f32>,
    pub reciprocal_rank: Option<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    EmptyDocumentBody,
    EmptyQuery,
    InvalidChunkingPolicy,
}

pub trait DenseEncoder {
    fn encode(&self, text: &str) -> Vec<f32>;
}

pub trait LateInteractionScorer {
    fn score(&self, query: &str, chunk: &Chunk) -> f32;
}

pub trait SparseExpander {
    fn expand(&self, query: &str) -> Vec<WeightedTerm>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoLateInteraction;

#[derive(Debug, Clone, Copy, Default)]
pub struct NoSparseExpansion;
