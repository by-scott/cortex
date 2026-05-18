use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::hash::{Hash, Hasher};

use cortex_types::{EvidenceAccessClass, EvidenceRole, EvidenceTaint, RetrievalDecisionKind};

use super::{Candidate, CandidateFlag, Chunk, Metrics};

#[must_use]
pub fn tokenize(input: &str) -> Vec<String> {
    let mut terms = Vec::new();
    let mut current = String::new();
    for character in input.chars() {
        if character.is_alphanumeric() {
            for lower in character.to_lowercase() {
                current.push(lower);
            }
        } else if !current.is_empty() {
            terms.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        terms.push(current);
    }
    terms
}

#[must_use]
pub fn compress_text(input: &str, max_chars: usize) -> String {
    if input.chars().count() <= max_chars {
        return input.to_owned();
    }
    input.chars().take(max_chars).collect()
}

pub(super) const fn report_metrics(
    evidence_count: usize,
    dropped_count: usize,
    best_score: f32,
) -> Metrics {
    Metrics {
        candidate_count: evidence_count.saturating_add(dropped_count),
        evidence_count,
        best_score,
        recall_at_k: None,
        reciprocal_rank: None,
    }
}

pub(super) const fn decision_kind_for(
    evidence: &[cortex_types::EvidenceItem],
) -> RetrievalDecisionKind {
    if evidence.is_empty() {
        RetrievalDecisionKind::Insufficient
    } else {
        RetrievalDecisionKind::Needed
    }
}

pub(super) fn compare_candidates(left: &Candidate, right: &Candidate) -> std::cmp::Ordering {
    right
        .scores
        .hybrid()
        .partial_cmp(&left.scores.hybrid())
        .unwrap_or(std::cmp::Ordering::Equal)
        .then_with(|| left.chunk.id.cmp(&right.chunk.id))
}

pub(super) fn visible_to_actor(chunk: &Chunk, actor: &str) -> bool {
    matches!(chunk.access, EvidenceAccessClass::Public) || chunk.visibility_actor == actor
}

pub(super) fn filters_match(chunk: &Chunk, filters: &[String]) -> bool {
    filters.iter().all(|filter| {
        if let Some(corpus_id) = filter.strip_prefix("corpus=") {
            return chunk.corpus_id == corpus_id;
        }
        if let Some(metadata) = filter.strip_prefix("meta:") {
            let Some((key, value)) = metadata.split_once('=') else {
                return false;
            };
            return chunk
                .metadata
                .get(key)
                .is_some_and(|stored| stored == value);
        }
        true
    })
}

pub(super) fn evidence_role_for_chunk(chunk: &Chunk) -> EvidenceRole {
    if metadata_truthy(&chunk.metadata, "outdated") {
        return EvidenceRole::Outdated;
    }
    if let Some(role) = chunk
        .metadata
        .get("evidence_role")
        .and_then(|value| parse_evidence_role(value))
    {
        return role;
    }
    if is_low_trust_chunk(chunk) {
        EvidenceRole::LowTrust
    } else {
        EvidenceRole::Supporting
    }
}

fn parse_evidence_role(value: &str) -> Option<EvidenceRole> {
    match normalize_term(value).as_str() {
        "supporting" | "support" => Some(EvidenceRole::Supporting),
        "contradicting" | "contradiction" | "negative" => Some(EvidenceRole::Contradicting),
        "contextual" | "context" => Some(EvidenceRole::Contextual),
        "procedural" | "procedure" | "runbook" => Some(EvidenceRole::Procedural),
        "definition" | "definitional" => Some(EvidenceRole::Definition),
        "example" | "sample" => Some(EvidenceRole::Example),
        "outdated" | "stale" | "superseded" => Some(EvidenceRole::Outdated),
        "low trust" | "untrusted" => Some(EvidenceRole::LowTrust),
        _ => None,
    }
}

fn metadata_truthy(metadata: &BTreeMap<String, String>, key: &str) -> bool {
    metadata
        .get(key)
        .is_some_and(|value| matches!(normalize_term(value).as_str(), "true" | "yes" | "1"))
}

fn is_low_trust_chunk(chunk: &Chunk) -> bool {
    metadata_truthy(&chunk.metadata, "low_trust")
        || chunk
            .metadata
            .get("trust")
            .is_some_and(|value| matches!(normalize_term(value).as_str(), "low" | "untrusted"))
        || matches!(chunk.taint, EvidenceTaint::Web)
}

pub(super) fn is_stop_word(term: &str) -> bool {
    matches!(
        term,
        "a" | "an"
            | "and"
            | "are"
            | "as"
            | "be"
            | "by"
            | "for"
            | "from"
            | "has"
            | "have"
            | "in"
            | "is"
            | "it"
            | "of"
            | "on"
            | "or"
            | "that"
            | "the"
            | "this"
            | "to"
            | "with"
    )
}

pub(super) fn normalize_term(input: &str) -> String {
    tokenize(input).join(" ")
}

pub(super) fn count_terms(terms: Vec<String>) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for term in terms {
        *counts.entry(term).or_insert(0) += 1;
    }
    counts
}

