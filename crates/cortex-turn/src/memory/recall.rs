use chrono::{DateTime, Utc};
use cortex_kernel::{EmbeddingClient, EmbeddingStore};
use cortex_types::{MemoryEntry, MemoryStatus};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, AtomicU32, Ordering};

// ── Weights for multi-dimensional ranking ─────────────────

const W_BM25: f64 = 0.25;
const W_COSINE: f64 = 0.40;
const W_RECENCY: f64 = 0.15;
const W_STATUS: f64 = 0.10;
const W_ACCESS: f64 = 0.05;
const W_GRAPH: f64 = 0.10;

// ── Scoring functions ─────────────────────────────────────

/// Simple BM25-like scoring: term frequency in document.
#[must_use]
pub fn bm25_score(query: &str, document: &str) -> f64 {
    let query_terms: Vec<&str> = query.split_whitespace().collect();
    let doc_lower = document.to_lowercase();

    let mut score = 0.0;
    for term in &query_terms {
        let term_lower = term.to_lowercase();
        let tf =
            f64::from(u32::try_from(doc_lower.matches(&term_lower).count()).unwrap_or(u32::MAX));
        if tf > 0.0 {
            // Simplified BM25: tf / (tf + 1.2)
            score += tf / (tf + 1.2);
        }
    }
    score
}

/// Cosine similarity between two vectors.
#[must_use]
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> f64 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f64 = a.iter().map(|x| x * x).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| x * x).sum::<f64>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Recency score: exponential decay based on hours since creation.
///
/// Returns value in `[0, 1]` where recent memories score higher.
#[must_use]
pub fn recency_score(created_at: DateTime<Utc>) -> f64 {
    let secs = u32::try_from((Utc::now() - created_at).num_seconds().max(0)).unwrap_or(u32::MAX);
    let hours = f64::from(secs) / 3600.0;
    (-0.01 * hours).exp()
}

/// Status-based reliability score.
#[must_use]
pub const fn status_score(status: MemoryStatus) -> f64 {
    match status {
        MemoryStatus::Stabilized => 1.0,
        MemoryStatus::Materialized => 0.7,
        MemoryStatus::Captured => 0.4,
        MemoryStatus::Deprecated => 0.1,
    }
}

/// Access frequency score: logarithmic scaling, clamped to `[0, 1]`.
#[must_use]
pub fn access_score(access_count: u32) -> f64 {
    if access_count == 0 {
        return 0.0;
    }
    let score = f64::from(access_count).ln_1p() / 100.0_f64.ln_1p();
    score.min(1.0)
}

// ── Ranking functions ─────────────────────────────────────

