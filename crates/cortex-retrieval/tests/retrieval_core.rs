use std::collections::BTreeSet;

use cortex_retrieval::{
    ChunkingPolicy, Document, Engine, HashDenseEncoder, Index, RerankPolicy, verify_answer_support,
};
use cortex_types::retrieval::QueryPlan;

#[test]
fn retrieval_promotes_visible_supporting_evidence() -> Result<(), cortex_retrieval::Error> {
    let encoder = HashDenseEncoder::default();
    let documents = vec![
        Document::new(
            "manual",
            "runtime",
            "memory://runtime",
            "The Cortex daemon serves HTTP RPC and the embedded dashboard.",
            "local:default",
        )
        .public()
        .with_title("Runtime"),
        Document::new(
            "manual",
            "private",
            "memory://private",
            "Only another actor can read this private chunk.",
            "remote:actor",
        ),
    ];
    let index = Index::build(&documents, ChunkingPolicy::fixed(200, 0), &encoder)?;
    let engine = Engine::new(index, encoder, RerankPolicy::strict(3, 0.01, 200));
    let report = engine.search(&QueryPlan::hybrid("daemon dashboard rpc", "local:default"))?;

    assert_eq!(report.evidence.len(), 1);
    assert!(report.evidence[0].text.contains("embedded dashboard"));
    assert_eq!(report.evidence[0].corpus_id, "manual");
    Ok(())
}

#[test]
fn support_verifier_flags_contradictions() {
    let evidence = vec![cortex_types::EvidenceItem::new(
        "ev1",
        "manual",
        "chunk1",
        "memory://manual",
        "The daemon does not expose the operator dashboard remotely.",
        "local:default",
    )];

    let report = verify_answer_support(
        "The daemon exposes the operator dashboard remotely.",
        &evidence,
    );

    assert_eq!(report.contradicted_count, 1);
    assert!(!report.is_fully_supported());
}

#[test]
fn retrieval_metrics_report_recall_and_rank() {
    let evidence = vec![
        cortex_types::EvidenceItem::new("ev1", "manual", "chunk-a", "memory://a", "alpha", "actor"),
        cortex_types::EvidenceItem::new("ev2", "manual", "chunk-b", "memory://b", "beta", "actor"),
    ];
    let relevant = BTreeSet::from(["chunk-b".to_string()]);
    let metrics = cortex_retrieval::evaluate(&evidence, &relevant);

    assert_eq!(metrics.recall_at_k, Some(1.0));
    assert_eq!(metrics.reciprocal_rank, Some(0.5));
}
