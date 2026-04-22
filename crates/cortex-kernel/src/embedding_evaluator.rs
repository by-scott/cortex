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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_empty() {
        let dir = tempfile::tempdir().unwrap();
        let eval = EmbeddingEvaluator::open(dir.path());
        assert!(eval.all().is_empty());
    }

    #[test]
    fn record_and_get() {
        let dir = tempfile::tempdir().unwrap();
        let mut eval = EmbeddingEvaluator::open(dir.path());
        eval.record_recall("model-a", true, 0.9);
        eval.record_recall("model-a", false, 0.3);
        let perf = eval.get("model-a").unwrap();
        assert_eq!(perf.hit_count, 1);
        assert_eq!(perf.miss_count, 1);
    }

    #[test]
    fn persist_and_reload() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut eval = EmbeddingEvaluator::open(dir.path());
            eval.record_recall("model-b", true, 0.8);
        }
        let eval2 = EmbeddingEvaluator::open(dir.path());
        assert!(eval2.get("model-b").is_some());
    }

    #[test]
    fn best_model_min_samples() {
        let dir = tempfile::tempdir().unwrap();
        let mut eval = EmbeddingEvaluator::open(dir.path());
        eval.record_recall("few", true, 0.9);
        assert!(eval.best_model(5).is_none());
        for _ in 0..5 {
            eval.record_recall("enough", true, 0.8);
        }
        assert_eq!(eval.best_model(5), Some("enough"));
    }
}
