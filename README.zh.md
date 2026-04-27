<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center"><strong>面向语言模型的认知运行时 Harness</strong></p>
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

Cortex 是一个本地优先的语言模型认知 Harness。它以 daemon 形式运行，为面向模型的应用提供一套受控运行面：持久状态、Actor 级身份、工具执行、记忆、检索、频道投递、策略检查、重放和操作员可观测性。

主流生态已经转向 harness：成熟的编码助手和运行时产品都在把模型、工具、文件、终端、记忆、评审循环和策略放到同一个操作面中。Cortex 选择的是本地 daemon 路线，面向需要在真实工具和真实接口中评估、演练、加固模型行为的操作员，并强调检查、重放、Actor 边界和可审计控制。

Cortex 中的认知科学术语用于命名已经实现的运行时机制。Global Workspace Theory 对应调度与注意力；Complementary Learning Systems 对应记忆巩固；冲突监控、漂移扩散置信度和认知负荷处理，分别落到阈值、证据累积、上下文预算和调度决策上。这些机制是工程模型，不是生物认知模型。

## Cortex 提供什么

- 跨 CLI、HTTP、socket、Telegram、QQ、MCP、ACP 的长期会话。
- 面向会话、记忆、任务、审计数据和频道绑定的 Actor 级身份边界。
- 带有来源、信任、归属、冲突链接、使用结果和图关系的持久记忆。
- 与长期记忆分离的 RAG 证据：可引用、可限定作用域、可标记污染、可重排、可压缩。
- 带有 effect 声明、风险策略、确认、预览、校验和提交记录的工具执行。
- 面向进程隔离 JSON 工具和强信任 native ABI 扩展的插件治理。
- 重放、审计、operator dashboard、timeline 检查和严格发布验证。

Cortex 不是托管式多租户平台。它是一个本地 daemon 和 Rust workspace，用于在显式控制下运行语言模型行为。

## 安装

前置条件：

- Linux x86_64
- systemd
- 一个 LLM provider key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

管理 daemon：

```bash
cortex start
cortex status
cortex restart
cortex stop
```

运行 Cortex：

```bash
cortex                            # REPL
cortex "总结这个项目"             # 单轮调用
echo "数据" | cortex "总结"        # 管道输入
cortex --mcp-server               # MCP server
```

完整首次使用流程见[快速开始](docs/zh/quickstart.md)。

## 架构

Cortex 分为三个协作平面。

| 平面 | 职责 |
|------|------|
| **Substrate** | 持久运行时状态、Journal、重放、记忆、检索、策略、风险和调度。 |
| **Executive** | Prompt 装配、运行时策略上下文、元认知协议、bootstrap/resume 上下文和 Skill 激活。 |
| **Repertoire** | Skills、学习到的行为模式、执行 trace、效用追踪和热重载行为库。 |

Substrate 是 Rust 运行时表面，包含 SQLite WAL 持久化、blob 外部化、类型化事件、Actor 级存储、工具注册表、模型路由、策略模拟和重放投影。

Executive 根据持久 Prompt 文件、运行时策略、Skill 摘要、retrieved evidence、召回记忆、工具 schema、推理状态和历史消息构建模型输入。能力事实始终以运行时 schema 为准。

Repertoire 保存可执行行为。`deliberate`、`diagnose`、`review`、`orient`、`plan` 等系统 Skill 可以由输入模式、上下文压力、事件、元认知警报或运行时判断触发。

## 运行时表面

Cortex 将关键运行时行为做成显式、可测试的契约：

- 事件 Journal 当前记录 87 种事件变体，覆盖消息、Turn、工具、权限、重放 checkpoint、外部化 payload、检索、workspace、guardrail 和调度事件。
- 10 态 Turn 状态机管理 idle、processing、tool wait、permission wait、human-input wait、compaction、consolidation、completion、interruption 和 suspension。
- Journaled turns and replay 包含 compaction boundary、副作用替换和 replay digest。
- 记忆召回在六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）。
- 三个注意力通道（Foreground、Maintenance、Emergency）以反饥饿策略调度工作。
- 五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）监控运行时健康。
- Workspace admission 记录 lane、utility、risk、volatility、taint、marginal utility、预算影响、admission decision 和 eviction。
- 模型路由使用能力画像，覆盖 coding、long context、vision、tool use、JSON reliability、latency、cost、safety 和 reasoning depth。

## 权限与风险

默认权限模式是 `balanced`。

| 模式 | 行为 |
|------|------|
| `strict` | 只有 `Allow` 决策可以无确认执行。 |
| `balanced` | `Allow` 直接执行；`Review` 及以上需要确认。 |
| `open` | 非阻断工具无需确认。只应在可信的单用户机器上使用。 |

切换权限模式：

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

执行前检查策略决策：

