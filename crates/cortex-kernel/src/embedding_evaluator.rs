use std::collections::HashMap;
use std::path::{Path, PathBuf};

use cortex_types::config::EmbeddingPerformance;

use crate::util::atomic_write;

pub struct EmbeddingEvaluator {
    path: PathBuf,
    models: HashMap<String, EmbeddingPerformance>,
}

impl EmbeddingEvaluator {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        let path = data_dir.join("embedding_perf.json");
        let models = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, models }
    }

    pub fn record_recall(&mut self, model: &str, hit: bool, similarity: f64) {
        let perf = self
            .models
            .entry(model.to_string())
            .or_insert_with(|| EmbeddingPerformance::new(model.to_string()));
        perf.query_count += 1;
        if hit {
            perf.hit_count += 1;
        } else {
            perf.miss_count += 1;
        }
        perf.total_similarity += similarity;
        self.persist();
    }

    #[must_use]
    pub fn get(&self, model: &str) -> Option<&EmbeddingPerformance> {
        self.models.get(model)
    }

    #[must_use]
    pub const fn all(&self) -> &HashMap<String, EmbeddingPerformance> {
        &self.models
    }

    /// Find the best model by precision, with ties broken by avg similarity.
    #[must_use]
    pub fn best_model(&self, min_samples: u32) -> Option<&str> {
        self.models
            .iter()
            .filter(|(_, p)| p.sample_count() >= min_samples)
            .max_by(|(_, a), (_, b)| {
                a.precision()
                    .partial_cmp(&b.precision())
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| {
                        a.avg_similarity()
                            .partial_cmp(&b.avg_similarity())
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
            })
            .map(|(name, _)| name.as_str())
    }

    fn persist(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.models) {
            let _ = atomic_write(&self.path, json.as_bytes());
        }
    }
}