/// Rank memories by BM25 relevance to query, return top N.
#[must_use]
pub fn rank_memories<'a>(
    query: &str,
    memories: &'a [MemoryEntry],
    top_n: usize,
) -> Vec<&'a MemoryEntry> {
    let mut scored: Vec<(&MemoryEntry, f64)> = memories
        .iter()
        .map(|m| {
            let text = format!("{} {}", m.description, m.content);
            let score = bm25_score(query, &text);
            (m, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(top_n).map(|(m, _)| m).collect()
}

/// Rank memories with optional trust-based filtering.
///
/// When `filter_untrusted` is true, memories with
/// `MemorySource::Network` (trust level `Untrusted`) are excluded.
#[must_use]
pub fn rank_memories_filtered<'a>(
    query: &str,
    memories: &'a [MemoryEntry],
    top_n: usize,
    filter_untrusted: bool,
) -> Vec<&'a MemoryEntry> {
    let filtered: Vec<&MemoryEntry> = if filter_untrusted {
        memories
            .iter()
            .filter(|m| m.source.trust_level() != cortex_types::TrustLevel::Untrusted)
            .collect()
    } else {
        memories.iter().collect()
    };

    let mut scored: Vec<(&MemoryEntry, f64)> = filtered
        .into_iter()
        .map(|m| {
            let text = format!("{} {}", m.description, m.content);
            let score = bm25_score(query, &text);
            (m, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(top_n).map(|(m, _)| m).collect()
}

/// Multi-dimensional hybrid ranking.
///
/// Combines BM25 + cosine + recency + status + access + optional graph.
/// Falls back to BM25 + metadata when embeddings unavailable.
/// When `graph_scores` is provided, `W_GRAPH` weight is added (taken from `W_BM25`).
#[must_use]
pub fn hybrid_rank<'a, S: std::hash::BuildHasher>(
    query: &str,
    query_embedding: Option<&[f64]>,
    memories: &'a [MemoryEntry],
    memory_embeddings: &[Option<Vec<f64>>],
    top_n: usize,
    graph_scores: Option<&HashMap<String, f64, S>>,
) -> Vec<&'a MemoryEntry> {
    let has_graph = graph_scores.is_some_and(|gs| !gs.is_empty());

    // When no embedding, redistribute cosine weight to bm25
    let (w_bm25, w_cosine) = if query_embedding.is_some() {
        if has_graph {
            (W_BM25 - W_GRAPH, W_COSINE)
        } else {
            (W_BM25, W_COSINE)
        }
    } else if has_graph {
        (W_BM25 + W_COSINE - W_GRAPH, 0.0)
    } else {
        (W_BM25 + W_COSINE, 0.0)
    };

    let w_graph = if has_graph { W_GRAPH } else { 0.0 };

    let mut scored: Vec<(&MemoryEntry, f64)> = memories
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let text = format!("{} {}", m.description, m.content);
            let bm25 = bm25_score(query, &text);
            let cosine = match (
                query_embedding,
                memory_embeddings.get(i).and_then(|e| e.as_deref()),
            ) {
                (Some(qe), Some(me)) => cosine_similarity(qe, me),
                _ => 0.0,
            };
            let recency = recency_score(m.created_at);
            let status = status_score(m.status);
            let access = access_score(m.access_count);
            let graph = graph_scores
                .and_then(|gs| gs.get(&m.id))
                .copied()
                .unwrap_or(0.0);

            let score = bm25.mul_add(
                w_bm25,
                cosine.mul_add(
                    w_cosine,
                    recency.mul_add(
                        W_RECENCY,
                        status.mul_add(W_STATUS, access.mul_add(W_ACCESS, graph * w_graph)),
                    ),
                ),
            );
            (m, score)
        })
        .filter(|(_, score)| *score > 0.0)
        .collect();

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.into_iter().take(top_n).map(|(m, _)| m).collect()
}

/// Mark recalled memories as entering the reconsolidation window (Nader 2000).
///
/// Loads each recalled memory by ID from the store, sets
/// `reconsolidation_until` to `now + window_minutes`, and saves back.
/// Errors on individual memories are silently ignored.
pub fn mark_reconsolidation(
    recalled: &[&MemoryEntry],
    store: &cortex_kernel::MemoryStore,
    window_minutes: i64,
) {
    let window = chrono::Duration::minutes(window_minutes);
    let until = chrono::Utc::now() + window;
    for m in recalled {
        if let Ok(mut entry) = store.load(&m.id) {
            entry.reconsolidation_until = Some(until);
            let _ = store.save(&entry);
        }
    }
}

/// Build memory context string for injection into system prompt.
#[must_use]
pub fn build_memory_context(memories: &[&MemoryEntry]) -> String {
    if memories.is_empty() {
        return String::new();
    }

    let mut parts = Vec::new();
    for m in memories {
        parts.push(format!(
            "[{:?}/{:?}] {}: {}",
            m.memory_type, m.kind, m.description, m.content
        ));
    }
    parts.join("\n")
}

// ── Embedding health tracking ───────────────────────────────

const DEGRADED_THRESHOLD: u32 = 3;
const COOLDOWN_SECS: i64 = 60;

/// Tracks embedding provider health across Turn executions.
///
/// Uses atomic operations so it can be shared via `&self` without mutation.
/// Transitions: Healthy -> Degraded (after `DEGRADED_THRESHOLD` consecutive
/// failures) -> Healthy (after successful probe post-cooldown).
pub struct EmbeddingHealthStatus {
    consecutive_failures: AtomicU32,
    last_failure_epoch: AtomicI64,
}

