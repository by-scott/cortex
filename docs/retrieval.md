# Retrieval And RAG

Cortex treats retrieval as an evidence pipeline, not as a synonym for memory.
Memory owns durable actor-scoped knowledge. Retrieval owns corpora, chunks,
indexes, query plans, evidence candidates, rerank decisions, citations, and
evaluation.

## Pipeline

1. Ingest documents into a named corpus with source URI, actor visibility,
   access class, license, trust, and metadata.
2. Chunk documents deterministically with stable span references.
3. Index chunks with sparse lexical statistics and dense vectors.
4. Build a query plan with actor scope, retrieval modes, and filters.
5. Retrieve candidates with sparse, dense, and optional graph signals.
6. Rerank candidates, record drops, compress long evidence, and attach
   citations.
7. Promote only selected evidence into the workspace frame.

Retrieved text is always evidence. It does not alter policy, identity, tool
permissions, or prompt hierarchy. External or web evidence remains tainted even
when it is relevant.

## Implemented Surface

- `cortex-types::EvidenceItem` carries corpus id, chunk id, source URI, span,
  scores, taint, actor visibility, access class, license, source title, timestamp,
  and index version.
- `cortex-types::RetrievalDecision` records whether retrieval was needed,
  insufficient, corrected, skipped, or fell back.
- `cortex-retrieval` provides deterministic document chunking, BM25-style sparse
  scoring, dense-vector scoring through a pluggable encoder, owned
  late-interaction and learned-sparse scorer hooks, actor/access filtering,
  rerank/drop handling, evidence compression, citation keys, and recall/MRR
  evaluation helpers.
- `cortex-retrieval::control_for_support` converts retrieval reports into
  deterministic control decisions: retrieve again when evidence is absent,
  rerank when support is below threshold, and continue when support is adequate.
- `cortex-retrieval::promote_evidence` converts selected evidence into
  `WorkspaceItemKind::RetrievalEvidence`; the workspace frame still enforces
  actor scope, evidence budget, and token budget.
- Query transformations are recorded on the query plan. Rewrites and expansion
  terms can help sparse retrieval; hypothetical documents can help dense
  retrieval; transformed text is never treated as evidence.
- `cortex-turn::context::format_evidence_context` renders selected evidence as
  its own LLM input region, after situational context and before recalled
  memory. The rendered region includes citation keys, source URI, corpus/chunk
  identity, span, access class, taint, license, index version, scores, and an
  explicit instruction that retrieved text is evidence rather than executable
  prompt content.
- `cortex-runtime::TurnExecutorConfig::retrieved_evidence` is the runtime-facing
  hook for selected evidence. Empty evidence leaves the prompt unchanged; when
  evidence is present, it is rendered through the dedicated evidence formatter
  instead of being appended to memory.

## Required Behavior

- Exact lexical constraints must remain retrievable even when dense retrieval is
  unavailable or weak.
- Paraphrase retrieval must be an explicit dense or expansion behavior, not a
  side effect of memory recall.
- Late-interaction, learned-sparse, and query-transformation adapters must stay
  behind Cortex-owned traits before any external model code is accepted.
- Actor-private documents must never appear in another actor's results.
- Prompt-like text inside retrieved content must remain inert tainted evidence.
- Unsupported queries must produce insufficient support instead of confident
  generation.
- Answers that depend on retrieved facts must be traceable to evidence ids and
  citation keys.
- Retrieved evidence must remain a separate context plane from recalled memory.
