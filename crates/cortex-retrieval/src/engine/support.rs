use std::collections::BTreeSet;

use cortex_types::{EvidenceItem, EvidenceRole};

use super::{ClaimSupport, ClaimSupportStatus, SupportReport, is_stop_word, ratio, tokenize};

const MIN_CLAIM_TERMS: usize = 2;
const MIN_SUPPORT_OVERLAP: f32 = 0.45;

impl SupportReport {
    #[must_use]
    pub fn from_claims(claims: Vec<ClaimSupport>) -> Self {
        let supported_count = count_status(&claims, ClaimSupportStatus::Supported);
        let contradicted_count = count_status(&claims, ClaimSupportStatus::Contradicted);
        let unsupported_count = count_status(&claims, ClaimSupportStatus::Unsupported);
        let insufficient_count = count_status(&claims, ClaimSupportStatus::InsufficientEvidence);
        let coverage_score = ratio(supported_count, claims.len());
        Self {
            claims,
            supported_count,
            contradicted_count,
            unsupported_count,
            insufficient_count,
            coverage_score,
        }
    }

    #[must_use]
    pub const fn is_fully_supported(&self) -> bool {
        !self.claims.is_empty()
            && self.contradicted_count == 0
            && self.unsupported_count == 0
            && self.insufficient_count == 0
    }
}

/// Verifies answer claims against retrieved evidence and reports unsupported or
/// contradicted claims.
#[must_use]
pub fn verify_answer_support(answer: &str, evidence: &[EvidenceItem]) -> SupportReport {
    let claims = extract_claims(answer)
        .into_iter()
        .map(|claim| verify_claim_support(&claim, evidence))
        .collect();
    SupportReport::from_claims(claims)
}

/// Verifies one claim against retrieved evidence.
#[must_use]
pub fn verify_claim_support(claim: &str, evidence: &[EvidenceItem]) -> ClaimSupport {
    let terms = meaningful_terms(claim);
    if terms.len() < MIN_CLAIM_TERMS {
        return claim_result(
            claim,
            ClaimSupportStatus::InsufficientEvidence,
            EvidenceMatches::default(),
        );
    }
    let matches = collect_evidence_matches(&terms, evidence);
    let status = claim_status(&matches, evidence.is_empty());
    claim_result(claim, status, matches)
}

#[derive(Debug, Default)]
struct EvidenceMatches {
    supporting: Vec<String>,
    contradicting: Vec<String>,
    contextual: Vec<String>,
    support_score: f32,
    contradiction_score: f32,
}

fn collect_evidence_matches(
    terms: &BTreeSet<String>,
    evidence: &[EvidenceItem],
) -> EvidenceMatches {
    let mut matches = EvidenceMatches::default();
    for item in evidence {
        let overlap = overlap_score(terms, &item.text);
        if !has_enough_overlap(overlap, terms.len()) {
            continue;
        }
        record_evidence_match(&mut matches, item, overlap);
    }
    matches
}

fn record_evidence_match(matches: &mut EvidenceMatches, item: &EvidenceItem, overlap: f32) {
    if item.is_negative() || contains_contradiction_marker(&item.text) {
        matches.contradiction_score = matches.contradiction_score.max(overlap);
        matches.contradicting.push(item.id.clone());
    } else if is_contextual_evidence(item) {
        matches.contextual.push(item.id.clone());
    } else {
        matches.support_score = matches.support_score.max(overlap);
        matches.supporting.push(item.id.clone());
    }
}

const fn claim_status(matches: &EvidenceMatches, evidence_empty: bool) -> ClaimSupportStatus {
    if !matches.contradicting.is_empty() {
        ClaimSupportStatus::Contradicted
    } else if !matches.supporting.is_empty() {
        ClaimSupportStatus::Supported
    } else if evidence_empty {
        ClaimSupportStatus::InsufficientEvidence
    } else {
        ClaimSupportStatus::Unsupported
    }
}

fn claim_result(claim: &str, status: ClaimSupportStatus, matches: EvidenceMatches) -> ClaimSupport {
    let confidence = match status {
        ClaimSupportStatus::Supported => matches.support_score,
        ClaimSupportStatus::Contradicted => matches.contradiction_score,
        ClaimSupportStatus::Unsupported | ClaimSupportStatus::InsufficientEvidence => 0.0,
    };
    ClaimSupport {
        claim: claim.trim().to_owned(),
        status,
        supporting_evidence: matches.supporting,
        contradicting_evidence: matches.contradicting,
        contextual_evidence: matches.contextual,
        support_score: matches.support_score,
        contradiction_score: matches.contradiction_score,
        confidence,
    }
}

fn count_status(claims: &[ClaimSupport], status: ClaimSupportStatus) -> usize {
    claims.iter().filter(|claim| claim.status == status).count()
}

fn extract_claims(answer: &str) -> Vec<String> {
    answer
        .split(['\n', '.', '!', '?', ';', '。', '！', '？'])
        .map(clean_claim_fragment)
        .filter(|claim| meaningful_terms(claim).len() >= MIN_CLAIM_TERMS)
        .collect()
}

fn clean_claim_fragment(fragment: &str) -> String {
    let trimmed = fragment
        .trim()
        .trim_start_matches(['-', '*', '+', '>'])
        .trim();
    let without_number = trimmed
        .trim_start_matches(|character: char| character.is_ascii_digit())
        .trim_start_matches(['.', ')'])
        .trim();
    without_number.to_owned()
}

fn meaningful_terms(input: &str) -> BTreeSet<String> {
    tokenize(input)
        .into_iter()
        .filter(|term| !is_stop_word(term))
        .collect()
}

fn overlap_score(terms: &BTreeSet<String>, text: &str) -> f32 {
    if terms.is_empty() {
        return 0.0;
    }
    let evidence_terms = meaningful_terms(text);
    let hits = terms
        .iter()
        .filter(|term| evidence_terms.contains(*term))
        .count();
    ratio(hits, terms.len())
}

fn has_enough_overlap(overlap: f32, term_count: usize) -> bool {
    overlap >= MIN_SUPPORT_OVERLAP || (term_count <= 3 && overlap >= 0.5)
}

const fn is_contextual_evidence(item: &EvidenceItem) -> bool {
    matches!(
        item.role,
        EvidenceRole::Contextual | EvidenceRole::LowTrust | EvidenceRole::Procedural
    )
}

fn contains_contradiction_marker(text: &str) -> bool {
    let normalized = text.to_lowercase();
    [
        "not ",
        "no longer",
        "does not",
        "do not",
        "cannot",
        "can't",
        "unsupported",
        "deprecated",
        "removed",
        "disabled",
        "replaced by",
        "false",
        "incorrect",
    ]
    .iter()
    .any(|marker| normalized.contains(marker))
}