impl EmbeddingHealthStatus {
    /// Create a new healthy status.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            consecutive_failures: AtomicU32::new(0),
            last_failure_epoch: AtomicI64::new(0),
        }
    }

    /// Whether the embedding provider is considered degraded.
    #[must_use]
    pub fn is_degraded(&self) -> bool {
        self.consecutive_failures.load(Ordering::Relaxed) >= DEGRADED_THRESHOLD
    }

    /// Whether the cooldown period has elapsed since last failure.
    #[must_use]
    pub fn cooldown_elapsed(&self) -> bool {
        let last = self.last_failure_epoch.load(Ordering::Relaxed);
        if last == 0 {
            return true;
        }
        Utc::now().timestamp() - last >= COOLDOWN_SECS
    }

    /// Record a failure (degraded vector or connection error).
    pub fn record_failure(&self) {
        self.consecutive_failures.fetch_add(1, Ordering::Relaxed);
        self.last_failure_epoch
            .store(Utc::now().timestamp(), Ordering::Relaxed);
    }

    /// Record a success — resets the failure counter.
    pub fn record_success(&self) {
        self.consecutive_failures.store(0, Ordering::Relaxed);
    }
}

impl Default for EmbeddingHealthStatus {
    fn default() -> Self {
        Self::new()
    }
}

// ── Embedding-powered recall ────────────────────────────────

pub struct EmbeddingRecaller<'a> {
    client: &'a EmbeddingClient,
    cache: &'a EmbeddingStore,
    health: Option<&'a EmbeddingHealthStatus>,
}

impl<'a> EmbeddingRecaller<'a> {
    #[must_use]
    pub const fn new(client: &'a EmbeddingClient, cache: &'a EmbeddingStore) -> Self {
        Self {
            client,
            cache,
            health: None,
        }
    }

    /// Create a recaller with health status tracking.
    #[must_use]
    pub const fn with_health(
        client: &'a EmbeddingClient,
        cache: &'a EmbeddingStore,
        health: &'a EmbeddingHealthStatus,
    ) -> Self {
        Self {
            client,
            cache,
            health: Some(health),
        }
    }

    /// Recall memories using hybrid BM25 + embedding + metadata search.
    ///
    /// When sqlite-vec is available, the vector index is queried first to
    /// obtain a candidate set (O(log n) instead of O(n)), which is then
    /// re-ranked with the full hybrid scoring pipeline. Falls back to
    /// BM25-only if:
    /// - embedding provider is degraded and cooldown has not elapsed
    /// - embedding call fails or returns degraded vectors
    pub async fn recall<'m>(
        &self,
        query: &str,
        memories: &'m [MemoryEntry],
        top_n: usize,
        graph_scores: Option<HashMap<String, f64>>,
    ) -> Vec<&'m MemoryEntry> {
        // Check health: if degraded and cooldown not elapsed, skip embedding entirely
        if let Some(health) = self.health
            && health.is_degraded()
            && !health.cooldown_elapsed()
        {
            return rank_memories(query, memories, top_n);
        }

        // Try to embed the query + validate quality
        let query_embedding = self.client.embed(query).await.ok().and_then(|emb| {
            cortex_kernel::embedding_client::validate_embedding(&emb)
                .ok()
                .map(|()| emb)
        });

        if let Some(health) = self.health {
            if query_embedding.is_some() {
                health.record_success();
            } else {
                health.record_failure();
            }
        }

        let Some(query_emb) = query_embedding else {
            return rank_memories(query, memories, top_n);
        };

        // Use sqlite-vec vector index for candidate retrieval.
        // Fetch more candidates than needed so hybrid ranking can refine.
        let vec_candidates = self.cache.search_vectors(&query_emb, top_n * 3);

        if vec_candidates.is_empty() {
            // No indexed vectors yet — fall back to full scan with BM25 only.
            return rank_memories(query, memories, top_n);
        }

        // Map candidate IDs back to indices into the original slice.
        let candidate_ids: std::collections::HashSet<&str> =
            vec_candidates.iter().map(|(id, _)| id.as_str()).collect();

        let candidate_indices: Vec<usize> = memories
            .iter()
            .enumerate()
            .filter(|(_, m)| candidate_ids.contains(m.id.as_str()))
            .map(|(i, _)| i)
            .collect();

        if candidate_indices.is_empty() {
            return rank_memories(query, memories, top_n);
        }

        // Build a contiguous candidate slice and matching embeddings.
        let candidate_entries: Vec<MemoryEntry> = candidate_indices
            .iter()
            .map(|&i| memories[i].clone())
            .collect();

        let mut candidate_embeddings: Vec<Option<Vec<f64>>> =
            Vec::with_capacity(candidate_entries.len());
        for mem in &candidate_entries {
            let text = format!("{} {}", mem.description, mem.content);
            let hash = cortex_kernel::embedding_store::content_hash(&text);
            candidate_embeddings.push(self.cache.get(&hash));
        }

        // Run hybrid ranking on the candidate set, then map back to
        // references into the caller's original `memories` slice.
        let ranked = hybrid_rank(
            query,
            Some(&query_emb),
            &candidate_entries,
            &candidate_embeddings,
            top_n,
            graph_scores.as_ref(),
        );

        // Map ranked results back to references in the original slice.
        let ranked_ids: Vec<String> = ranked.iter().map(|m| m.id.clone()).collect();
        ranked_ids
            .iter()
            .filter_map(|id| memories.iter().find(|m| m.id == *id))
            .collect()
    }
}

