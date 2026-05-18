use std::collections::BTreeSet;

use chrono::Utc;
use cortex_types::{
    ControlActionCandidate, ControlDecision, ControlSignal, CorrelationId, EffectReversibility,
    Event, EvidenceItem, EvidenceTaint, FrameError, Payload, TurnId, WorkspaceFrame, WorkspaceItem,
    WorkspaceItemKind, WorkspaceLane, WorkspaceTaint, WorkspaceVolatility,
};

use super::{Metrics, Report};

impl Metrics {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            candidate_count: 0,
            evidence_count: 0,
            best_score: 0.0,
            recall_at_k: None,
            reciprocal_rank: None,
        }
    }
}

#[must_use]
pub fn evaluate(evidence: &[EvidenceItem], relevant_chunk_ids: &BTreeSet<String>) -> Metrics {
    if relevant_chunk_ids.is_empty() {
        return Metrics {
            candidate_count: evidence.len(),
            evidence_count: evidence.len(),
            best_score: evidence
                .first()
                .map_or(0.0, |item| item.scores.hybrid().clamp(0.0, 1.0)),
            recall_at_k: None,
            reciprocal_rank: None,
        };
    }

    let mut hits = 0_usize;
    let mut reciprocal_rank = None;
    for (index, item) in evidence.iter().enumerate() {
        if relevant_chunk_ids.contains(&item.chunk_id) {
            hits = hits.saturating_add(1);
            if reciprocal_rank.is_none() {
                reciprocal_rank = Some(1.0 / super::scoring::usize_to_f32(index.saturating_add(1)));
            }
        }
    }

    Metrics {
        candidate_count: evidence.len(),
        evidence_count: evidence.len(),
        best_score: evidence
            .first()
            .map_or(0.0, |item| item.scores.hybrid().clamp(0.0, 1.0)),
        recall_at_k: Some(super::scoring::ratio(hits, relevant_chunk_ids.len())),
        reciprocal_rank,
    }
}

#[must_use]
pub fn control_for_support(report: &Report, min_support: f32) -> ControlDecision {
    let threshold = min_support.clamp(0.0, 1.0);
    if report.evidence.is_empty() {
        return ControlDecision::new(
            ControlSignal::Retrieve,
            "retrieval produced no supporting evidence",
        )
        .with_scores(0.2, 0.8, 0.2, 0.1)
        .with_reversibility(EffectReversibility::Reversible)
        .with_candidate(
            ControlActionCandidate::new(ControlSignal::Retrieve, "run a broader retrieval query")
                .with_scores(0.2, 0.8, 0.2, 0.1)
                .with_reversibility(EffectReversibility::Reversible)
                .with_required_evidence("supporting evidence"),
        )
        .with_rejected_alternative(
            ControlSignal::ContinueTurn,
            "continuing would produce unsupported claims",
        )
        .with_required_evidence("supporting evidence")
        .with_blocking_uncertainty("no evidence was retrieved")
        .with_risk_boundary("answers without evidence must retrieve or ask for help")
        .with_fallback_plan("ask the operator for a source or narrow the question");
    }
    if report.metrics.best_score < threshold {
        return ControlDecision::new(
            ControlSignal::Rerank,
            "retrieval support is below the required threshold",
        )
        .with_scores(report.metrics.best_score, 0.7, 0.2, 0.1)
        .with_reversibility(EffectReversibility::Reversible)
        .with_candidate(
            ControlActionCandidate::new(ControlSignal::Rerank, "rerank available evidence")
                .with_scores(report.metrics.best_score, 0.7, 0.2, 0.1)
                .with_reversibility(EffectReversibility::Reversible)
                .with_required_evidence("higher support score"),
        )
        .with_candidate(
            ControlActionCandidate::new(ControlSignal::Retrieve, "retrieve additional evidence")
                .with_scores(report.metrics.best_score, 0.6, 0.3, 0.1)
                .with_reversibility(EffectReversibility::Reversible),
        )
        .with_rejected_alternative(
            ControlSignal::ContinueTurn,
            "support score is below the configured threshold",
        )
        .with_required_evidence("higher support score")
        .with_blocking_uncertainty(format!(
            "best support {:.2} is below threshold {:.2}",
            report.metrics.best_score, threshold
        ))
        .with_risk_boundary("low-support answers must not continue without rerank or retrieval")
        .with_fallback_plan("ask human if rerank and retrieval both remain insufficient");
    }
    ControlDecision::new(
        ControlSignal::ContinueTurn,
        "retrieval support is sufficient",
    )
    .with_scores(report.metrics.best_score, 0.6, 0.1, 0.1)
    .with_reversibility(EffectReversibility::Reversible)
    .with_candidate(
        ControlActionCandidate::new(ControlSignal::ContinueTurn, "answer with cited evidence")
            .with_scores(report.metrics.best_score, 0.6, 0.1, 0.1)
            .with_reversibility(EffectReversibility::Reversible),
    )
    .with_rejected_alternative(
        ControlSignal::Retrieve,
        "current evidence already satisfies the support threshold",
    )
}

