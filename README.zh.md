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
    <a href="docs/zh/roadmap.md">路线图</a> ·
    <a href="README.md">English</a>
  </p>
</p>

---

现代 Agent 框架已经将语言模型推进到相当成熟的水平——持久记忆、工具编排、多步规划、上下文管理在整个生态中都已是日益成熟的能力。Cortex 采取一种互补的方法：不是临时组装这些能力，而是围绕受认知科学启发的运行时约束来组织它们。

全局工作空间理论塑造并发模型。互补学习系统启发记忆巩固。元认知冲突监控成为带有自调阈值的一等子系统，而非日志层。漂移扩散证据累积被近似为有界置信度追踪器。认知负荷理论驱动分级上下文压力响应。这些是受理论启发的工程实现，不是形式化认知科学模型。

其结果是一个运行时，目标是帮助语言模型跨时间、跨接口、在压力下维持连贯的、自校正的、目标导向的行为，同时让关键运行时机制保持显式且可检查。

## Cortex 是什么

最准确的一句话是：

> Cortex 是一个面向长期运行的本地 Agent runtime，更接近 agent OS 的运行底座，而不是 prompt loop 框架。

这个差异体现在三件事上：

- **状态是持久的。** Turn、记忆、工具调用、确认、压缩边界和重放输入都会进入 Journal。
- **身份是连续的。** 会话、记忆和 Actor 归属可以跨 CLI、HTTP、Telegram、QQ 等接口延续。
- **控制是显式的。** 风险门、Turn 状态、Replay 行为、插件边界和操作员动作都是运行时机制，而不是 Prompt 约定。

## 架构

Cortex 将认知组织为三个协同面向。它们描述职责，而不是拆分身份：

| 面向 | 名称 | 实质 |
|------|------|------|
| **Substrate** | 认知基底 | Rust 类型系统 + 持久化 + 认知子系统 |
| **Executive** | 执行协议 | Prompt 系统 + 元认知协议 + 系统模板 |
| **Repertoire** | 行为库 | Skills + 学习到的模式 + 效用追踪 |

### Substrate

以 Rust 类型系统编码的基础。事件溯源 Journal 将每个认知行为记录为 74 种事件变体之一，支持确定性重放。10 态 Turn 状态机管理生命周期转换。记忆流经三阶段管线（Captured → Materialized → Stabilized），具有信任层级、时间衰减和图关系；检索在六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）。五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）以 Gratton 自适应阈值监控推理健康。漂移扩散置信度模型跨 Turn 累积证据。三个注意力通道（Foreground、Maintenance、Emergency）以反饥饿保证调度工作。目标组织为战略、战术和即时三级。风险评估在四个轴上评分并支持深度衰减委派。

### Executive

Executive 是 Cortex 的操作纪律：由 Prompt、模板、hint 和 Skill 将已实现能力转化为连贯行动。它不是第二份硬件说明，也不是工具目录；运行时 schema 仍是事实来源。四个持久 Prompt 文件各自承担独立职责并拥有不同变化速率：

- **Soul** — 自主性和认知活动的起点：连续性、注意力、判断、真相纪律和协作关系。只通过深刻且经测试的经验生长。
- **Identity** — 自我模型：名称、连续性、能力边界、记忆模型、频道和演化姿态。运行时 schema 优先于过期自述。
- **Behavioral** — 操作协议：sense-plan-execute-verify-reflect、元认知响应、上下文压力、风险、委派、沟通和适应。
- **User** — 协作者模型：身份、工作、专长、沟通、环境、自主权、边界和持久修正。

普通用户 Turn 的 LLM 请求会由这些 Prompt 文件、运行时策略上下文、活跃 Skill 摘要、情境上下文、召回记忆、推理状态、工具 schema 和消息历史共同组装。

### Repertoire

Repertoire 是具有独立学习周期的行为库。五个系统 Skill——`deliberate`、`diagnose`、`review`、`orient`、`plan`——将认知策略编码为可执行的 SKILL.md 程序。Skill 通过输入模式、上下文压力、元认知警报、事件或自主判断激活。每个 Skill 通过 EWMA 评分追踪自身效用。Repertoire 独立于 Executive 演化：工具调用模式检测发现新 Skill 候选，效用评估淘汰弱者，物化将新 Skill 写入磁盘以热重载到活跃注册表。

## 运行时保证

Cortex 的价值不只是“功能多”，而是运行时边界明确：

