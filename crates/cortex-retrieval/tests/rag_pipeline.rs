use cortex_retrieval::RetrievalEngine;
use cortex_types::{
    AccessClass, ActorId, AuthContext, ClientId, CorpusId, Evidence, OwnedScope, QueryPlan,
    RetrievalDecision, TenantId, Visibility,
};

fn context(tenant: &'static str, actor: &'static str, client: &'static str) -> AuthContext {
    AuthContext::new(
        TenantId::from_static(tenant),
        ActorId::from_static(actor),
        ClientId::from_static(client),
    )
}

fn query(context: &AuthContext, corpus_id: CorpusId, query: &'static str) -> QueryPlan {
    QueryPlan {
        query: query.to_string(),
        scope: OwnedScope::new(
            context.tenant_id.clone(),
            context.actor_id.clone(),
            Some(context.client_id.clone()),
            Visibility::ActorShared,
        ),
        corpus_id,
        active_retrieval: true,
    }
}

#[test]
fn bm25_lexical_scoring_ranks_matching_evidence() {
    let owner = context("tenant-a", "alice", "cli");
    let corpus = CorpusId::from_static("corpus-rag");
    let mut engine = RetrievalEngine::default().with_threshold(0.2);
    engine.ingest(Evidence::new(
        "journal",
        OwnedScope::private_for(&owner),
        corpus.clone(),
        "https://docs.invalid/journal",
        "Cortex uses an append only journal for durable replay and recovery.",
    ));
    engine.ingest(Evidence::new(
        "unrelated",
        OwnedScope::private_for(&owner),
        corpus.clone(),
        "https://docs.invalid/media",
        "Image and audio transports have separate capability descriptors.",
    ));

    let result = engine.retrieve(
        &query(&owner, corpus, "durable journal replay recovery"),
        &owner,
    );

    assert_eq!(result.decision, RetrievalDecision::Sufficient);
    assert_eq!(result.evidence[0].id, "journal");
    assert!(result.evidence[0].scores.lexical > result.evidence[1].scores.lexical);
}

#[test]
fn private_corpus_evidence_is_blocked_for_other_actor() {
    let owner = context("tenant-a", "alice", "cli");
    let other = context("tenant-a", "bob", "cli");
    let corpus = CorpusId::from_static("corpus-private");
    let mut engine = RetrievalEngine::default();
    engine.ingest(
        Evidence::new(
            "private-note",
            OwnedScope::private_for(&owner),
            corpus.clone(),
            "file://tenant-a/private-note",
            "Alice private deployment token rotation notes.",
        )
        .with_access(AccessClass::Private),
    );

    let result = engine.retrieve(&query(&other, corpus, "token rotation"), &other);

    assert_eq!(result.decision, RetrievalDecision::BlockedByAccess);
    assert!(result.evidence.is_empty());
}

#[test]
fn forged_query_scope_is_rejected_before_corpus_loading() {
    let owner = context("tenant-a", "alice", "cli");
    let other = context("tenant-b", "mallory", "api");
    let corpus = CorpusId::from_static("corpus-forged");
    let mut engine = RetrievalEngine::default();
    engine.ingest(Evidence::new(
        "tenant-note",
        OwnedScope::private_for(&owner),
        corpus.clone(),
        "file://tenant-a/note",
        "Tenant scoped material must not be loaded for a foreign query plan.",
    ));

    let result = engine.retrieve(&query(&owner, corpus, "tenant scoped material"), &other);

    assert_eq!(result.decision, RetrievalDecision::BlockedByAccess);
    assert!(result.evidence.is_empty());
}
