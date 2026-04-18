<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center">持久化记忆 · 自演化身份 · 元认知自觉察</p>
  <p align="center">一个让 AI 智能体在长期使用中显著变好的认知运行时。</p>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.82%2B-orange.svg" alt="Rust 1.82+"></a>
  <a href="https://github.com/by-scott/cortex/releases"><img src="https://img.shields.io/github/v/release/by-scott/cortex?color=green" alt="Release"></a>
  <a href="https://github.com/by-scott/cortex/actions"><img src="https://img.shields.io/github/actions/workflow/status/by-scott/cortex/ci.yml?label=CI" alt="CI"></a>
  <a href="docs/zh/quickstart.md"><img src="https://img.shields.io/badge/docs-快速开始-brightgreen.svg" alt="快速开始"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh.md">中文</a>
</p>

---

## 为什么是 Cortex

今天大多数智能体框架都提供持久化记忆和分层提示词 — 这已经是基本能力。但它们没有建模的是*认知本身如何运作*。它们不断追加记忆直到召回退化。它们让你配置人设但从不质疑它。它们在循环中调用工具直到上下文窗口填满或用户放弃。

Cortex 采取不同的路径：它将计算认知神经科学的模型实现为 Rust 代码。每个对话轮次运行一个模拟大脑网络的三阶段周期 — 感觉预处理、带实时自监控的专注执行、反思性巩固。记忆遵循互补学习系统理论：快速情景捕获馈入缓慢语义整合，战略性遗忘让召回随知识库增长而*改善*。五个元认知检测器 — 灵感来自前扣带皮层的冲突监控 — 在轮次中期捕获推理失败，并通过经验自我调节阈值。

结果是一个由架构本身驱动改善的智能体运行时，而非仅靠积累的数据。

## 差异化

**记忆随规模改善，而非退化。** 大多数记忆系统是追加式存储，随增长召回越来越嘈杂。Cortex 实现了四阶段生命周期（Captured → Materialized → Stabilized → Deprecated），配合将情景模式转化为语义知识的巩固管线。召回融合六个维度 — 语义相似度、BM25 关键词、时间衰减、信任状态、知识图谱邻近度、访问频率。第 1000 条记忆的召回和第 10 条一样可靠，因为系统主动策展它所知道的。

**自我纠正的推理，而非工具循环。** 五个元认知检测器在每个轮次中并行运行：死循环检测（重复相同的动作）、认知疲劳追踪、框架锚定（困在错误假设上）、奖赏预测误差（习惯性 vs 有用的工具选择）、健康退化。当推理偏移时，纠正提示直接注入下一次 LLM 调用 — 智能体在你注意到之前就打破了自己的循环。阈值通过 Gratton 效应自我调节：误报放松灵敏度，确认的捕获使其更敏锐。

**提示词从证据中演化，而非从配置。** 四层提示词 — Soul、Identity、Behavioral、User — 通过积累的交互信号改变，而非手动编辑。六种证据类型（纠正、偏好、领域暴露、工具模式、输入复杂度、首轮信号）在任何演化发生前被评分和门控。最深层变化最慢，需要最强的证据 — 信念是赢得的，不是配置的。

**认知周期，而非 while 循环。** 每个轮次遵循映射到神经科学的三个阶段：SN（感觉 — 关键词提取、工作记忆激活、输入守卫）、TPN（任务正向 — LLM 推理和工具调度，带元认知检查点）、DMN（默认模式 — 信心评估、记忆提取、提示词演化）。轮次之间，心跳引擎驱动自主维护：巩固、嵌入、知识图谱更新、技能演化。

## 特性