/// Promotes selected retrieved evidence into a workspace frame.
///
/// # Errors
///
/// Returns [`FrameError`] if frame actor scope or budget validation rejects any
/// evidence item.
pub fn promote_evidence(
    report: &Report,
    frame: &mut WorkspaceFrame,
) -> Result<Vec<String>, FrameError> {
    let mut promoted = Vec::new();
    for evidence in &report.evidence {
        let item = workspace_item_for_evidence(evidence);
        let item_id = item.id.clone();
        frame.promote(item)?;
        promoted.push(item_id);
    }
    Ok(promoted)
}

#[must_use]
pub fn report_events(
    turn_id: TurnId,
    correlation_id: CorrelationId,
    report: &Report,
) -> Vec<Event> {
    let mut events = Vec::with_capacity(report.evidence.len().saturating_add(1));
    events.push(Event::new(
        turn_id,
        correlation_id,
        Payload::RetrievalDecisionRecorded {
            decision: report.decision.clone(),
        },
    ));
    events.extend(report.evidence.iter().cloned().map(|evidence| {
        Event::new(
            turn_id,
            correlation_id,
            Payload::EvidenceRetrieved {
                evidence: Box::new(evidence),
            },
        )
    }));
    events
}

#[must_use]
pub fn promotion_events(
    turn_id: TurnId,
    correlation_id: CorrelationId,
    report: &Report,
    promoted_item_ids: &[String],
) -> Vec<Event> {
    report
        .evidence
        .iter()
        .zip(promoted_item_ids)
        .map(|(evidence, frame_item_id)| {
            Event::new(
                turn_id,
                correlation_id,
                Payload::EvidencePromoted {
                    evidence_id: evidence.id.clone(),
                    frame_item_id: frame_item_id.clone(),
                },
            )
        })
        .collect()
}

fn workspace_item_for_evidence(evidence: &EvidenceItem) -> WorkspaceItem {
    WorkspaceItem {
        id: format!("evidence:{}", evidence.id),
        kind: WorkspaceItemKind::RetrievalEvidence,
        lane: Some(WorkspaceLane::Evidence),
        content: evidence.text.clone(),
        owner_actor: evidence.visibility_actor.clone(),
        session_id: None,
        provenance: evidence.provenance.clone(),
        taint: WorkspaceTaint::Retrieved,
        activation: evidence.scores.hybrid(),
        utility: evidence.scores.hybrid(),
        risk: evidence_risk(evidence),
        volatility: WorkspaceVolatility::Turn,
        estimated_tokens: estimate_tokens(&evidence.text),
        evidence_ref: Some(evidence.id.clone()),
        binding_group: Some(evidence.corpus_id.clone()),
        expires_at: None,
        promoted_at: Utc::now(),
        promotion_reason: "retrieval evidence selected for this workspace frame".to_string(),
    }
}

fn estimate_tokens(text: &str) -> usize {
    text.chars().count().saturating_add(3) / 4
}

const fn evidence_risk(evidence: &EvidenceItem) -> f32 {
    match evidence.taint {
        EvidenceTaint::TrustedCorpus => 0.0,
        EvidenceTaint::UserCorpus => 0.05,
        EvidenceTaint::ExternalCorpus | EvidenceTaint::ToolOutput | EvidenceTaint::Web => 0.20,
    }
}