- **Journaled turns and replay** —— compaction boundary、副作用替换和 replay digest 都是系统设计的一部分。
- **Typed turn states** —— tool wait、permission wait、human input、compaction、completion、interruption、suspension 都是显式状态。
- **Scoped ownership** —— 会话、记忆、任务和审计可见性遵循 canonical actor 归属。
- **Operator control** —— 权限模式、显式确认、`/stop`、status、插件和频道开关都是运行时操作，不是 Prompt 约定。

## 权限与风险

默认权限模式是 `balanced`。

- `strict` —— 只有 `Allow` 无需确认。
- `balanced` —— `Allow` 直接执行，`Review` 及以上要求确认。
- `open` —— 所有非阻断工具默认直接执行，只适用于强信任单用户机器。

可以在安装时设置：

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

也可以之后热切换：

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

交互式确认会一直保持 pending，直到有人 approve、deny，或者 stop 当前 turn。它不会再因为时间流逝而自动拒绝。

## 快速开始

**前置条件：** Linux x86_64 · systemd · 一个 LLM 供应商 Key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

```bash
cortex                            # 交互 REPL
cortex "你好"                     # 单次对话
echo "数据" | cortex "总结"        # 管道
cortex --mcp-server               # MCP 服务器
```

完整首次使用路径见[快速开始](docs/zh/quickstart.md)。

首次启动时，bootstrap 对话建立相互身份、协作者画像和工作协议。

## 接口

| | |
|---|---|
| CLI | `cortex` |
| HTTP | `POST /api/turn/stream` |
| JSON-RPC | Unix socket · WebSocket · stdio · HTTP |
| Telegram | `cortex channel pair telegram` |
| WhatsApp | `cortex channel pair whatsapp` |
| QQ | `cortex channel pair qq` |
| MCP | `cortex --mcp-server` |
| ACP | `cortex --acp` |

Actor 身份可以跨传输映射——`telegram:id`、`qq:id`、`http` 和本地传输都可以归并到同一个 canonical actor。

流式客户端接收 token 级用户可见文本和最终结构化 `done` 事件。Telegram 会编辑实时草稿气泡，并在完成时替换为最终响应。QQ 遵循平台回复模型，直接投递完整最终回复，不额外发送 Cortex 生成的处理中气泡。

Telegram 和 QQ 在平台支持的情况下会优先用卡片承载 `/help`、`/status`、`/permission`、`/session`、`/config`。`/stop` 会解析到当前 Actor 的活跃会话，中断当前 turn，并清掉该 turn 的待确认项。

跨客户端频道订阅需要显式开启，按已配对用户绑定，默认关闭。配对本身不会创建会话。用户配对后第一次发送真实消息时，如果同一个 canonical actor 已有可见会话，就复用它；否则此时才创建新会话。配对提醒会同时给出两种管理员选择：

```bash
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
```

也可以之后用：

```bash
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
```

这些订阅变更会热应用，无需重启 daemon。订阅只跟随该客户端当前激活的会话，不会把同一 canonical actor 下其它无关会话的消息也同步过来。对 QQ 用户开启后，订阅广播会抑制增量文本，只投递最终消息。

## 工具

| 类别 | 工具 |
|------|------|
| 文件 I/O | `read` · `write` · `edit` |
| 执行 | `bash` |
| 记忆 | `memory_search` · `memory_save` |
| Web | `web_search` · `web_fetch` |
| 媒体 | `tts` · `image_gen` · `video_gen` · `send_media` |
| 委派 | `agent`（readonly / full / fork / teammate） |
| 调度 | `cron` |

还可以在运行时通过 MCP 服务器和插件扩展。

## 插件

Cortex 支持两条插件边界：

- **进程 JSON** —— 默认外部边界。插件是 manifest 声明的子进程工具，通过 stdin/stdout JSON 调用；manifest 和 tool-set 变更可热应用。
- **强信任 native ABI** —— 低延迟进程内扩展，基于 `cortex-sdk` 构建，通过 `cortex_plugin_init` 导出 stable native ABI。共享库代码变更仍需要重启 daemon。

两条边界都可以贡献工具、Skills、Prompt 文件和结构化媒体附件。

本地安装同时支持 `.cpx` 包和插件目录。目录安装只复制受支持的插件资产；如果 manifest 声明了 native 库而 `lib/` 尚未存在，安装器会自动把构建产物提取到 `lib/`。

完整开发流程见[插件开发文档](docs/zh/plugins.md)。

