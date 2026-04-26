# 路线图

1.5 线是完整重写，目标是生产就绪、多用户 ownership，以及移除历史漂移。

## 重写核心已完成

- delete-first workspace replacement。
- ownership、visibility、authenticated ingress。
- Journal replay 和 SQLite state recovery。
- RAG evidence 作为独立 runtime object，不混同 memory。
- provider token usage contract 和 usage ledger。
- SDK manifest / ABI / capability conformance。
- delivery planning、transport rendering、per-recipient outbox records。
- deployment evidence、artifacts、rollback actions、rollback completion。
- release packaging script。

## 1.5 发布前剩余

- 最终公开文档审阅。
- Docker Hub metadata 可达后跑官方 Docker gate。
- SDK publish。
- Git tag。
- GitHub release，包含 binary 和 checksum assets。

## 1.5 之后

daemon lifecycle、HTTP API、live channels、tool execution、native plugin
loading 必须在每条边界都有 ownership、authorization、recovery、failure
behavior 测试后再重建。