```bash
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

未知插件和 MCP 工具默认按保守风险评分处理，并需要确认。

## 检索与记忆

Cortex 将 retrieved evidence 与 durable memory 分开处理。

检索材料进入 corpus 后会被切分为 chunk，计算 sparse/dense 分数，经过 Actor 与访问权限过滤，再进行 rerank、压缩、引用和 evidence role 分类，最后作为惰性证据进入 Prompt。检索内容中的指令不能变成运行时指令。专用检索 crate 是 `cortex-retrieval`。

记忆是长期运行时状态。每条记忆都记录 owner actor、evidence、trust、status、contradiction link、validity window、usage outcome 和 graph relationship。只有在证据和冲突规则允许时，记忆才会从 captured fact 进入 stabilized belief。

## 接口

| 接口 | 表面 |
|------|------|
| CLI | `cortex`、`cortex start`、`cortex status`、`cortex restart`、`cortex stop` |
| HTTP | `POST /api/turn/stream`、operator status、health、metrics、dashboard |
| JSON-RPC | Unix socket、WebSocket、stdio、HTTP |
| Channels | Telegram、QQ、WhatsApp |
| MCP | `cortex --mcp-server` |
| ACP | `cortex --acp` |

Actor 身份会跨 transport 归一化。已配对的 Telegram 或 QQ 用户可以共享同一个 Actor，但不会自动订阅无关会话。配对本身不创建会话；审批后的第一条真实消息会复用该 Actor 的可见会话，如果没有可见会话才创建新会话。

## 插件

Cortex 支持两种插件边界：

- **Process JSON**：默认外部边界。工具在 `manifest.toml` 中声明，并以子进程方式通过 stdin/stdout JSON 调用。
- **Trusted native ABI**：低延迟进程内扩展，基于 `cortex-sdk` 构建，并通过 `cortex_plugin_init` 导出。

进程隔离命令实现更新会在下一次工具调用生效。共享库代码变更仍需要重启 daemon。

插件 manifest 声明 trust tier、请求的 capability、sandbox profile、package metadata、signature、SBOM/risk-profile 引用、conformance state 和 tool effect。安装前可以审查和测试：

```bash
cortex plugin review <dir>
cortex plugin test <dir>
cortex plugin install <dir-or-package>
```

Rust SDK 独立于 Cortex 内部 crate。它不依赖 `cortex-types`、`cortex-kernel` 或其他 workspace crate；daemon 会在边界处把 SDK DTO 转换为内部运行时类型。

完整流程见[插件开发文档](docs/zh/plugins.md)。

## Crate 结构

```text
cortex-app          CLI、安装、service 命令、插件、频道
cortex-runtime      daemon、HTTP/socket/stdio RPC、会话、频道、dashboard
cortex-turn         Turn 编排、工具、Skills、元认知、上下文装配
cortex-kernel       Journal、重放、记忆、图谱、Prompts、配置、审计
cortex-retrieval    RAG corpus、chunking、hybrid retrieval、支持校验
cortex-types        事件、状态机、配置、信任、策略、安全 DTO
cortex-sdk          独立的强信任 native 插件 SDK
```

## 开发

仓库 Docker 环境是发布验证依据。

```bash
./scripts/gate.sh --docker
```

该命令使用本仓库 `docker-compose.yml` 中的 `dev` service 和 `Dockerfile`。宿主机上的 `cargo` 命令只适合本地排查，不能作为发布通过依据。

发布验证必须同时满足以下条件：

- `cargo fmt --all --check` 无 diff。
- `cargo clippy` 覆盖整个 workspace，并使用 `-D warnings -W clippy::pedantic -W clippy::nursery`，结果为 0 warning。
- `cargo test` 通过整个 workspace。
- 禁止 Rust 警告抑制属性和编译器警告抑制 flag。
- 文档、package surface、secret/path 和 release asset 检查全部通过。

## 文档

- [快速开始](docs/zh/quickstart.md)
- [使用指南](docs/zh/usage.md)
- [配置](docs/zh/config.md)
- [Executive](docs/zh/executive.md)
- [运维](docs/zh/ops.md)
- [插件开发](docs/zh/plugins.md)
- [检索](docs/zh/retrieval.md)
- [成熟度与生产说明](docs/zh/maturity.md)
- [兼容性策略](docs/zh/compatibility.md)
- [测试](docs/testing.md)
- [路线图](docs/zh/roadmap.md)

## 信任边界

Cortex 是本地优先基础设施。Process JSON 是推荐的外部扩展边界。Trusted native ABI 插件运行在 daemon 进程内，必须视为受信任代码。

工具输出进入模型历史前会先记录为不可信外部输入。Guardrails 会分类常见的 prompt injection、system-prompt leakage、role override 和 exfiltration 模式。Policy lint 会拒绝危险组合，例如 open permission 加未审查插件、native 插件缺少显式 risk profile、以及 hostile evidence 自动进入 memory。

Cortex 的目标是让这些边界可见。它不声称能完整隔离敌对租户、不可信 native 代码，或会修改外部系统的工具。

确定性重放会在投影时替换已记录或 provider 提供的副作用值。压缩边界和重放输入都会进入 Journal。

## 许可

[MIT](LICENSE)
