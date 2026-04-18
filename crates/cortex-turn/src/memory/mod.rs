pub mod batch_post_turn;
pub mod calibration;
pub mod consolidate;
pub mod decay;
pub mod evolution;
pub mod extract;
pub mod learning_metrics;
pub mod lifecycle;
pub mod recall;
pub mod user_signal;

pub use calibration::{
    CalibrationSnapshot, ReconsolidationSnapshot, ReconsolidationTracker, SignalPrecisionTracker,
    UpdateQualityScorer,
};
pub use consolidate::{
    ConsolidateResult, FullConsolidateResult, MergedAttributes, SmartConsolidateResult,
};
pub use decay::{freshness, should_deprecate};
pub use learning_metrics::{LearningMetrics, LearningSnapshot};
pub use lifecycle::{deprecate_expired, should_consolidate, should_extract};
pub use recall::{
    EmbeddingHealthStatus, EmbeddingRecaller, bm25_score, build_memory_context, cosine_similarity,
    graph_expand_recall_with_depth, graph_reasoning_scores, hybrid_rank, mark_reconsolidation,
    rank_memories, rank_memories_filtered,
};
pub use user_signal::{detect_correction, detect_new_domain, detect_preference};
