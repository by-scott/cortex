#![warn(clippy::pedantic, clippy::nursery)]

mod engine;

pub use engine::{
    Candidate, CandidateFlag, Chunk, ChunkingPolicy, ClaimSupport, ClaimSupportStatus,
    DenseEncoder, Document, DroppedCandidate, DroppedReason, Engine, Error, HashDenseEncoder,
    Index, LateInteractionScorer, Metrics, NoLateInteraction, NoSparseExpansion, Report,
    RerankPolicy, SparseExpander, SupportReport, WeightedTerm, compress_text, control_for_support,
    evaluate, promote_evidence, promotion_events, report_events, tokenize, verify_answer_support,
    verify_claim_support,
};
