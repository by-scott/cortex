use std::collections::{BTreeMap, BTreeSet, HashMap};

use chrono::Utc;
use cortex_types::{
    EvidenceAccessClass, EvidenceItem, EvidenceTaint, QueryTransformKind, RetrievalDecision,
    RetrievalQueryPlan, RetrievalScores,
};

const DEFAULT_TOP_K: usize = 8;
const DEFAULT_MIN_SCORE: f32 = 0.05;
const DEFAULT_MAX_EVIDENCE_CHARS: usize = 1_200;
const DEFAULT_VECTOR_DIMENSIONS: usize = 128;

mod reporting;
mod scoring;
mod support;
mod types;

pub use reporting::{
    control_for_support, evaluate, promote_evidence, promotion_events, report_events,
};
use scoring::{
    bm25_component, bucket_for, compare_candidates, cosine, count_terms, decision_kind_for,
    evidence_role_for_chunk, filters_match, graph_hint_score, has_instructional_text,
    normalize_term, normalize_vector, ratio, report_metrics, rerank_score, stable_index_version,
    visible_to_actor,
};
pub use scoring::{compress_text, tokenize};
pub use support::{verify_answer_support, verify_claim_support};
pub use types::{
    Candidate, CandidateFlag, Chunk, ChunkingPolicy, ClaimSupport, ClaimSupportStatus,
    DenseEncoder, Document, DroppedCandidate, DroppedReason, Error, LateInteractionScorer, Metrics,
    NoLateInteraction, NoSparseExpansion, Report, RerankPolicy, SparseExpander, SupportReport,
    WeightedTerm,
};

#[derive(Debug, Clone)]
pub struct HashDenseEncoder {
    dimensions: usize,
    synonyms: HashMap<String, Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Index {
    chunks: Vec<Chunk>,
    document_frequency: HashMap<String, usize>,
    sparse_terms: HashMap<String, HashMap<String, usize>>,
    dense_vectors: HashMap<String, Vec<f32>>,
    avg_terms_per_chunk: f32,
    version: String,
}

#[derive(Debug, Clone)]
pub struct Engine<E, L = NoLateInteraction, S = NoSparseExpansion> {
    index: Index,
    encoder: E,
    late_scorer: L,
    sparse_expander: S,
    policy: RerankPolicy,
}

impl Document {
    #[must_use]
    pub fn new(
        corpus_id: impl Into<String>,
        id: impl Into<String>,
        source_uri: impl Into<String>,
        body: impl Into<String>,
        visibility_actor: impl Into<String>,
    ) -> Self {
        Self {
            id: id.into(),
            corpus_id: corpus_id.into(),
            source_uri: source_uri.into(),
            title: None,
            body: body.into(),
            visibility_actor: visibility_actor.into(),
            access: EvidenceAccessClass::ActorPrivate,
            taint: EvidenceTaint::UserCorpus,
            license: None,
            metadata: BTreeMap::new(),
            updated_at: Utc::now(),
        }
    }

    #[must_use]
    pub const fn public(mut self) -> Self {
        self.access = EvidenceAccessClass::Public;
        self
    }

    #[must_use]
    pub const fn external(mut self) -> Self {
        self.taint = EvidenceTaint::ExternalCorpus;
        self
    }

    #[must_use]
    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    #[must_use]
    pub fn with_license(mut self, license: impl Into<String>) -> Self {
        self.license = Some(license.into());
        self
    }

    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }
}

impl Default for ChunkingPolicy {
    fn default() -> Self {
        Self {
            max_chars: 900,
            overlap_chars: 120,
        }
    }
}

impl ChunkingPolicy {
    #[must_use]
    pub const fn fixed(max_chars: usize, overlap_chars: usize) -> Self {
        Self {
            max_chars,
            overlap_chars,
        }
    }

