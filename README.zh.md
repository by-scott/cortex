# Cortex

**Cortex 1.5 是一次面向生产多用户交付的完整重写。**

这个分支已经把旧的 1.4 运行时主路径删掉。Git 历史仍然是归档；
1.5 的发布路径从一个瘦身 workspace 重新开始，只保留能被直接测试的
机制：

- tenant、actor、client、session、turn、event、delivery、permission、
  corpus 的类型化标识；
- 默认拒绝的 ownership / visibility 检查；
- 有界 workspace 准入、salience / urgency 竞争、广播订阅者和丢弃原因；
- fast capture 与 slow semantic memory consolidation，以及 interference
  report；
- drift 风格 evidence accumulation 与 expected-control-value 决策；
- turn executor：装配 workspace / retrieval / control context，把 retrieved
  material 包裹为 untrusted evidence，调用 provider，并保留 provider 返回的
  token usage；
- RAG query-scope authorization、corpus ACL、BM25 lexical scoring、support
  scoring、放置、taint 阻断和 active retrieval 决策；
- 基于 transport capability 的结构化 outbound delivery，以及按 recipient
  私有持久化的 delivery outbox；
- 按 visibility 过滤 replay 的文件事件日志；
- SQLite state store：schema migration ledger、owner-filtered session query、
  active-session persistence、owner-filtered memory persistence、permission
  request / resolution persistence、owner-filtered delivery outbox record、
  owner-filtered token usage ledger、fixture-backed 1.4 session metadata import；
- 多用户 runtime client binding、first-turn actor session reuse、per-client
  active session、active-session delivery gate，以及这些 binding 的 journal
  recovery；
- authenticated ingress registry：只保存 bearer token 的 SHA-256 digest，
  未认证 client binding 在修改 runtime state 前被拒绝；
- 绑定 request id、owner、private client 的 permission resolution；
- SDK plugin authorization：声明能力、host-path deny、output limit，以及
  manifest ABI validation 和 declared-capability conformance；
- deployment / release 状态机：backup、migration、install、smoke、package、
  publish 全部通过后才允许 release-ready，并记录 evidence、artifact
  manifest、rollback action 与 rollback completion state；
- Telegram / QQ / CLI transport adapter，按各 transport 的 Markdown /
  plain / media capability 渲染 `DeliveryPlan`。

1.5 不接受“形式像论文”。认知科学或 RAG 术语只有在代码和测试里有对应
机制时才允许保留。

## Workspace

| Crate | 职责 |
| --- | --- |
| `cortex-types` | runtime contracts：ownership、workspace、memory、retrieval、control、policy、outbound delivery、events。 |
| `cortex-kernel` | durable substrate primitives。目前是带 visibility-filtered replay 的 file journal。 |
| `cortex-retrieval` | ownership-filtered evidence retrieval 与 placement。 |
| `cortex-turn` | workspace / control / retrieval turn planning。 |
| `cortex-runtime` | 多用户 runtime boundary 与 tenant/session gate。 |
| `cortex-sdk` | capability-first plugin context surface。 |
| `cortex-app` | CLI binary entrypoint。 |

## 当前状态

1.5 还没有达到发布完成状态。旧实现已移出主路径，新 core 刻意保持小，
后续生产机制必须在严格测试下逐块补回，而不是藏在历史模块里。

发布 gate 命令：

```bash
./scripts/gate.sh --docker
```

gate 使用 `rust:latest`、仓库声明的 stable toolchain、0 warning
suppression、`cargo fmt --all --check`、严格 clippy
`-D warnings -W clippy::pedantic -W clippy::nursery`，以及完整 workspace
测试。

## 多用户规则

所有 release-path 对象都必须带 ownership。跨 tenant 或跨 actor 访问必须
在加载、replay、retrieval、delivery 或 mutate 私有状态之前被拒绝。

当前相关测试：

- `crates/cortex-types/tests/mechanisms.rs`
- `crates/cortex-retrieval/tests/rag_pipeline.rs`
- `crates/cortex-kernel/tests/journal.rs`
- `crates/cortex-kernel/tests/sqlite_store.rs`
- `crates/cortex-runtime/tests/multi_user.rs`
- `crates/cortex-runtime/tests/ingress.rs`
- `crates/cortex-runtime/tests/transport.rs`
- `crates/cortex-turn/tests/executor.rs`
- `crates/cortex-sdk/tests/plugin_contract.rs`
- `crates/cortex-types/tests/deployment.rs`

## 发布门槛

Cortex 1.5 不能在以下内容完成前发布：最终公开文档、SDK 发布、binary
artifact upload、tag 与 GitHub release artifacts。