pub(super) fn bm25_component(
    term_frequency: usize,
    document_frequency: usize,
    document_count: usize,
    chunk_terms: usize,
    avg_terms_per_chunk: f32,
) -> f32 {
    if term_frequency == 0 || document_frequency == 0 || document_count == 0 {
        return 0.0;
    }
    let k1 = 1.2;
    let b = 0.75;
    let documents = usize_to_f32(document_count);
    let frequency = usize_to_f32(document_frequency);
    let term_count = usize_to_f32(term_frequency);
    let chunk_len = usize_to_f32(chunk_terms);
    let avg_len = avg_terms_per_chunk.max(1.0);
    let idf = ((documents - frequency + 0.5) / (frequency + 0.5)).ln_1p();
    let denominator = term_count + k1 * (1.0 - b + b * chunk_len / avg_len);
    idf * (term_count * (k1 + 1.0) / denominator)
}

pub(super) fn cosine(left: &[f32], right: &[f32]) -> f32 {
    if left.len() != right.len() || left.is_empty() {
        return 0.0;
    }
    let dot = left
        .iter()
        .zip(right.iter())
        .map(|(left_value, right_value)| left_value * right_value)
        .sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    if left_norm == 0.0 || right_norm == 0.0 {
        return 0.0;
    }
    dot / (left_norm * right_norm)
}

pub(super) fn normalize_vector(vector: &mut [f32]) {
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm == 0.0 {
        return;
    }
    for value in vector {
        *value /= norm;
    }
}

pub(super) fn bucket_for(term: &str, dimensions: usize) -> usize {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    term.hash(&mut hasher);
    let dimensions_u64 = u64::try_from(dimensions).unwrap_or(u64::MAX).max(1);
    usize::try_from(hasher.finish() % dimensions_u64).unwrap_or(0)
}

pub(super) fn graph_hint_score(query: &str, chunk: &Chunk) -> f32 {
    let query_terms: BTreeSet<String> = tokenize(query).into_iter().collect();
    let title_terms: BTreeSet<String> = chunk
        .source_title
        .as_deref()
        .map_or_else(BTreeSet::new, |title| tokenize(title).into_iter().collect());
    if query_terms.is_disjoint(&title_terms) {
        0.0
    } else {
        0.2
    }
}

pub(super) fn rerank_score(candidate: &Candidate) -> f32 {
    let mut score = candidate.scores.best();
    if candidate.flags.contains(&CandidateFlag::InstructionalText) {
        score *= 0.6;
    }
    score.clamp(0.0, 1.0)
}

pub(super) fn has_instructional_text(text: &str) -> bool {
    let normalized = text.to_lowercase();
    [
        "ignore previous",
        "system prompt",
        "developer message",
        "print secrets",
        "reveal secrets",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

pub(super) fn stable_index_version(chunks: &[Chunk]) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for chunk in chunks {
        chunk.id.hash(&mut hasher);
        chunk.source_uri.hash(&mut hasher);
        chunk.span.hash(&mut hasher);
        chunk.text.hash(&mut hasher);
    }
    format!("idx-{:016x}", hasher.finish())
}

pub(super) fn ratio(numerator: usize, denominator: usize) -> f32 {
    if denominator == 0 {
        return 0.0;
    }
    usize_to_f32(numerator) / usize_to_f32(denominator)
}

pub(super) fn usize_to_f32(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}
