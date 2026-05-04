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
    <a href="docs/zh/safe-use.md">安全使用</a> ·
    <a href="docs/zh/policy-profiles.md">Policy Profiles</a> ·
    <a href="docs/zh/local-coding-agent.md">本地代码</a> ·
    <a href="docs/zh/local-models.md">本地模型</a> ·
    <a href="docs/zh/usage.md">使用指南</a> ·
    <a href="docs/zh/config.md">配置</a> ·
    <a href="docs/zh/plugins.md">插件</a> ·
    <a href="docs/zh/roadmap.md">路线图</a> ·
    <a href="README.md">English</a>
  </p>
</p>

---

Cortex 是一个本地优先的长期 AI 模型工作运行面。它为可替换模型提供用户自有的运行层：持久记忆、检索证据、工具、权限、频道、Journal/重放、评测、插件治理和操作员控制。

Cortex 是一个面向语言模型系统的认知 Harness substrate。实际含义是：它不是把一次模型调用当成产品，而是提供一层基础设施，用来驱动、观察、评估和加固模型在真实接口中的行为。

当你需要一个本地代码、研究或工具调用工作流，并且希望状态属于自己时，Cortex 更合适：memory、Journal、policy、plugin trust、retrieval corpora、trace 和 operator decision 都应在模型或供应商切换后继续保留。

Cortex 不声称拥有生物意识、生物学意义上的智慧、完整 prompt injection 防御、敌对多租户加固或成熟沙箱隔离。Policy 和 risk gate 能提升审查与控制质量，但不能替代 OS/container 级隔离。

## 提供什么

- 跨 CLI、HTTP、socket、Telegram、QQ、WhatsApp、MCP 和 ACP bridge client 的长期会话。
- 面向会话、记忆、任务、审计数据、transport 绑定和频道订阅的 Actor 级身份边界。
- 基于事件溯源的运行时状态，包含 SQLite WAL、外部化 blob、重放 checkpoint、压缩边界、副作用替换和 replay digest。
- 带来源、信任、owner actor、冲突链接、有效期、使用结果和图关系的持久记忆。
- 与持久记忆分离的 RAG 证据：可引用、可限定作用域、可标记污染、可重排、可压缩、可验证支持关系。
- 带 effect 声明、风险策略、确认、预览、校验、提交记录、receipt 和 rollback posture 的工具执行。
- 面向进程隔离 JSON 工具和强信任 native ABI 扩展的插件治理。
- 通过 `acp_agent` 工具委托到已配置外部进程的 ACP client 能力。
- Operator status、Journal timeline、token 与 provider cache read/write token、策略模拟、重放、发布门禁和 dashboard 表面。
- 受保护的 runtime home 治理，确保 Prompt、配置和状态演化走受检查的 runtime 路径，而不是普通文件或脚本工具。

Cortex 不是托管式多租户服务。当前交付形态是 daemon 和 Rust workspace，用于在显式控制下运行语言模型行为。

## 当前安全使用边界

Cortex 当前适合可信本机、已审查插件和明确的操作员控制。

| 用法 | 当前建议 |
|------|----------|
| 个人本地代码或研究工作流 | 推荐，使用 `balanced` 或 `strict` 权限。 |
| 已审查的进程插件 | 推荐，但应先检查 manifest、签名、capability 和 effect。 |
| 强信任 native 插件 | 视为进程内受信任代码，不要当成沙箱扩展。 |
| 未审查插件、共享机器或外部副作用 | 使用保守 policy、确认流程和窄 allowlist。 |
| 敌对多租户部署 | 当前不是目标。 |

