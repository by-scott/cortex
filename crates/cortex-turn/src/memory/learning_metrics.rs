use std::sync::atomic::{AtomicU32, Ordering};

use serde::Serialize;

/// Thread-safe learning metrics tracked via atomics.
pub struct LearningMetrics {
    recall_count: AtomicU32,
    recall_hits: AtomicU32,
    consolidation_merges: AtomicU32,
    prompt_updates: AtomicU32,
    memory_count: AtomicU32,
}

#[derive(Debug, Clone, Serialize)]
pub struct LearningSnapshot {
    pub recall_count: u32,
    pub recall_hits: u32,
    pub recall_precision: f64,
    pub consolidation_merges: u32,
    pub prompt_updates: u32,
    pub memory_count: u32,
}

impl LearningMetrics {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            recall_count: AtomicU32::new(0),
            recall_hits: AtomicU32::new(0),
            consolidation_merges: AtomicU32::new(0),
            prompt_updates: AtomicU32::new(0),
            memory_count: AtomicU32::new(0),
        }
    }

    pub fn record_recall(&self) {
        self.recall_count.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_recall_hit(&self) {
        self.recall_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_consolidation_merge(&self) {
        self.consolidation_merges.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_prompt_update(&self) {
        self.prompt_updates.fetch_add(1, Ordering::Relaxed);
    }

    pub fn set_memory_count(&self, count: u32) {
        self.memory_count.store(count, Ordering::Relaxed);
    }

    #[must_use]
    pub fn snapshot(&self) -> LearningSnapshot {
        let recall_count = self.recall_count.load(Ordering::Relaxed);
        let recall_hits = self.recall_hits.load(Ordering::Relaxed);
        let recall_precision = if recall_count > 0 {
            f64::from(recall_hits) / f64::from(recall_count)
        } else {
            0.0
        };
        LearningSnapshot {
            recall_count,
            recall_hits,
            recall_precision,
            consolidation_merges: self.consolidation_merges.load(Ordering::Relaxed),
            prompt_updates: self.prompt_updates.load(Ordering::Relaxed),
            memory_count: self.memory_count.load(Ordering::Relaxed),
        }
    }

    pub fn reset(&self) {
        self.recall_count.store(0, Ordering::Relaxed);
        self.recall_hits.store(0, Ordering::Relaxed);
        self.consolidation_merges.store(0, Ordering::Relaxed);
        self.prompt_updates.store(0, Ordering::Relaxed);
        self.memory_count.store(0, Ordering::Relaxed);
    }
}

impl Default for LearningMetrics {
    fn default() -> Self {
        Self::new()
    }
}
