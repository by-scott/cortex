<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center"><strong>语言模型认知运行时</strong></p>
  <p align="center">
    <a href="https://github.com/by-scott/cortex/releases"><img src="https://img.shields.io/github/v/release/by-scott/cortex?display_name=tag" alt="Release"></a>
    <a href="https://crates.io/crates/cortex-sdk"><img src="https://img.shields.io/crates/v/cortex-sdk" alt="Crates.io"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
  <p align="center">
    <a href="docs/zh/quickstart.md">快速开始</a> ·
    <a href="docs/zh/usage.md">使用指南</a> ·
    <a href="docs/zh/config.md">配置</a> ·
    <a href="docs/zh/plugins.md">插件</a> ·
    <a href="docs/zh/compatibility.md">兼容性</a> ·
    <a href="docs/zh/roadmap.md">路线图</a> ·
    <a href="README.md">English</a>
  </p>
</p>

---

现代 Agent 框架已经将语言模型推进到相当成熟的水平：持久记忆、工具编排、
多步规划、上下文管理在整个生态中都已是日益成熟的能力。Cortex 采取一种
互补的方法：不是临时组装这些能力，而是围绕受认知科学启发的运行时约束
来组织它们。

全局工作空间理论塑造并发模型。互补学习系统启发记忆巩固。元认知冲突
监控成为带有自调阈值的一等子系统，而非日志层。漂移扩散证据累积被近似
为有界置信度追踪器。认知负荷理论驱动分级上下文压力响应。这些是受理论
启发的工程实现，不是形式化认知科学模型。

其结果是一个运行时，目标是帮助语言模型跨时间、跨接口、在压力下维持
连贯的、自校正的、目标导向的行为，同时让关键运行时机制保持显式且可
检查。

## Cortex 是什么

最准确的一句话是：

> Cortex 是一个面向长期运行的本地 Agent runtime，更接近 agent OS 的
> 运行底座，而不是 prompt loop 框架。

Cortex 1.5.0 是面向这个方向的生产 core 基线。它是对活跃源码树的主动重写，
不是对 1.4 daemon 的渐进整理。旧实现仍然保留在 Git 历史里；当前树只保留
足够小、可以直接测试、可以继续重建的机制。

## 当前状态

1.5.0 不是 1.4 所有用户可见功能的完整替代。它交付的是生产级多用户 runtime
所需的 core contract 和发布纪律；更宽的 live surface 要等能落在这些 contract
之上时再恢复。

当前已实现：

- tenant、actor、client、session、turn、event、delivery、permission、
  corpus 的类型化标识；
- 默认拒绝的 ownership / visibility 检查；
- SQLite persistence：migration、session、active session、memory、
  permission、delivery outbox、token usage；
- 带 visibility-filtered replay 的文件事件日志；
- RAG evidence retrieval：query-scope authorization、corpus ACL、BM25
  lexical scoring、placement、taint blocking、support decision；
- turn execution：将 retrieved material 包裹为 untrusted evidence，并保留
  provider 返回的 token usage；
- Telegram、QQ、CLI rendering contract 的结构化 outbound delivery planning；
- 基于 SHA-256 bearer-token digest 的 authenticated client ingress；
- capability-first SDK plugin contract，包括 ABI、declared capability、
  host-path 和 output-limit validation；
- 带有有序 release step、evidence、artifact、rollback state 的 deployment
  planning；
- 通过 `scripts/cortex.sh` 安装和更新 release binary，并校验 checksum。

1.5.0 活跃路径里尚未恢复：

- 长驻 daemon 和 systemd service setup flow；
- HTTP、WebSocket、JSON-RPC、MCP、ACP、Telegram、QQ、browser live client；
- live tool execution、media tool、native plugin loading 和旧 skills registry；
- 旧 1.4 prompt、memory、task、audit、channel、orchestration 模块。

这些能力只能在新的 ownership、retrieval、persistence、delivery 和 strict-gate
contract 之上恢复。

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

## 设计规则

1.5 不接受“形式像论文”。认知科学或 RAG 术语只有在代码和测试里有对应
机制时才允许保留。

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

## 质量门槛

发布 gate 命令：

```bash
./scripts/gate.sh --docker
```

gate 使用 `rust:latest`、仓库声明的 stable toolchain、0 warning
suppression、`cargo fmt --all --check`、严格 clippy
`-D warnings -W clippy::pedantic -W clippy::nursery`，以及完整 workspace
测试。

## 发布

Cortex 1.5.0 已完成 SDK crate、tag、GitHub release、Linux binary artifact、
checksum 和严格 Docker gate。后续 1.5.x 只能在新的 ownership、retrieval、
persistence、delivery 和 gate contract 之上恢复用户可见运行时功能。
