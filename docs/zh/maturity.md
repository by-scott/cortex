# 成熟度

Cortex 1.5 是重写线，不是成熟平台宣称。它有意移除旧 runtime 主路径，只
把能在严格 gate 下测试的机制重建回来。

## 已实现

- 多用户 ownership 和 visibility contract。
- Workspace 准入与竞争。
- Memory capture / consolidation records 和 interference reporting。
- RAG authorization、ACL、BM25 scoring、taint blocking、placement。
- Turn planning 和 model-provider usage 保留。
- sessions、memory、permissions、deliveries、token usage 的 SQLite 持久化。
- 使用 SHA-256 bearer-token digest 的 authenticated ingress。
- SDK plugin manifest、ABI、capability、host-path、output-limit 检查。
- Deployment plan ordering、evidence、artifact manifest、rollback action、
  rollback completion state。
- 可复现 release packaging script 和严格 Docker gate。

## 尚未恢复

- Live daemon lifecycle。
- HTTP / WebSocket API。
- Telegram / QQ live clients。
- Browser integration。
- Tool execution engine。
- Native plugin loading。
- 敌对多租户 OS isolation。

1.5 通过减少未文档化 surface 来走向生产就绪，而不是保留旧代码路径。
