# Retrieval

Cortex 1.5 把 retrieval evidence 作为独立 runtime object，而不是 memory。

当前 1.5 retrieval 切片已经实现：

- `CorpusId` ownership；
- 带 `OwnedScope`、`AccessClass`、`EvidenceTaint`、source URI、text 和
  `HybridScores` 的 `Evidence`；
- corpus loading 前的 query-scope authorization；
- 通过 `AccessClass` 执行 corpus ACL；
- 对候选 evidence 实际计算 BM25 lexical score；
- 基于已计算 lexical、外部 dense、确定性 rerank、citation 四类信号的
  support scoring；
- 对类似 prompt injection 的 retrieved text 做 taint blocking；
- support 不足时进入 active-retrieval decision；
- `PlacementStrategy::Sandwich` 或 front-loaded best evidence placement；
- `cortex-retrieval` 中的 ownership-filtered retrieval。

Evidence 不能隐式变成 durable memory。memory promotion 属于 consolidation
机制，并且必须保留 actor / tenant ownership。

当前测试：

- `crates/cortex-types/tests/mechanisms.rs`
- `crates/cortex-retrieval/tests/rag_pipeline.rs`
- `crates/cortex-runtime/tests/multi_user.rs`
