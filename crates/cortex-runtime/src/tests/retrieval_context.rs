use cortex_retrieval::{ChunkingPolicy, Document, Engine, HashDenseEncoder, Index, RerankPolicy};
use cortex_turn::context::{ContextBuilder, SituationalContext, format_evidence_context};
use cortex_types::RetrievalQueryPlan;

#[test]
fn retrieved_evidence_enters_runtime_context_before_memory() {
    let actor = "actor:runtime-rag";
    let documents = [Document::new(
        "operator-docs",
        "rag-contract",
        "file:///docs/retrieval.md",
        "Cortex renders retrieved evidence in a dedicated evidence plane before recalled memory.",
        actor,
    )
    .with_title("Retrieval Contract")
    .with_license("Apache-2.0")];
    let encoder = HashDenseEncoder::default();
    let index = Index::build(&documents, ChunkingPolicy::fixed(240, 0), &encoder)
        .unwrap_or_else(|err| panic!("index should build: {err:?}"));
    let engine = Engine::new(index, encoder, RerankPolicy::strict(3, 0.01, 400));
    let plan = RetrievalQueryPlan::hybrid("dedicated evidence plane before memory", actor);
    let report = engine
        .search(&plan)
        .unwrap_or_else(|err| panic!("search should return evidence: {err:?}"));
    let Some(evidence_context) = format_evidence_context(&report.evidence) else {
        panic!("retrieved evidence should render");
    };

    let mut builder = ContextBuilder::new();
    builder.set_situational(SituationalContext::Active {
        phase: "rag-fixture".to_string(),
        goals: "verify runtime context placement".to_string(),
        resume: String::new(),
    });
    builder.set_evidence(evidence_context);
    builder.set_memory("## Memory\nrecalled profile data".to_string());
    let Some(rendered) = builder.build() else {
        panic!("runtime context should render");
    };

    let phase_pos = rendered.find("[Phase: rag-fixture]").unwrap_or(usize::MAX);
    let evidence_pos = rendered.find("## Retrieved Evidence").unwrap_or(usize::MAX);
    let memory_pos = rendered.find("## Memory").unwrap_or(usize::MAX);
    assert!(phase_pos < evidence_pos);
    assert!(evidence_pos < memory_pos);
    assert!(rendered.contains("Citation: file:///docs/retrieval.md#rag-contract:0:chars:0-"));
    assert!(rendered.contains("License: Apache-2.0"));
}
