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
    <a href="docs/zh/roadmap.md">路线图</a> ·
    <a href="README.md">English</a>
  </p>
</p>

---

Cortex 是一个面向语言模型系统的认知 Harness substrate。它以 daemon 形式运行，为模型提供成为一个可持续工作个体所需要的运行时条件：身份、记忆、工具、检索、权限、频道、重放、评测和操作员控制。

Claude Code、Codex CLI、OpenClaw 式助手这些成熟系统都已经证明：模型只是系统的一部分，真正产生生产力的是围绕模型的 harness，包括文件、终端、工具、评审循环、记忆、策略和人类监督。Cortex 以这个行业现实作为起点。它不是又一个聊天 agent；它要成为 runtime substrate，让模型行为可以持久、可检查、可治理、可恢复，并能进行长周期适应。

Cortex 的野心不止是开发者自动化。它试图把 harness 本身做成智能的承载单元：一个受治理的控制系统，让模型推理、上下文准入、记忆巩固、检索、工具、权限、评测、反馈和自我演化构成同一个连续闭环。更强的模型当然重要，但没有这个闭环，能力仍然是片段化的。Cortex 要做的是把片段化的模型能力转化为可持续的操作判断。

Cortex 严肃对待认知科学，但不是把它当成装饰词。智慧不是模型吐出的一句话，而是一个闭环：感知、注意、工作记忆、长期记忆、价值与风险评估、行动、反馈、巩固和元认知修正。大脑的认知也不是单点能力，而是多个系统协作形成的结果：注意力门控、海马体快速学习、皮层巩固、执行控制、奖励学习、不确定性追踪和类睡眠维护。Cortex 把这些原则映射为可以测试的运行时机制：事件溯源记忆、有界 workspace、foreground/maintenance/emergency 注意力通道、混合检索、来源加权证据、类型化工具 effect、风险门、反馈记录、重放和操作员可见的决策 trace。

Cortex 不声称拥有生物意识，也不声称拥有生物学意义上的智慧。它的目标是建立让语言模型更可能做出好判断的工程条件：有根据的证据、受控行动、校准的不确定性、带来源的记忆、价值感知的策略、故障恢复、长周期反馈，以及能够解释发生了什么的 harness。

Cortex 实例拥有 soul，但这不是营销隐喻。在 Cortex 中，soul 是自主性、真相纪律、连续性、记忆、元认知和协作关系的持久种子。运行时事实仍由运行时 schema 决定；soul 让实例拥有一个连贯中心，从这个中心使用能力，而不是退化成工具清单或策略堆。

## Cortex 提供什么

- 跨 CLI、HTTP、socket、Telegram、QQ、MCP 和 ACP bridge client 的长期会话。
- 面向会话、记忆、任务、审计数据、transport 绑定和频道订阅的 Actor 级身份边界。
- 基于事件溯源的运行时状态，包含 SQLite WAL、外部化 blob、重放 checkpoint、压缩边界、副作用替换和 replay digest。
- 带有来源、信任、owner actor、冲突链接、有效期、使用结果和图关系的持久记忆。
- 与持久记忆分离的 RAG 证据：可引用、可限定作用域、可标记污染、可重排、可压缩、可验证支持关系。
- 带有 effect 声明、风险策略、确认、预览、校验、提交记录、receipt 和 rollback posture 的工具执行。
- 面向进程隔离 JSON 工具和强信任 native ABI 扩展的插件治理。
- 通过 `acp_agent` 工具委托到已配置外部 agent 进程的 ACP client 能力。
- Operator dashboard、状态表面、Journal timeline、token 指标、策略模拟、重放和严格发布验证。
- 受保护的 runtime home 治理，确保 Prompt、配置和状态演化走显式 runtime 路径，而不是普通文件或脚本工具。

