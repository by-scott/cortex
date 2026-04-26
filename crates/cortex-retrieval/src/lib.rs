#![forbid(unsafe_code)]

use std::collections::{BTreeMap, BTreeSet};

use cortex_types::{
    AccessClass, AuthContext, Evidence, HybridScores, OwnedScope, PlacementStrategy, QueryPlan,
    RetrievalDecision, decide, place,
};

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalEngine {
    evidence: Vec<Evidence>,
    support_threshold: f32,
}

impl Default for RetrievalEngine {
    fn default() -> Self {
        Self {
            evidence: Vec::new(),
            support_threshold: 0.62,
        }
    }
}

impl RetrievalEngine {
    #[must_use]
    pub const fn with_threshold(mut self, support_threshold: f32) -> Self {
        self.support_threshold = support_threshold;
        self
    }

    pub fn ingest(&mut self, evidence: Evidence) {
        self.evidence.push(evidence);
    }

    #[must_use]
    pub fn retrieve(&self, plan: &QueryPlan, context: &AuthContext) -> RetrievalResult {
        if !plan.scope.is_visible_to(context) {
            return RetrievalResult {
                decision: RetrievalDecision::BlockedByAccess,
                evidence: Vec::new(),
            };
        }

        let mut blocked_by_access = false;
        let mut visible = Vec::new();
        for item in self
            .evidence
            .iter()
            .filter(|item| item.corpus_id == plan.corpus_id)
        {
            if access_allows(item, context) {
                visible.push(item.clone());
            } else {
                blocked_by_access = true;
            }
        }
        if visible.is_empty() && blocked_by_access {
            return RetrievalResult {
                decision: RetrievalDecision::BlockedByAccess,
                evidence: Vec::new(),
            };
        }

        let scored = score_evidence(&plan.query, visible);
        let decision = decide(&scored, self.support_threshold);
        RetrievalResult {
            decision,
            evidence: place(scored, PlacementStrategy::Sandwich),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RetrievalResult {
    pub decision: RetrievalDecision,
    pub evidence: Vec<Evidence>,
}

#[must_use]
pub fn scope_matches(left: &OwnedScope, right: &OwnedScope) -> bool {
    left.tenant_id == right.tenant_id && left.actor_id == right.actor_id
}

fn access_allows(evidence: &Evidence, context: &AuthContext) -> bool {
    match evidence.access {
        AccessClass::Public => true,
        AccessClass::Tenant => evidence.scope.tenant_id == context.tenant_id,
        AccessClass::Actor => {
            evidence.scope.tenant_id == context.tenant_id
                && evidence.scope.actor_id == context.actor_id
        }
        AccessClass::Private => evidence.scope.is_visible_to(context),
    }
}

fn score_evidence(query: &str, evidence: Vec<Evidence>) -> Vec<Evidence> {
    let stats = CorpusStats::from_evidence(&evidence);
    let query_terms = unique_terms(tokenize(query));
    evidence
        .into_iter()
        .map(|item| stats.score_item(&query_terms, item))
        .collect()
}

#[derive(Debug, Clone, PartialEq)]
struct CorpusStats {
    documents: usize,
    average_terms: f32,
    document_frequency: BTreeMap<String, usize>,
}

impl CorpusStats {
    fn from_evidence(evidence: &[Evidence]) -> Self {
        let mut document_frequency = BTreeMap::new();
        let mut total_terms = 0_usize;
        for item in evidence {
            let terms = unique_terms(tokenize(&item.text));
            total_terms += terms.len();
            for term in terms {
                *document_frequency.entry(term).or_insert(0) += 1;
            }
        }
        let documents = evidence.len();
        let average_terms = if documents == 0 {
            1.0
        } else {
            bounded_count(total_terms) / bounded_count(documents)
        };
        Self {
            documents,
            average_terms,
            document_frequency,
        }
    }

    fn score_item(&self, query_terms: &[String], mut item: Evidence) -> Evidence {
        let document_terms = tokenize(&item.text);
        let lexical = self.bm25(query_terms, &document_terms);
        let rerank = overlap_score(query_terms, &document_terms);
        item.scores = HybridScores {
            lexical,
            dense: item.scores.dense,
            rerank: item.scores.rerank.max(rerank),
            citation: item.scores.citation.max(citation_score(&item.source_uri)),
        };
        item
    }

    fn bm25(&self, query_terms: &[String], document_terms: &[String]) -> f32 {
        const B: f32 = 0.75;
        const K1: f32 = 1.2;

        if query_terms.is_empty() || document_terms.is_empty() || self.documents == 0 {
            return 0.0;
        }

        let counts = term_counts(document_terms);
        let document_len = bounded_count(document_terms.len());
        let corpus_len = self.average_terms.max(1.0);
        let documents = bounded_count(self.documents);
        let mut raw = 0.0_f32;
        for term in query_terms {
            let Some(frequency) = counts.get(term) else {
                continue;
            };
            let document_frequency =
                bounded_count(*self.document_frequency.get(term).unwrap_or(&0));
            let idf = ((documents - document_frequency + 0.5) / (document_frequency + 0.5)).ln_1p();
            let term_frequency = bounded_count(*frequency);
            let denominator = K1.mul_add(1.0 - B + B * document_len / corpus_len, term_frequency);
            raw += idf * (term_frequency * (K1 + 1.0)) / denominator;
        }
        (raw / (raw + 3.0)).clamp(0.0, 1.0)
    }
}

fn tokenize(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    for character in text.chars().flat_map(char::to_lowercase) {
        if character.is_alphanumeric() {
            current.push(character);
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

fn unique_terms(terms: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    terms
        .into_iter()
        .filter(|term| seen.insert(term.clone()))
        .collect()
}

fn term_counts(terms: &[String]) -> BTreeMap<String, usize> {
    let mut counts = BTreeMap::new();
    for term in terms {
        *counts.entry(term.clone()).or_insert(0) += 1;
    }
    counts
}

fn overlap_score(query_terms: &[String], document_terms: &[String]) -> f32 {
    if query_terms.is_empty() {
        return 0.0;
    }
    let document_terms = document_terms.iter().collect::<BTreeSet<_>>();
    let matches = query_terms
        .iter()
        .filter(|term| document_terms.contains(term))
        .count();
    bounded_count(matches) / bounded_count(query_terms.len())
}

fn citation_score(source_uri: &str) -> f32 {
    if source_uri.starts_with("https://")
        || source_uri.starts_with("http://")
        || source_uri.starts_with("file://")
    {
        1.0
    } else if source_uri.is_empty() {
        0.0
    } else {
        0.5
    }
}

fn bounded_count(value: usize) -> f32 {
    f32::from(u16::try_from(value).unwrap_or(u16::MAX))
}
