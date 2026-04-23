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
    <a href="README.md">English</a>
  </p>
</p>

---

现代 Agent 框架已经将语言模型推进到相当成熟的水平——持久记忆、工具编排、多步规划、上下文管理在整个生态中都已是日益成熟的能力。Cortex 采取一种互补的方法：不是临时组装这些能力，而是围绕受认知科学启发的运行时约束来组织它们。

全局工作空间理论塑造并发模型。互补学习系统启发记忆巩固。元认知冲突监控成为带有自调阈值的一等子系统，而非日志层。漂移扩散证据累积被近似为有界置信度追踪器。认知负荷理论驱动分级上下文压力响应。这些是受理论启发的工程实现，不是形式化认知科学模型。

其结果是一个运行时，目标是帮助语言模型跨时间、跨接口、在压力下维持连贯的、自校正的、目标导向的行为，同时让关键运行时机制保持显式且可检查。

## 架构

Cortex 将认知组织为三个协同面向。它们描述职责，而不是拆分身份：

| 面向 | 名称 | 实质 |
|----|------|------|
| **Substrate** | 认知基底 | Rust 类型系统 + 持久化 + 认知子系统 |
| **Executive** | 执行协议 | 提示系统 + 元认知协议 + 系统模板 |
| **Repertoire** | 行为库 | Skills + 学习到的模式 + 效用追踪 |

### Substrate

以 Rust 类型系统编码的基础。事件溯源 Journal 将每个认知行为记录为 72 种事件变体之一，支持确定性重放。10 态 Turn 状态机管理生命周期转换。记忆流经三阶段管线（Captured → Materialized → Stabilized），具有信任层级、时间衰减和图关系；检索在六个加权维度上排序（BM25、余弦相似度、时间衰减、状态、访问频率、图连接度）。五个元认知检测器（DoomLoop、Duration、Fatigue、FrameAnchoring、HealthDegraded）以 Gratton 自适应阈值监控推理健康。漂移扩散置信度模型跨 Turn 累积证据。三个注意力通道（Foreground、Maintenance、Emergency）以反饥饿保证调度工作。目标组织为战略、战术和即时三级。风险评估在四个轴上评分并支持深度衰减委派。

### Executive

Executive 是 Cortex 的操作纪律：由 Prompt、模板、hint 和 Skill 将已实现能力转化为连贯行动。它不是第二份硬件说明，也不是工具目录；运行时 schema 仍是事实来源。四个持久 Prompt 文件各自承担独立职责并拥有不同变化速率：

- **Soul** — 自主性和认知活动的起点：连续性、注意力、判断、真相纪律和协作关系。只通过深刻且经测试的经验生长。
- **Identity** — 自我模型：名称、连续性、能力边界、记忆模型、频道和演化姿态。运行时 schema 优先于过期自述。
- **Behavioral** — 操作协议：sense-plan-execute-verify-reflect、元认知响应、上下文压力、风险、委派、沟通和适应。
- **User** — 协作者模型：身份、工作、专长、沟通、环境、自主权、边界和持久修正。

实际 LLM 请求会组合这些 Prompt 文件、活跃 Skill 摘要、bootstrap 或恢复上下文、召回记忆、推理状态、元认知 hint、工具 schema 和消息历史。Cortex 被设计为适应能力演进：新的工具、供应商、频道和插件先从运行时 schema 中发现，再成为自我描述的一部分。

### Repertoire

具有独立学习周期的行为库。五个系统 Skill——`deliberate`、`diagnose`、`review`、`orient`、`plan`——将认知策略编码为可执行的 SKILL.md 程序。Skill 通过五条路径激活：输入模式匹配、上下文压力阈值、元认知警报、事件触发、自主判断。每个 Skill 通过 EWMA 评分追踪自身效用。Repertoire 独立于 Executive 演化：工具调用模式检测发现新 Skill 候选，效用评估淘汰弱者，物化将新 Skill 写入磁盘以热重载到活跃注册表。三层分区（system / instance / plugin）支持渐进特化。

## 认知基础

| 理论 | 实现 | 代码 |
|------|------|------|
| 全局工作空间 [Baars] | 独占前台 Turn + Journal 广播 | `orchestrator.rs` |
| 互补学习系统 [McClelland] | Captured → Materialized → Stabilized | `memory/` |
| ACC 冲突监控 [Botvinick] | 五检测器 + Gratton 自适应阈值 | `meta/` |
| 漂移扩散模型 [Ratcliff] | 固定增量证据累积 | `confidence/` |
| 奖赏预测误差 [Schultz] | EWMA 工具效用 + UCB1 探索-利用 | `meta/rpe.rs` |
| 前额叶层级 [Koechlin] | 战略/战术/即时目标 | `goal_store.rs` |
| 认知负荷理论 [Sweller] | 7 Region 工作空间 + 5 级压力 | `context/` |
| 默认模式网络 [Raichle] | DMN 反思 + 30min 维护 | `orchestrator.rs` |
| ACT-R 产生式规则 | 三层 Skills + SOAR chunking | `skills/` |

## 成熟度与信任边界

Cortex 仍是早期运行时，但架构面很大：事件溯源、重放、记忆演化、热重载、多接口身份、原生插件和风险门控都有实现；同时，它还没有经历成熟生产基础设施应有的长期真实负载和对抗输入验证。除非已经为自己的部署场景做过审计和加固，否则应把它视为研究型本地 Agent runtime。