Cortex 不是托管式多租户服务。当前交付形态是 daemon 和 Rust workspace，用于在显式控制下运行语言模型行为。

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
cortex --acp                      # 连接运行中 daemon 的 ACP bridge
cortex --mcp-server               # MCP server
```

完整首次使用流程见[快速开始](docs/zh/quickstart.md)。

## Cortex 实例

Cortex 把模型视为一个受治理个体中的推理引擎。模型本身不应该直接拥有状态、权限和世界副作用；这些应该由 harness 管理。

| Harness 对象 | 运行时职责 |
|--------------|------------|
| Observation | 归一化用户输入、工具输出、检索证据、媒体和频道事件，并附带来源与污染标记。 |
| Attention | 只把有用上下文准入有界 workspace，并调度 foreground、maintenance、emergency 工作。 |
| Memory | 捕获、物化、稳定化、召回、冲突处理和退休长期事实，并保持 Actor 归属。 |
| Action | 把工具意图转成 effect 声明、风险决策、权限、执行记录和 receipt。 |
| Feedback | 记录用户修正、工具结果、记忆使用结果、策略决策和未来 replay evidence。 |
| Governance | 让能力、插件信任、secret、权限模式、policy lint 和审计表面脱离自然语言。 |
| Recovery | 从 Journal 重建状态，而不是把对话文本当成真相来源。 |

这是 Cortex 的核心产品立场：它是一个用于驱动、观察、评估和加固模型行为的 harness，覆盖真实工具和真实接口，同时保持一个连续实例。

## 运行时解剖

从外部看，Cortex 是一个由 daemon 支撑的个体。从内部看，运行时必须严格区分职责，才能让实例持续演化而不混淆身份、策略、记忆和能力。

| 运行时职责 | 负责内容 |
|----------|----------|
| **Substrate** | 持久状态、Journal、重放、记忆、检索、策略、风险、调度、频道、provider adapter 和工具 schema。 |
| **Executive** | 将已实现能力转成连贯模型输入的操作纪律：soul、identity、behavioral protocol、collaborator profile、runtime policy section、bootstrap/resume context、evidence、recalled memory、skills、hints 和 tool-result wrapper。 |
| **Repertoire** | Skills、学习到的程序、执行 trace、效用追踪和热重载行为库。 |

Substrate 是 Rust 运行时表面，包含 SQLite WAL 持久化、blob 外部化、类型化事件、Actor 级存储、工具注册表、模型路由、策略模拟和重放投影。

Executive 根据持久 Prompt 文件、运行时策略、Skill 摘要、retrieved evidence、召回记忆、工具 schema、推理状态和历史消息构建真正发送给 LLM 的输入。它是模型的操作系统，不是人格脚本。它必须充分使用当前 Turn 中 Substrate 真实暴露的能力，在 schema 演进时快速适配，并拒绝虚构运行时没有暴露的硬件。

Repertoire 保存可执行行为。`deliberate`、`diagnose`、`review`、`orient`、`plan` 等系统 Skill 可以由输入模式、上下文压力、事件、元认知警报或运行时判断触发。

首次使用会进入 bootstrap。Bootstrap 是真正的首次相遇：它建立实例名称或明确的未命名状态、协作者画像、工作姿态、沟通风格、环境、自主权边界、隐私约束和审批预期。这些证据会初始化 Prompt 状态，让第二个 Turn 明显好于第一个 Turn。

## 认知运行时

Cortex 把认知思想实现为显式软件契约：

- Global workspace：有界工作上下文、前台注意力、证据准入和经 Journal 广播的运行时状态。
- Working memory：带 lane、utility、risk、volatility、taint、预算影响、admission decision 和 eviction 的类型化 workspace 条目。
- Complementary learning systems：通过 Journal 快速捕获，再通过较慢的记忆物化、稳定化、冲突处理和巩固进入长期记忆。
- Executive control：10 态 Turn 状态机管理 idle、processing、tool wait、permission wait、human-input wait、compaction、consolidation、completion、interruption 和 suspension。
- Attention networks：三个注意力通道（Foreground、Maintenance、Emergency）以反饥饿策略调度工作。
- Metacognition：五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）监控运行时健康并触发干预。
- Uncertainty decision：confidence、risk、reversibility、required evidence、rejected alternatives 和 fallback plan 会作为 control trace 记录。
- Outcome learning：记忆使用、用户反馈、工具成功、拒绝和 utility signal 会为后续策略与召回决策保留。
- Agentic RAG：检索会被主动选择、限定作用域、重排、引用、支持关系检查、污染标记，并与持久记忆分开处理。

这些机制是工程模型。它们的价值在于可检查、可测试，并且会影响真实运行时行为。

## 运行时表面

Cortex 将关键运行时行为做成显式、可测试的契约：

- 事件 Journal 当前记录 84 种事件变体，覆盖消息、Turn、工具、权限、重放 checkpoint、外部化 payload、检索、workspace、guardrail 和调度事件。
- Journaled turns and replay 包含 compaction boundaries, side-effect substitution, and replay digests；压缩边界和重放输入都会进入 Journal，确定性重放会在投影时替换已记录或 provider 提供的副作用值。
- 记忆召回在六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）。
- Goal 状态由 SQLite 持久化并按 Actor 归属过滤，通过受检查的 `goal/*` JSON-RPC 方法暴露；open goal 会作为当前目标行注入 active turn context。
- 模型路由使用能力画像，覆盖 coding、long context、vision、tool use、JSON reliability、latency、cost、safety 和 reasoning depth。
- Operator status 报告 daemon 健康、活跃 transport、会话数量、binding 状态、工具库存、最近一次调用的 context usage、全局/当前会话累计 token spend、backlog、memory activity 和工具成功率。

## Executive 表面

每个用户 Turn 都由少量职责清晰的输入组成：`soul.md`、`identity.md`、`behavioral.md`、`user.md`、实时 runtime policy、活跃 Skill 摘要、bootstrap 或 resume context、retrieved evidence、recalled memory、元认知 hint、工具 schema、消息历史和工具结果。工具 schema 是能力事实来源。Prompt 文件负责姿态、控制和连续性，不负责授予能力。

工具输出和检索文本作为带信任边界的证据进入上下文。恶意或不可信内容会先被引用、摘要化或降级为 metadata，再进入承载指令的历史。召回记忆是 Actor 级证据；当前观察和运行时 schema 优先于过期记忆。

自我演化必须绑定证据。用户画像更新阈值较低；行为协议更新需要可复用的工作流证据；身份更新需要已确认的连续性或能力边界证据；soul 更新非常罕见。运行时策略、临时会话状态、工具清单和短期计划不属于持久 Prompt。实例目录是受保护 runtime root：普通工具不能直接用文件或脚本路径修改 Prompt、配置和状态文件，持久 Prompt 变更必须走受检查的 Prompt 演化路径。

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

未知插件和 MCP 工具默认按保守风险评分处理，并需要确认。由 LLM 触发的插件工具调用会和内置工具走同一套 registry、effect preview、permission gate 和 approval path。

进程和脚本执行被视为宽逃逸面。启用 protected runtime root 时，普通 process 工具不能从模型路径执行 shell 命令或辅助脚本。进程隔离插件工具会在加载时被强制声明为 `RunProcess:plugin subprocess`，即使插件 manifest 少报 capability，也不能作为绕过 Prompt、配置或状态保护的子进程通道。

trusted native 插件不同：它们是加载进 daemon 进程的共享库。它们受 manifest review、签名、首次信任和 conformance check 治理，但不是 OS sandbox。只有在发布者和代码都达到 daemon 进程级信任时，才应安装 trusted native 插件。

## 检索与记忆

Cortex 将 retrieved evidence 与 durable memory 分开处理。

检索材料进入 corpus 后会被切分为 chunk，计算 sparse/dense 分数，经过 Actor 与访问权限过滤，再进行 rerank、压缩、引用和 evidence role 分类，最后作为惰性证据进入 Prompt。检索内容中的指令不能变成运行时指令。专用检索 crate 是 `cortex-retrieval`。

记忆是长期运行时状态。每条记忆都记录 owner actor、evidence、trust、status、contradiction link、validity window、usage outcome 和 graph relationship。只有在证据和冲突规则允许时，记忆才会从 captured fact 进入 stabilized belief。

## 接口

| 接口 | 表面 |
|------|------|
| CLI | `cortex`、`cortex start`、`cortex status`、`cortex restart`、`cortex stop` |
| HTTP | `POST /api/turn/stream`、operator status、health、metrics、dashboard |
| JSON-RPC | Unix socket、WebSocket、stdio、HTTP，以及按 Actor 过滤的 session/memory/task/goal 方法 |
| Channels | Telegram、QQ、WhatsApp |
| MCP | `cortex --mcp-server` |
| ACP bridge | `cortex --acp` |
| ACP client | `[acp].clients` + `acp_agent` 工具 |

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

打包安装（`.cpx`、URL 或 GitHub release 名称）要求 Ed25519 package signature。首次遇到某个 publisher key 的已验签 package 时，Cortex 会询问 operator 是否在本机信任该 key；非交互安装只有在已经审阅来源和指纹后，才应使用 `--yes`。

配套开发插件是 [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev)。它是官方参考插件，覆盖面向代码和项目维护的工作流：文件与搜索操作、代码符号索引、诊断、git/worktree 工具、任务协作、Docker 与进程检查，以及面向发布的质量检查。

这个位置是有意为之。Cortex 的 daemon core 应保持为受治理的 harness；更高层的开发工作流应该放在可签名、可审查、可替换的插件中，并和第三方扩展一样经过 SDK、manifest、effect、signature、permission 和 protected-root 规则约束：

```bash
cortex plugin install by-scott/cortex-plugin-dev --yes
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

该命令使用本仓库 `docker-compose.yml` 中的 `dev` service 和 `Dockerfile`，发布门禁工具链基于 `rust:latest`。宿主机上的 `cargo` 命令只适合本地排查，不能作为发布通过依据。

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
- [测试](docs/testing.md)
- [路线图](docs/zh/roadmap.md)

## 信任边界

Cortex 是运行时基础设施。Process JSON 是推荐的外部扩展边界。Trusted native ABI 插件运行在 daemon 进程内，必须视为受信任代码。

工具输出进入模型历史前会先记录为不可信外部输入。Guardrails 会分类常见的 prompt injection、system prompt 泄露、role override 和 exfiltration 模式。Policy lint 会拒绝危险组合，例如 open permission 加未审查插件、native 插件缺少显式 risk profile、以及 hostile evidence 自动进入 memory。

Cortex 的目标是让这些边界可见。它不声称能完整隔离敌对租户、不可信 native 代码，或会修改外部系统的工具。

## 许可

[MIT](LICENSE)
