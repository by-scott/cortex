# Retrieval

Cortex 1.5 treats retrieval evidence as a separate runtime object, not memory.

The current 1.5 retrieval slice implements:

- `CorpusId` ownership;
- `Evidence` with `OwnedScope`, `AccessClass`, `EvidenceTaint`, source URI,
  text, and `HybridScores`;
- query-scope authorization before corpus loading;
- corpus ACL checks through `AccessClass`;
- real BM25 lexical scoring over candidate evidence;
- support scoring across computed lexical, external dense, deterministic
  rerank, and citation signals;
- taint blocking for prompt-injection-like retrieved text;
- active-retrieval decisions when support is below threshold;
- placement through `PlacementStrategy::Sandwich` or front-loaded best evidence;
- ownership-filtered retrieval in `cortex-retrieval`.

Evidence cannot become durable memory implicitly. Memory promotion belongs to
the consolidation mechanism and must keep actor/tenant ownership.

Current tests:

- `crates/cortex-types/tests/mechanisms.rs`
- `crates/cortex-retrieval/tests/rag_pipeline.rs`
- `crates/cortex-runtime/tests/multi_user.rs`