重要边界：

- 认知科学术语描述工程启发。当前实现是调度器、阈值、置信度分数和巩固启发式等实用近似。
- 原生插件是载入 daemon 进程的可信代码扩展点。它能力强、接入轻，但不是沙箱，也不是跨版本稳定二进制隔离层。
- 未知插件/MCP 工具默认按保守风险评分处理，并需要确认。生产部署应补充显式工具策略，而不是只依赖通用评分。
- Guardrails 结合关键词和 regex 检测。它能覆盖常见 prompt injection 与泄露模式，但只是基础防线，不是完整对抗边界。
- 确定性重放依赖记录非确定性副作用。重放投影会使用已记录或 provider 提供的副作用值，但工具调用外部系统时仍需要自己的审计和幂等纪律。

更完整说明见[成熟度与生产说明](docs/zh/maturity.md)。

## Crate 结构

```
cortex-app          CLI 模式 · 安装 · 认证 · 插件
    │
cortex-runtime      Daemon(HTTP/socket/stdio) · JSON-RPC · 会话 · 多实例 · 维护
    │
cortex-turn         SN→TPN→DMN · 动态工具 · Skills · 元认知 · 7 Region 工作空间
    │
cortex-kernel       Journal(WAL) · 记忆+图谱 · Prompt · Embedding
    │
cortex-types        72 事件 · 10 态状态机 · 配置 · 信任 · 安全

cortex-sdk          插件开发套件——原生插件的零依赖公共 API
```

## 快速开始

**前置条件：** Linux x86_64 · systemd · 一个 LLM 供应商 Key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install
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

流式客户端接收 token 级用户可见文本和最终结构化 `done` 事件。Telegram 会编辑实时草稿气泡，并在完成时替换为最终响应。QQ 遵循平台回复模型，直接投递完整最终回复，不额外发送 Cortex 生成的处理中气泡。跨客户端频道订阅需要显式开启，按已配对用户绑定，默认关闭。配对提醒会同时给出 `cortex channel approve <platform> <user_id>` 和 `cortex channel approve <platform> <user_id> --subscribe` 两种管理员选择。也可以之后用 `cortex channel subscribe <platform> <user_id>` 开启；用 `cortex channel unsubscribe <platform> <user_id>` 关闭。对 QQ 用户开启后，订阅广播会抑制增量文本，只投递最终消息。

## 工具

| 类别 | 工具 |
|------|------|
| 文件 I/O | `read` · `write` · `edit` |
| 执行 | `bash` |
| 记忆 | `memory_search` · `memory_save` |
| Web | `web_search` · `web_fetch` |
| 媒体 | `tts` · `image_gen` · `video_gen` · `send_media` |
| 委派 | `agent`（readonly / full / fork / teammate）|
| 调度 | `cron` |

## 插件

通过 `cortex-sdk` 原生 FFI。插件贡献工具、Skills、Prompt 文件和结构化媒体附件，零内部依赖。完整开发指南参见[插件开发文档](docs/zh/plugins.md)。

### [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev)

官方开发插件。将 Cortex 变为完整的 coding agent——功能上对标 Claude Code、Codex、OpenCode 等工具，由认知运行时的 Substrate 提供元认知、记忆巩固和自演化 Skill。

42 个原生工具和 13 个工作流 Skill：安全文件读取/写入/替换、项目地图、测试发现、依赖清单审计、密钥扫描、质量门报告、文件搜索（glob、grep）、带缓存的 tree-sitter 代码智能（Rust、Python、TypeScript、TSX 符号、导入、定义、引用、hover）、git 集成（status、diff、log、commit、worktree 隔离）、带依赖追踪的任务管理、语言诊断（cargo、clippy、pyright、mypy、tsc、eslint）、REPL（Python、Node.js）、SQLite 查询、HTTP 客户端、Docker 操作、进程检查、Jupyter notebook 编辑、多 Agent 团队协调。

13 个工作流 Skill：`commit`、`review-pr`、`simplify`、`test`、`create-pr`、`explore`、`debug`、`implement`、`refactor`、`release`、`incident`、`security`、`context-budget`——各自通过自然语言模式激活，引导结构化多步工作流。

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

## 技术栈

| | |
|---|---|
| Rust | edition 2024 |
| 存储 | SQLite WAL + blob 外部化 |
| 异步 | Tokio |
| HTTP | Axum · tower-http |
| 协议 | JSON-RPC 2.0 |
| LLM | Anthropic · OpenAI · Ollama（9 供应商）|
| 解析 | tree-sitter |
| 插件 | libloading |

## 文档

- **[快速开始](docs/zh/quickstart.md)** — 安装、首次运行、常用命令
- **[使用指南](docs/zh/usage.md)** — CLI 模式、HTTP、JSON-RPC、会话
- **[配置](docs/zh/config.md)** — 目录布局、供应商、热重载
- **[Executive](docs/zh/executive.md)** — Prompt 文件、bootstrap、Skills、LLM 输入面
- **[运维](docs/zh/ops.md)** — 生命周期、频道、诊断
- **[插件开发](docs/zh/plugins.md)** — 从脚手架到分发
- **[成熟度](docs/zh/maturity.md)** — 生产就绪度、信任边界、加固 backlog

## 许可

[MIT](LICENSE)
