# 路线图

1.5 线是完整重写，目标是生产就绪、多用户 ownership，以及移除历史漂移。

## 重写核心已完成

- delete-first workspace replacement。
- ownership、visibility、authenticated ingress。
- hierarchical control goals、cognitive-load pressure profiles、
  metacognitive monitoring。
- Journal replay 和 SQLite state recovery。
- RAG evidence 作为独立 runtime object，不混同 memory。
- provider token usage contract 和 usage ledger。
- SDK manifest / ABI / capability conformance。
- Unix socket RPC daemon lifecycle：bootstrap、status、send、tenant
  registration、client binding、shutdown、journal recovery、SQLite state
  recovery。
- runtime tool execution：SDK validation、host-granted capability、output
  limit、durable side-effect intent/result record。
- delivery planning、transport rendering、per-recipient outbox records。
- deployment evidence、artifacts、rollback actions、rollback completion。
- release packaging script。

## 1.5 发布前剩余

- 完成仍属于 1.5 的 production daemon operations，尤其是 service installation
  evidence 和 release smoke tests。
- 最终公开文档审阅。
- SDK publish。
- Git tag。
- GitHub release，包含 binary 和 checksum assets。

## 1.5 之后

HTTP API、live channels、media tools、process plugin spawning、native plugin
loading 必须在每条边界都有 ownership、authorization、recovery、failure
behavior 测试后再重建。