// ── Graph-based recall expansion ──────────────────────────

const MAX_GRAPH_EXPANSION: usize = 5;
const DEFAULT_MAX_DEPTH: usize = 3;

/// Expand a set of recalled memory IDs by discovering related memories.
///
/// Uses multi-hop graph traversal (BFS up to `max_depth`).
/// Returns the original IDs plus up to `MAX_GRAPH_EXPANSION` additional IDs,
/// prioritized by proximity (closer hops first, then by connection count
/// within same depth).
#[must_use]
pub fn graph_expand_recall(
    initial_ids: &[String],
    graph: &cortex_kernel::MemoryGraph,
) -> Vec<String> {
    graph_expand_recall_with_depth(initial_ids, graph, DEFAULT_MAX_DEPTH)
}

/// Multi-hop graph expansion with configurable depth.
#[must_use]
pub fn graph_expand_recall_with_depth(
    initial_ids: &[String],
    graph: &cortex_kernel::MemoryGraph,
    max_depth: usize,
) -> Vec<String> {
    let initial_set: std::collections::HashSet<&str> = initial_ids
        .iter()
        .map(std::string::String::as_str)
        .collect();

    // BFS: track (id, min_depth) for all discovered nodes not in initial set
    let mut discovered: HashMap<String, usize> = HashMap::new();
    let mut frontier: Vec<String> = initial_ids.to_vec();

    for depth in 1..=max_depth {
        let mut next_frontier: Vec<String> = Vec::new();
        for id in &frontier {
            if let Ok(neighbors) = graph.neighbors(id) {
                for n in neighbors {
                    if !initial_set.contains(n.as_str()) && !discovered.contains_key(&n) {
                        discovered.insert(n.clone(), depth);
                        next_frontier.push(n);
                    }
                }
            }
        }
        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    // Sort by depth (closer first), take top MAX_GRAPH_EXPANSION
    let mut expansion: Vec<(String, usize)> = discovered.into_iter().collect();
    expansion.sort_by_key(|(_, depth)| *depth);

    let mut result: Vec<String> = initial_ids.to_vec();
    for (id, _) in expansion.into_iter().take(MAX_GRAPH_EXPANSION) {
        result.push(id);
    }

    result
}

/// Compute graph reasoning scores for memories reachable from initial IDs.
///
/// Uses BFS up to `max_depth` hops. Score decays exponentially: `2^(-depth)`.
/// Returns a map of `memory_id` to reasoning score for all reachable nodes
/// (excluding the initial IDs themselves).
#[must_use]
pub fn graph_reasoning_scores(
    initial_ids: &[String],
    graph: &cortex_kernel::MemoryGraph,
    max_depth: usize,
) -> HashMap<String, f64> {
    let initial_set: std::collections::HashSet<&str> = initial_ids
        .iter()
        .map(std::string::String::as_str)
        .collect();

    let mut scores: HashMap<String, f64> = HashMap::new();
    let mut visited: std::collections::HashSet<String> = initial_ids.iter().cloned().collect();
    let mut frontier: Vec<String> = initial_ids.to_vec();

    for depth in 1..=max_depth {
        let score = 2.0_f64.powi(-i32::try_from(depth).unwrap_or(i32::MAX));
        let mut next_frontier: Vec<String> = Vec::new();

        for id in &frontier {
            if let Ok(neighbors) = graph.neighbors(id) {
                for n in neighbors {
                    if !visited.contains(&n) {
                        visited.insert(n.clone());
                        if !initial_set.contains(n.as_str()) {
                            scores.insert(n.clone(), score);
                        }
                        next_frontier.push(n);
                    }
                }
            }
        }

        if next_frontier.is_empty() {
            break;
        }
        frontier = next_frontier;
    }

    scores
}