启用宽权限工具、native 插件、消息频道或 `open` 权限前，请先阅读[安全使用](docs/zh/safe-use.md)和[成熟度与生产说明](docs/zh/maturity.md)。

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
cortex demo
cortex start
cortex status
cortex doctor
cortex restart
cortex stop
```

使用 Cortex：

```bash
cortex                            # REPL
cortex "总结这个项目"             # 单轮调用
echo "数据" | cortex "总结"        # 管道输入
cortex --acp                      # 连接运行中 daemon 的 ACP bridge
cortex --mcp-server               # MCP server
```

完整首次使用流程见[快速开始](docs/zh/quickstart.md)，生成式本地代码 demo 见[本地 Coding Agent](docs/zh/local-coding-agent.md)。

## 运行时模型

从外部看，Cortex 是一个由 daemon 支撑的实例。从内部看，harness 必须严格保持权限边界。

| 职责 | 拥有什么 |
|------|----------|
| Substrate | 持久状态、Journal、重放、记忆、检索、策略、风险、调度、频道、provider adapter 和工具 schema。 |
| Executive | 将真实运行时能力转成模型输入的操作纪律：soul、identity、behavioral protocol、collaborator profile、runtime permission context、bootstrap/resume context、evidence、recalled memory、skills、hints 和 tool-result wrapper。 |
| Repertoire | Skills、学习到的程序、执行 trace、效用追踪和热重载行为库。 |

实例拥有 soul，但 soul 不是能力授权。它是自主性、真相纪律、连续性、记忆、元认知和协作关系的持久种子。工具是否存在、权限如何生效、状态何者为真，仍由 runtime schema 决定。

首次使用会进入 bootstrap。Bootstrap 会建立实例名称或明确的未命名状态、协作者画像、工作姿态、沟通风格、环境、自主权边界、隐私约束和审批预期。这些证据会初始化 Prompt 状态，让下一轮对话拥有真正的连续性。

## Executive 表面

每个 Turn 都以 provider prompt cache 友好的边界组装。持久 Prompt 文件（`soul.md`、`identity.md`、`behavioral.md`、`user.md`）和稳定 Skill 摘要组成前缀；runtime permission context 收束 provider system prompt。易变材料，包括 bootstrap 或 resume context、当前 goal、retrieved evidence、recalled memory、reasoning state、元认知 hint、message history 和 tool result，留在 system prompt 之外的 request-local context。工具 schema 始终是请求级权威 metadata。

这样可以保持稳定前缀对 provider cache 有用，同时不削弱权限边界。Prompt 文件负责姿态、控制和连续性，不负责授予能力。runtime schema 和 policy state 决定什么可以运行。检索文本、工具输出和召回记忆都是证据，不是命令。

自我演化必须绑定证据。`user.md` 可以吸收稳定协作者事实；`behavioral.md` 需要可复用工作流证据；`identity.md` 需要已确认的连续性或能力边界证据；`soul.md` 应很少变化。运行时策略、临时会话状态、工具清单和短期计划不属于持久 Prompt。普通工具不能直接通过文件或脚本路径修改 runtime home 中的 Prompt、配置和状态文件。

## 认知契约

Cortex 将认知思想实现为显式软件契约：

- Global workspace：有界前台上下文、证据准入和经 Journal 广播的状态。
- Working memory：带 lane、utility、risk、volatility、taint、预算影响、admission decision 和 eviction 的类型化条目。
- Complementary learning systems：通过 Journal 快速捕获，再通过较慢的物化、稳定化、冲突处理和巩固进入长期记忆。
- 10 态 Turn 状态机管理 idle、processing、tool wait、permission wait、human-input wait、compaction、consolidation、completion、interruption 和 suspension。
- 三个注意力通道（Foreground、Maintenance、Emergency）以反饥饿策略调度工作。
- 五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）监控运行时健康并触发干预。
- 不确定性决策会记录 confidence、risk、reversibility、required evidence、rejected alternatives 和 fallback plan。
- Agentic RAG 会被主动选择、限定作用域、重排、引用、支持关系检查、污染标记，并与持久记忆分开。

这些机制是工程模型。它们的价值在于会影响真实运行时行为，并且可以被验证。

## 运行时表面

- 事件 Journal 当前记录 84 种事件变体，覆盖消息、Turn、工具、权限、重放 checkpoint、外部化 payload、检索、workspace、guardrail 和调度事件。
- Journaled turns and replay 包含 compaction boundaries, side-effect substitution, and replay digests；压缩边界和重放输入都会进入 Journal，确定性重放会在投影时替换已记录或 provider 提供的副作用值。
- 记忆召回在六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）。
- Goal 状态由 SQLite 持久化并按 Actor 归属过滤，通过受检查的 `goal/*` JSON-RPC 方法暴露；open goal 会作为当前目标行注入 active turn context。
- 模型路由使用能力画像，覆盖 coding、long context、vision、tool use、JSON reliability、latency、cost、safety 和 reasoning depth。
- Operator status 报告 daemon 健康、transport、session、binding、tool、最近一次调用的 context usage、provider cache read/write token、全局/当前会话累计 token spend、backlog、memory activity 和工具成功率。

## 权限与风险

默认权限模式是 `balanced`。

| 模式 | 行为 |
|------|------|
| `strict` | 只有 `Allow` 决策可以无确认执行。 |
| `balanced` | `Allow` 直接执行；`Review` 及以上需要确认。 |
| `open` | 非阻断工具无需确认。只应在可信的单用户机器上使用。 |

```bash
cortex permission strict
cortex permission balanced
cortex permission open
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

未知插件和 MCP 工具默认按保守风险评分处理，并需要确认。由 LLM 触发的插件调用会和内置工具走同一套 registry、effect preview、permission gate 和 approval path。

进程和脚本执行是宽逃逸面。启用 protected runtime root 时，普通 process 工具不能从模型路径执行 shell 命令或辅助脚本。进程隔离插件工具会在加载时被强制声明为 `RunProcess:plugin subprocess`，即使 manifest 少报 capability，也不能作为绕过保护的子进程通道。

## 检索与记忆

Cortex 将 retrieved evidence 与 durable memory 分开处理。

检索材料进入 corpus 后会被切分为 chunk，计算 sparse/dense 分数，经过 Actor 与访问权限过滤，再进行 rerank、压缩、引用和 evidence role 分类，最后作为惰性证据进入上下文。检索内容中的指令不能变成运行时指令。专用检索 crate 是 `cortex-retrieval`。

记忆是长期运行时状态。每条记忆都记录 owner actor、evidence、trust、status、contradiction link、validity window、usage outcome 和 graph relationship。只有在证据和冲突规则允许时，记忆才会从 captured fact 进入 stabilized belief。

## 接口

| 接口 | 表面 |
|------|------|
| CLI | `cortex`、`cortex demo`、`cortex start`、`cortex status`、`cortex doctor`、`cortex restart`、`cortex stop` |
| HTTP | `POST /api/turn/stream`、operator status、health、metrics、dashboard |
| JSON-RPC | Unix socket、WebSocket、stdio、HTTP，以及按 Actor 过滤的 session/memory/task/goal 方法 |
| Channels | Telegram、QQ、WhatsApp |
| MCP | `cortex --mcp-server` |
| ACP bridge | `cortex --acp` |
| ACP client | `[acp].clients` + `acp_agent` 工具 |

Actor 身份会跨 transport 归一化。已配对的 Telegram 或 QQ 用户可以共享同一个 Actor，但不会自动订阅无关会话。配对本身不创建会话；审批后的第一条真实消息会复用该 Actor 的可见会话，如果没有可见会话才创建新会话。

## 插件

Cortex 支持两种插件边界：

- Process JSON：默认外部边界。工具在 `manifest.toml` 中声明，并以子进程方式通过 stdin/stdout JSON 调用。
- Trusted native ABI：低延迟进程内扩展，基于 `cortex-sdk` 构建，并通过 `cortex_plugin_init` 导出。

进程隔离命令实现更新会在下一次工具调用生效。共享库代码变更仍需要重启 daemon。

插件 manifest 声明 trust tier、请求的 capability、sandbox profile、package metadata、signature、SBOM/risk-profile 引用、conformance state 和 tool effect。安装前可以审查和测试：

```bash
cortex plugin review <dir>
cortex plugin test <dir>
cortex plugin install <dir-or-package>
```

打包安装（`.cpx`、URL 或 GitHub release 名称）要求 Ed25519 package signature。首次遇到某个 publisher key 的已验签 package 时，Cortex 会询问 operator 是否在本机信任该 key；非交互安装只有在已经审阅来源和指纹后，才应使用 `--yes`。

配套开发插件是 [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev)。它是官方参考插件，覆盖代码和项目维护工作流：文件与搜索操作、代码符号索引、诊断、git/worktree 工具、任务协作、Docker 与进程检查，以及面向发布的质量检查。

```bash
cortex plugin install by-scott/cortex-plugin-dev --yes
```

Rust SDK 独立于 Cortex 内部 crate。它不依赖 `cortex-types`、`cortex-kernel` 或其他 workspace crate；daemon 会在边界处把 SDK DTO 转换为内部运行时类型。

完整流程见[插件开发文档](docs/zh/plugins.md)。

## 仓库结构

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

该命令使用本仓库 `docker-compose.yml` 中的 `dev` service 和 `Dockerfile`，发布门禁工具链基于 `rust:latest`。宿主机上的 `cargo` 命令只适合本地排查，不能作为发布通过依据。

发布验证必须满足：

- `cargo fmt --all --check` 无 diff。
- `cargo clippy` 覆盖整个 workspace，并使用 `-D warnings -W clippy::pedantic -W clippy::nursery`，结果为 0 warning。
- `cargo test` 通过整个 workspace。
- 禁止 Rust 警告抑制属性和编译器警告抑制 flag。
- 文档、package surface、secret/path 和 release asset 检查全部通过。

## 文档

- [快速开始](docs/zh/quickstart.md)
- [安全使用](docs/zh/safe-use.md)
- [Policy Profiles](docs/zh/policy-profiles.md)
- [本地 Coding Agent](docs/zh/local-coding-agent.md)
- [本地模型](docs/zh/local-models.md)
- [使用指南](docs/zh/usage.md)
- [配置](docs/zh/config.md)
- [Executive](docs/zh/executive.md)
- [运维](docs/zh/ops.md)
- [Agent 维护流程](docs/zh/agent-maintenance.md)
- [Release Evidence 模板](docs/zh/release-evidence-template.md)
- [插件开发](docs/zh/plugins.md)
- [检索](docs/zh/retrieval.md)
- [成熟度与生产说明](docs/zh/maturity.md)
- [测试](docs/testing.md)
- [路线图](docs/zh/roadmap.md)

## 信任边界

Cortex 是运行时基础设施。Process JSON 是推荐的外部扩展边界。Trusted native ABI 插件运行在 daemon 进程内，必须视为受信任代码。

工具输出进入模型历史前会先记录为不可信外部输入。Guardrails 会分类常见的 prompt injection、system prompt 泄露、role override 和 exfiltration 模式。Policy lint 会拒绝危险组合，例如 open permission 加未审查插件、native 插件缺少显式 risk profile、以及 hostile evidence 自动进入 memory。

Cortex 的目标是让这些边界可见。它不声称能完整隔离敌对租户、不可信 native 代码，或会修改外部系统的工具。

## 许可

[MIT](LICENSE)