- **17 个核心工具** — 文件读写、Shell 执行、记忆操作、网络搜索、子智能体、定时任务、图像/视频/语音生成、自省（audit、prompt_inspect、memory_graph）
- **5 个认知技能** — deliberate、diagnose、review、orient、plan — 基于检测模式自动激活
- **插件生态** — 通过 MultiToolPlugin FFI 接口和 `.cpx` 归档格式扩展工具与技能；官方插件如 [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 独立提供
- **11 个 LLM 供应商** — Anthropic、OpenAI、智谱 AI、Kimi、MiniMax、OpenRouter、Ollama，支持三级路由和自定义供应商
- **9 种协议** — HTTP REST、JSON-RPC 2.0、SSE、WebSocket、ACP、MCP 服务端/客户端、Telegram、WhatsApp
- **多实例** — 完全隔离的实例，独立拥有配置、数据、记忆、提示词和 systemd 服务

## 快速开始

**前置条件：** 配备 systemd 的 Linux 系统，任一[受支持供应商](#llm-供应商)的 API 密钥

```bash
# 安装二进制文件
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install

# 创建实例并启动守护进程
CORTEX_API_KEY="your-key" cortex install

# 查看状态
cortex status

# 开始对话
cortex
```

完整安装指南请参阅 [docs/zh/quickstart.md](docs/zh/quickstart.md)。

## 架构

```
┌─────────────────────────────────────────────────────────────┐
│  知识层 (Repertoire) — 记忆、习得模式、领域技能              │  持续演变
├─────────────────────────────────────────────────────────────┤
│  执行层 (Executive) — Soul / Identity / Behavioral / User   │  信号驱动
│                       提示词、认知技能、元认知提示            │  演化
├─────────────────────────────────────────────────────────────┤
│  基底层 (Substrate) — Rust 运行时：事件溯源、记忆管线、      │  版本
│                       工具调度、三相认知循环                  │  发布
└─────────────────────────────────────────────────────────────┘
```

六个 crate — 五个构成分层依赖链，另有一个独立 SDK：

| Crate | 职责 |
|-------|------|
| **cortex-sdk** | 发布的插件接口 — 零内部依赖，独立发布到 crates.io（约 150 行，纯接口） |
| **cortex-types** | 领域类型 — 零逻辑，不依赖其他 crate |
| **cortex-kernel** | 事件日志（SQLite WAL）、嵌入管线、提示词管理器 |
| **cortex-turn** | 轮次执行、记忆召回、元认知、工具调度、技能（依赖 cortex-sdk，重新导出其 trait） |
| **cortex-runtime** | 守护进程生命周期、HTTP/RPC/SSE/WS 服务、热加载、心跳引擎 |
| **cortex-app** | CLI、REPL、systemd 集成、插件管理 |

## 工作原理

每轮对话遵循三相认知循环：

1. **感知** (SN) — 激活工作记忆，提取关键词，召回相关上下文
2. **执行** (TPN) — LLM 推理与工具调度循环，伴随元认知检查点
3. **反思** (DMN) — 评估置信度，提取记忆，触发提示词演化

在对话间隙，心跳引擎运行后台维护：记忆合并、嵌入生成、知识图谱更新和提示词自演化 — 全部按显著性排序并受速率限制。

## 记忆

记忆经历四个生命周期阶段 — **捕获**、**实体化**、**稳定化**、**弃用** — 通过六维混合评分召回：

| 维度 | 权重 | 方法 |
|------|------|------|
| 语义相似度 | 0.40 | 嵌入向量余弦相似度 |
| BM25 | 0.25 | 关键词匹配 |
| 时间衰减 | 0.15 | 指数时间衰减 |
| 生命周期状态 | 0.10 | 阶段加权 |
| 知识图谱 | 0.10 | 实体关系邻近度 |
| 访问频率 | 0.05 | 检索频率 |

## LLM 供应商

11 个内置供应商，三级路由（**重型** / **中型** / **轻型**）：

`anthropic` · `openai` · `zai` · `zai-openai` · `zai-cn` · `zai-cn-openai` · `kimi` · `kimi-cn` · `minimax` · `openrouter` · `ollama`

自定义供应商可添加至 `providers.toml`。四种预设方案控制子端点激活策略：`minimal`、`standard`、`cognitive`、`full`。

## 使用

| 模式 | 命令 |
|------|------|
| 交互式 REPL | `cortex` |
| 单次提问 | `cortex "问题"` |
| 管道输入 | `echo "数据" \| cortex "总结"` |
| Web 界面 | `http://127.0.0.1:PORT/` |
| 智能体协议 | `cortex --acp` |
| 工具提供者 | `cortex --mcp-server` |

```bash
# 服务管理
cortex install [--system] [--id work]   # 注册 systemd 服务
cortex start | stop | restart            # 管理守护进程
cortex status                            # 查看状态、地址、LLM 信息
cortex ps                                # 列出所有实例
cortex reset [--factory]                 # 重置数据或完全清除

# 插件管理
cortex plugin install owner/repo         # 从 GitHub 安装
cortex plugin list                       # 列出已安装插件
cortex plugin uninstall name             # 移除插件

# 环境配置
cortex node setup                        # 安装 Node.js + pnpm（用于 MCP 服务器）
cortex node status                       # 查看 Node.js 环境状态
cortex browser enable                    # 配置 Chrome DevTools MCP 服务器
cortex browser status                    # 查看浏览器集成状态
```

## 文档

| | |
|---|---|
| **[快速开始](docs/zh/quickstart.md)** | 安装、首次对话、使用模式 |
| **[使用指南](docs/zh/usage.md)** | CLI、API、工具、技能、记忆、协议 |
| **[配置参考](docs/zh/config.md)** | 全部配置节、供应商、热加载 |
| **[运维指南](docs/zh/ops.md)** | 服务管理、监控、安全、备份 |
| **[插件指南](docs/zh/plugins.md)** | 插件使用、开发、`.cpx` 格式、原生 FFI |

## 开发

```bash
docker compose run --rm dev cargo build --release
docker compose run --rm dev cargo test --workspace
docker compose run --rm dev cargo clippy --workspace --all-targets -- -D warnings
```

## 许可证

[MIT](LICENSE)