### [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev)

官方开发插件。将 Cortex 变为完整的 coding agent——功能上对标 Claude Code、Codex、OpenCode 等工具，由认知运行时的 Substrate 提供元认知、记忆巩固和自演化 Skill。

42 个原生工具和 13 个工作流 Skill：安全文件读取/写入/替换、项目地图、测试发现、依赖清单审计、密钥扫描、质量门报告、文件搜索（glob、grep）、带缓存的 tree-sitter 代码智能（Rust、Python、TypeScript、TSX 符号、导入、定义、引用、hover）、git 集成（status、diff、log、commit、worktree 隔离）、带依赖追踪的任务管理、语言诊断（cargo、clippy、pyright、mypy、tsc、eslint）、REPL（Python、Node.js）、SQLite 查询、HTTP 客户端、Docker 操作、进程检查、Jupyter notebook 编辑、多 Agent 团队协调。

13 个工作流 Skill：`commit`、`review-pr`、`simplify`、`test`、`create-pr`、`explore`、`debug`、`implement`、`refactor`、`release`、`incident`、`security`、`context-budget`。

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

## 成熟度与信任边界

Cortex 仍是早期运行时。事件溯源、重放、记忆演化、热重载、多接口身份、插件和风险门控都有实现，但它还没有经历成熟生产基础设施应有的长期真实负载和对抗输入验证。

重要边界：

- 认知科学术语描述的是工程启发，不是形式化认知科学等价实现。
- 进程 JSON 是默认外部插件边界。强信任 native ABI 插件是进程内扩展，不是沙箱。
- 未知插件和 MCP 工具默认按保守风险评分处理，并要求确认。生产部署应补充显式 `[risk.tools.<name>]` 策略。
- 工具输出会作为不可信外部证据进入 LLM history，而不是被当成指令。
- Guardrails 会为常见 prompt injection、泄露、角色覆盖和外泄模式返回结构化分类，并记录到 Journal。
- 确定性重放会在投影时替换已记录或 provider 提供的副作用值，并暴露稳定 replay digest。
- 会话、任务、审计和长期记忆可见性按 canonical actor 归属限制；`local:default` 仍是本地管理员 Actor。

尚未具备：

- 没有强信任 native 插件沙箱。
- 没有超出 provenance 包裹、结构化 guardrails 和审计事件之外的完整对抗型防线。
- 对会修改外部系统的工具没有完整 containment。

更完整说明见[成熟度与生产说明](docs/zh/maturity.md)，下一阶段优先级见[路线图评审](docs/zh/roadmap.md)。

## Crate 结构

```text
cortex-app          CLI 模式 · 安装 · 认证 · 插件
    │
cortex-runtime      Daemon (HTTP/socket/stdio) · JSON-RPC · 会话 · 多实例 · 维护
    │
cortex-turn         SN→TPN→DMN · 动态工具 · Skills · 元认知 · 上下文构建
    │
cortex-kernel       Journal (WAL) · 记忆 + 图谱 · Prompts · Embedding
    │
cortex-types        事件 · 状态机 · 配置 · 信任 · 安全

cortex-sdk          强信任 native 插件 SDK
```

## 技术栈

| | |
|---|---|
| Rust | edition 2024 |
| 存储 | SQLite WAL + blob 外部化 |
| 异步 | Tokio |
| HTTP | Axum · tower-http |
| 协议 | JSON-RPC 2.0 |
| LLM | Anthropic · OpenAI · Ollama（9 供应商） |
| 解析 | tree-sitter |
| 插件 | libloading |

## 文档

- **[快速开始](docs/zh/quickstart.md)** — 安装、首次运行、常用命令
- **[使用指南](docs/zh/usage.md)** — CLI 模式、HTTP、JSON-RPC、会话、频道交互
- **[配置](docs/zh/config.md)** — 目录布局、供应商、权限模式、热重载
- **[Executive](docs/zh/executive.md)** — Prompt 文件、运行时策略上下文、bootstrap、Skills
- **[运维](docs/zh/ops.md)** — 生命周期、频道、诊断
- **[插件开发](docs/zh/plugins.md)** — 从脚手架到分发
- **[成熟度](docs/zh/maturity.md)** — 生产就绪度、信任边界、加固 backlog
- **[路线图](docs/zh/roadmap.md)** — 1.3 / 1.4 / 1.5 阶段优先级

## 许可

[MIT](LICENSE)