    /// Splits one document into deterministic, overlapping chunks.
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidChunkingPolicy`] when the overlap is not smaller
    /// than the chunk size, and [`Error::EmptyDocumentBody`] when the document
    /// has no non-whitespace content.
    pub fn chunk(&self, document: &Document) -> Result<Vec<Chunk>, Error> {
        if self.max_chars == 0 || self.overlap_chars >= self.max_chars {
            return Err(Error::InvalidChunkingPolicy);
        }
        if document.body.trim().is_empty() {
            return Err(Error::EmptyDocumentBody);
        }

        let chars: Vec<char> = document.body.chars().collect();
        let mut chunks = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let end = start.saturating_add(self.max_chars).min(chars.len());
            let text: String = chars[start..end].iter().collect();
            let chunk_index = chunks.len();
            chunks.push(Chunk {
                id: format!("{}:{chunk_index}", document.id),
                document_id: document.id.clone(),
                corpus_id: document.corpus_id.clone(),
                source_uri: document.source_uri.clone(),
                source_title: document.title.clone(),
                text,
                span: format!("chars:{start}-{end}"),
                visibility_actor: document.visibility_actor.clone(),
                access: document.access,
                taint: document.taint,
                license: document.license.clone(),
                metadata: document.metadata.clone(),
            });
            if end == chars.len() {
                break;
            }
            start = end.saturating_sub(self.overlap_chars);
        }
        Ok(chunks)
    }
}

impl Default for RerankPolicy {
    fn default() -> Self {
        Self {
            top_k: DEFAULT_TOP_K,
            min_hybrid_score: DEFAULT_MIN_SCORE,
            max_evidence_chars: DEFAULT_MAX_EVIDENCE_CHARS,
        }
    }
}

impl RerankPolicy {
    #[must_use]
    pub const fn strict(top_k: usize, min_hybrid_score: f32, max_evidence_chars: usize) -> Self {
        Self {
            top_k,
            min_hybrid_score,
            max_evidence_chars,
        }
    }
}

impl Default for HashDenseEncoder {
    fn default() -> Self {
        Self::new(DEFAULT_VECTOR_DIMENSIONS)
    }
}

impl HashDenseEncoder {
    #[must_use]
    pub fn new(dimensions: usize) -> Self {
        Self {
            dimensions: dimensions.max(1),
            synonyms: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_synonym(mut self, term: impl Into<String>, equivalent: impl Into<String>) -> Self {
        self.synonyms
            .entry(normalize_term(&term.into()))
            .or_default()
            .push(normalize_term(&equivalent.into()));
        self
    }

    fn expanded_terms(&self, text: &str) -> Vec<String> {
        let mut terms = tokenize(text);
        let additions: Vec<String> = terms
            .iter()
            .filter_map(|term| self.synonyms.get(term))
            .flat_map(|items| items.iter().cloned())
            .collect();
        terms.extend(additions);
        terms
    }
}

impl DenseEncoder for HashDenseEncoder {
    fn encode(&self, text: &str) -> Vec<f32> {
        let mut vector = vec![0.0; self.dimensions];
        for term in self.expanded_terms(text) {
            let bucket = bucket_for(&term, self.dimensions);
            vector[bucket] += 1.0;
        }
        normalize_vector(&mut vector);
        vector
    }
}

impl LateInteractionScorer for NoLateInteraction {
    fn score(&self, _query: &str, _chunk: &Chunk) -> f32 {
        0.0
    }
}

impl SparseExpander for NoSparseExpansion {
    fn expand(&self, _query: &str) -> Vec<WeightedTerm> {
        Vec::new()
    }
}

impl WeightedTerm {
    #[must_use]
    pub fn new(term: impl Into<String>, weight: f32) -> Self {
        Self {
            term: normalize_term(&term.into()),
            weight: weight.clamp(0.0, 4.0),
        }
    }
}

impl Index {
    /// Builds an in-memory hybrid index from documents.
    ///
    /// # Errors
    ///
    /// Returns [`Error`] if any document cannot be chunked.
    pub fn build<E: DenseEncoder>(
        documents: &[Document],
        chunking: ChunkingPolicy,
        encoder: &E,
    ) -> Result<Self, Error> {
        let mut chunks = Vec::new();
        for document in documents {
            chunks.extend(chunking.chunk(document)?);
        }

        let mut document_frequency: HashMap<String, usize> = HashMap::new();
        let mut sparse_terms: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut dense_vectors = HashMap::new();
        let mut total_terms = 0_usize;

        for chunk in &chunks {
            let terms = tokenize(&chunk.text);
            total_terms = total_terms.saturating_add(terms.len());
            let counts = count_terms(terms);
            for term in counts.keys() {
                *document_frequency.entry(term.clone()).or_insert(0) += 1;
            }
            dense_vectors.insert(chunk.id.clone(), encoder.encode(&chunk.text));
            sparse_terms.insert(chunk.id.clone(), counts);
        }

        let avg_terms_per_chunk = if chunks.is_empty() {
            0.0
        } else {
            ratio(total_terms, chunks.len())
        };
        let version = stable_index_version(&chunks);
        Ok(Self {
            chunks,
            document_frequency,
            sparse_terms,
            dense_vectors,
            avg_terms_per_chunk,
            version,
        })
    }

    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.chunks.len()
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    #[must_use]
    pub fn chunks(&self) -> &[Chunk] {
        &self.chunks
    }

    fn visible_chunks<'a>(&'a self, actor: &'a str) -> impl Iterator<Item = &'a Chunk> + 'a {
        self.chunks
            .iter()
            .filter(move |chunk| visible_to_actor(chunk, actor))
    }
}

impl<E: DenseEncoder> Engine<E, NoLateInteraction, NoSparseExpansion> {
    #[must_use]
    pub const fn new(index: Index, encoder: E, policy: RerankPolicy) -> Self {
        Self {
            index,
            encoder,
            late_scorer: NoLateInteraction,
            sparse_expander: NoSparseExpansion,
            policy,
        }
    }
}

impl<E, L, S> Engine<E, L, S> {
    #[must_use]
    pub fn with_late_interaction<N>(self, late_scorer: N) -> Engine<E, N, S> {
        Engine {
            index: self.index,
            encoder: self.encoder,
            late_scorer,
            sparse_expander: self.sparse_expander,
            policy: self.policy,
        }
    }

    #[must_use]
    pub fn with_sparse_expander<N>(self, sparse_expander: N) -> Engine<E, L, N> {
        Engine {
            index: self.index,
            encoder: self.encoder,
            late_scorer: self.late_scorer,
            sparse_expander,
            policy: self.policy,
        }
    }
}

impl<E, L, S> Engine<E, L, S>
where
    E: DenseEncoder,
    L: LateInteractionScorer,
    S: SparseExpander,
{
    /// Runs query planning, hybrid retrieval, reranking, compression, citation,
    /// and evaluation for one actor-scoped query.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyQuery`] when the query contains no searchable text.
    pub fn search(&self, plan: &RetrievalQueryPlan) -> Result<Report, Error> {
        if plan.query.trim().is_empty() {
            return Err(Error::EmptyQuery);
        }

        let query_terms = self.weighted_query_terms(plan);
        let query_vector = self.encoder.encode(&plan.dense_query_text());
        let mut candidates = self.retrieve_candidates(plan, &query_terms, &query_vector);
        candidates.sort_by(compare_candidates);

        let (evidence, dropped) = self.rerank_and_promote(candidates);
        let decision_kind = decision_kind_for(&evidence);
        let best_score = evidence
            .first()
            .map_or(0.0, |item| item.scores.hybrid().clamp(0.0, 1.0));
        let decision = RetrievalDecision::new(
            decision_kind,
            plan.clone(),
            format!("retrieved {} evidence items", evidence.len()),
        )
        .with_support(best_score);
        let metrics = report_metrics(evidence.len(), dropped.len(), best_score);

        Ok(Report {
            decision,
            evidence,
            dropped,
            metrics,
        })
    }

    fn retrieve_candidates(
        &self,
        plan: &RetrievalQueryPlan,
        query_terms: &[WeightedTerm],
        query_vector: &[f32],
    ) -> Vec<Candidate> {
        self.index
            .visible_chunks(&plan.actor)
            .filter(|chunk| filters_match(chunk, &plan.filters))
            .filter_map(|chunk| self.score_chunk(plan, query_terms, query_vector, chunk))
            .collect()
    }

    fn score_chunk(
        &self,
        plan: &RetrievalQueryPlan,
        query_terms: &[WeightedTerm],
        query_vector: &[f32],
        chunk: &Chunk,
    ) -> Option<Candidate> {
        let sparse = if plan.sparse {
            self.sparse_score(query_terms, chunk)
        } else {
            0.0
        };
        let dense = if plan.dense {
            self.dense_score(query_vector, chunk)
        } else {
            0.0
        };
        let graph = if plan.graph {
            graph_hint_score(&plan.query, chunk)
        } else {
            0.0
        };
        let late = self.late_scorer.score(&plan.query, chunk).clamp(0.0, 1.0);
        let scores = RetrievalScores {
            sparse,
            dense,
            rerank: late,
            graph,
        };
        if scores.best() <= 0.0 {
            return None;
        }
        let mut flags = BTreeSet::new();
        if has_instructional_text(&chunk.text) {
            flags.insert(CandidateFlag::InstructionalText);
        }
        Some(Candidate {
            chunk: chunk.clone(),
            scores,
            flags,
        })
    }

    fn sparse_score(&self, query_terms: &[WeightedTerm], chunk: &Chunk) -> f32 {
        let Some(term_counts) = self.index.sparse_terms.get(&chunk.id) else {
            return 0.0;
        };
        let chunk_terms = term_counts.values().sum();
        let mut score = 0.0;
        for weighted in query_terms {
            let Some(term_frequency) = term_counts.get(&weighted.term).copied() else {
                continue;
            };
            let document_frequency = self
                .index
                .document_frequency
                .get(&weighted.term)
                .copied()
                .unwrap_or(0);
            score += weighted.weight
                * bm25_component(
                    term_frequency,
                    document_frequency,
                    self.index.len(),
                    chunk_terms,
                    self.index.avg_terms_per_chunk,
                );
        }
        score / (score + 1.0)
    }

    fn weighted_query_terms(&self, plan: &RetrievalQueryPlan) -> Vec<WeightedTerm> {
        let mut terms: Vec<WeightedTerm> = tokenize(&plan.query)
            .into_iter()
            .map(|term| WeightedTerm { term, weight: 1.0 })
            .collect();
        for transform in &plan.transforms {
            match transform.kind {
                QueryTransformKind::Rewrite | QueryTransformKind::Expansion => {
                    terms.extend(
                        tokenize(&transform.transformed_query)
                            .into_iter()
                            .map(|term| WeightedTerm { term, weight: 0.8 }),
                    );
                }
                QueryTransformKind::HypotheticalDocument | QueryTransformKind::Clarification => {}
            }
        }
        terms.extend(self.sparse_expander.expand(&plan.query));
        terms
    }

    fn dense_score(&self, query_vector: &[f32], chunk: &Chunk) -> f32 {
        self.index
            .dense_vectors
            .get(&chunk.id)
            .map_or(0.0, |vector| cosine(query_vector, vector).max(0.0))
    }

    fn rerank_and_promote(
        &self,
        candidates: Vec<Candidate>,
    ) -> (Vec<EvidenceItem>, Vec<DroppedCandidate>) {
        let mut evidence = Vec::new();
        let mut dropped = Vec::new();
        for mut candidate in candidates {
            let rerank = rerank_score(&candidate);
            candidate.scores.rerank = rerank;
            let hybrid = candidate.scores.hybrid();
            if hybrid < self.policy.min_hybrid_score {
                candidate.flags.insert(CandidateFlag::LowScore);
                dropped.push(DroppedCandidate {
                    chunk_id: candidate.chunk.id,
                    reason: DroppedReason::BelowThreshold,
                    hybrid_score: hybrid,
                });
                continue;
            }
            if evidence.len() >= self.policy.top_k {
                continue;
            }
            evidence.push(self.candidate_to_evidence(&candidate));
        }
        (evidence, dropped)
    }

    fn candidate_to_evidence(&self, candidate: &Candidate) -> EvidenceItem {
        let text = compress_text(&candidate.chunk.text, self.policy.max_evidence_chars);
        let evidence = EvidenceItem::new(
            format!("{}@{}", candidate.chunk.id, self.index.version()),
            candidate.chunk.corpus_id.clone(),
            candidate.chunk.id.clone(),
            candidate.chunk.source_uri.clone(),
            text,
            candidate.chunk.visibility_actor.clone(),
        )
        .with_span(candidate.chunk.span.clone())
        .with_scores(candidate.scores.clone())
        .with_taint(candidate.chunk.taint)
        .with_role(evidence_role_for_chunk(&candidate.chunk))
        .with_access(candidate.chunk.access)
        .with_index_version(self.index.version().to_owned())
        .with_source_title(
            candidate
                .chunk
                .source_title
                .clone()
                .unwrap_or_else(|| candidate.chunk.source_uri.clone()),
        );
        if let Some(license) = candidate.chunk.license.clone() {
            evidence.with_license(license)
        } else {
            evidence
        }
    }
}
