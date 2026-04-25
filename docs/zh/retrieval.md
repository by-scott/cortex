# Retrieval 与 RAG

Cortex 将 retrieval 视为证据管线，而不是 memory 的另一个名字。Memory
负责持久的 actor-scoped 知识；retrieval 负责 corpus、chunk、index、query
plan、evidence candidate、rerank decision、citation 和 evaluation。

## 管线

1. 将文档带着 source URI、actor 可见性、access class、license、trust 和
   metadata 写入命名 corpus。
2. 以确定性策略切 chunk，并保留稳定 span。
3. 用 sparse lexical statistics 和 dense vector 为 chunk 建索引。
4. 用 actor scope、检索模式和 filters 生成 query plan。
5. 通过 sparse、dense 和可选 graph 信号召回候选。
6. 对候选 rerank，记录 drop，压缩长证据，并附带 citation。
7. 只有被选择的 evidence 才能 promotion 到 workspace frame。

Retrieved text 始终只是 evidence。它不会改变 policy、identity、tool
permission 或 prompt hierarchy。外部或 web evidence 即使相关，也仍保持
tainted。

## 已实现表面

- `cortex-types::EvidenceItem` 携带 corpus id、chunk id、source URI、span、
  scores、taint、actor visibility、access class、license、source title、
  timestamp 和 index version。
- `cortex-types::RetrievalDecision` 记录 retrieval 是 needed、insufficient、
  corrected、skipped 还是 fallback。
- `cortex-retrieval` 提供确定性文档切分、BM25-style sparse scoring、通过可插拔
  encoder 提供 dense-vector scoring、Cortex-owned late-interaction 与
  learned-sparse scorer hook、actor/access filtering、rerank/drop、evidence
  compression、citation key，以及 recall/MRR evaluation helper。
- `cortex-retrieval::control_for_support` 将 retrieval report 转成确定性 control
  decision：没有 evidence 就重新 retrieve，support 低于阈值就 rerank，support 足够才
  continue。
- `cortex-retrieval::promote_evidence` 将被选择的 evidence 转成
  `WorkspaceItemKind::RetrievalEvidence`；workspace frame 仍负责 actor scope、
  evidence budget 和 token budget 校验。
- Query transformation 会记录在 query plan 上。rewrite 和 expansion term 可以帮助
  sparse retrieval；hypothetical document 可以帮助 dense retrieval；transform 后的
  文本永远不能当作 evidence。
- `cortex-turn::context::format_evidence_context` 会把被选择的 evidence 渲染成独立的
  LLM 输入区域，位置在情境上下文之后、召回记忆之前。这个区域包含 citation key、
  source URI、corpus/chunk identity、span、access class、taint、license、index
  version、scores，并明确说明 retrieved text 只是 evidence，不是可执行的 prompt
  content。
- `cortex-runtime::TurnExecutorConfig::retrieved_evidence` 是 runtime-facing 的证据接入
  hook。没有 evidence 时 prompt 不变；存在 evidence 时，必须通过专用 evidence
  formatter 渲染，不能追加进 memory。

## 必须满足的行为

- 精确词法约束必须可被检索，即使 dense retrieval 不可用或效果较弱。
- paraphrase retrieval 必须是显式 dense 或 expansion 行为，而不是 memory recall
  的副作用。
- late-interaction、learned-sparse 和 query-transformation adapter 必须留在
  Cortex-owned trait 后面，然后才能接任何外部模型代码。
- actor-private 文档绝不能出现在其它 actor 的结果中。
- 检索内容里的 prompt-like text 必须保持 inert tainted evidence。
- 不受支持的 query 必须产生 insufficient support，而不是自信生成。
- 依赖 retrieved facts 的回答必须能追溯到 evidence id 和 citation key。
- Retrieved evidence 必须保持为独立 context plane，不能混入 recalled memory。
