# 配置

## 目录布局

```text
~/.cortex/
  providers.toml                 # 共享供应商注册表
  plugins/                       # 共享插件安装根目录
  <instance>/
    config.toml                  # 声明式实例配置
    config.defaults.toml         # 生成的默认值参考（只读）
    actors.toml                  # Actor 别名 + 传输绑定
    mcp.toml                     # MCP 服务器定义
    prompts/                     # Prompt 文件 + 系统模板
    skills/                      # 内置和实例级 Skills
    data/                        # 运行时状态：Journal、嵌入、task/goal 数据库
    memory/                      # 持久记忆存储
    sessions/                    # 会话历史
    channels/                    # 频道认证 + 运行时配对状态
```

**经验法则：** 根目录文件定义实例 _应该是什么样_。`data/` 记录 _运行时发生了什么_。

## 关键文件

### `config.toml`

实例主配置。涵盖：

- Daemon 和传输设置（HTTP 绑定地址、Unix socket 路径、TLS）
- API 供应商默认值（供应商、模型、基础 URL）
- 嵌入配置（供应商、模型、维度）
- 记忆行为（召回数量、提取节奏、巩固间隔、衰减率、相似度阈值）
- Turn 行为（最大迭代次数、整轮超时、单工具超时、Token 限制）
- 元认知（检测器权重、健康检查间隔、疲劳阈值）
- 上下文处理（压力阈值、摘要策略）
- ACP client 定义，用于配置外部 agent 进程
- 插件启用
- 工具风险策略
- 认证（OAuth、JWT）
- 限速（每会话和全局 RPM）
- 媒体生成默认值

### `config.defaults.toml`

自动生成的参考文件，展示所有默认值。Cortex 在安装和配置变更后写入。不是配置文件——不要编辑。用来发现可用设置及其默认值。

### `actors.toml`

传输、频道 Actor 和规范用户之间的身份映射：

```toml
[aliases]
"telegram:123456789" = "user:alice"

[transports]
http = "user:alice"
rpc = "user:alice"
ws = "user:alice"
sock = "user:alice"
stdio = "user:alice"
```

### `mcp.toml`

MCP 服务器定义。每个条目命名一个 Cortex 可连接的外部 MCP 服务器，提供额外工具和 Prompt。

### ACP clients

ACP client 在 `config.toml` 的 `[acp]` 下声明。只要配置了至少一个 client，Cortex 就会注册 `acp_agent` 工具，让 Turn 可以通过 stdio JSON-RPC 委托到外部 ACP 兼容 agent 进程。

```toml
[acp]
request_timeout_secs = 120

[[acp.clients]]
id = "reviewer"
command = "reviewer-agent"
args = ["--stdio"]
cwd = "/workspace/project"
env = { REVIEWER_MODE = "strict" }
```

| 字段 | 用途 |
|------|------|
| `request_timeout_secs` | `initialize`、session 创建和 prompt 调用的单请求超时 |
| `clients[].id` | `acp_agent` 工具使用的稳定 id |
| `clients[].command` | 要启动的可执行文件 |
| `clients[].args` | 命令参数 |
| `clients[].cwd` | 传给 `session/new` 的 session 根目录；相对路径从 daemon 进程 cwd 解析 |
| `clients[].env` | 传给子进程的额外环境变量 |

### `providers.toml`

共享供应商注册表。每个供应商条目定义协议、基础 URL、认证方式、模型列表，以及可选的多模态路由：

| 字段 | 用途 |
|------|------|
| `protocol` | `anthropic`、`openai` 或 `ollama` 传输格式 |
| `base_url` | 供应商 API 根地址 |
| `auth_type` | `x-api-key`、`bearer` 或 `none` |
| `models` | 已知文本模型；为空表示运行时发现或由显式配置指定 |
| `vision_provider` | 仅用于视觉请求的可选供应商 |
| `vision_model` | 默认多模态模型；为空表示自动发现 |
| `image_input_mode` | OpenAI 兼容图片模式：`data-url`、`upload-then-url` 或 `remote-url-only` |
| `files_base_url` | `upload-then-url` 使用的文件上传/内容 API 根地址 |
| `openai_stream_options` | 端点是否接受 OpenAI `stream_options` |
| `vision_max_output_tokens` | 视觉调用输出上限；`0` 使用安全默认值 |
| `capability_cache_ttl_hours` | 模型/能力缓存 TTL；`0` 使用运行时默认值 |

Cortex 将文本和视觉路由分开。纯文本 Turn 使用配置的文本端点。带图片附件的 Turn 从显式配置、`vision_provider` / `vision_model`、自动发现和缓存中解析视觉端点。

### `[llm_groups.*]` 与模型路由

LLM group 是后台端点和 route decision 使用的模型能力注册表。默认 group 是
`heavy`、`medium`、`light`；显式的 `[api.endpoint_groups]` 仍表示 operator
偏好，但 runtime resolver 会先按 capability、health posture、cost、latency、
safety 和 reasoning depth 对可用 group 评分，再选择子端点模型。

| 字段 | 用途 |
|------|------|
| `provider` | 供应商名称；为空则继承 `[api].provider` |
| `model` | 模型名称；为空则继承 `[api].model` 或供应商第一个已知模型 |
| `api_key` | 可选的 group 专用 key；为空则继承 `[api].api_key` |
| `max_tokens` | 输出上限；`0` 继承父级/默认上限 |
| `capabilities` | 可选显式能力列表：`coding`、`long_context`、`vision`、`tool_calling`、`json_reliability`、`low_latency`、`low_cost`、`high_safety`、`deep_reasoning` |
| `context_tokens` | 输入上下文窗口；`0` 由 Cortex 推断 |
| `output_tokens` | 输出 token 上限；`0` 由 Cortex 推断 |
| `latency_ms` | 预期中位延迟；`0` 按 tier 推断 |
| `input_cost_per_million` | 输入 token 成本提示；`0` 按 tier 推断 |
| `output_cost_per_million` | 输出 token 成本提示；`0` 按 tier 推断 |
| `safety_score` | `[0, 1]` 范围的安全分；`0` 由 Cortex 推断 |
| `reasoning_depth` | `[0, 1]` 范围的推理深度；`0` 由 Cortex 推断 |
| `json_reliability` | `[0, 1]` 范围的结构化输出可靠性；`0` 由 Cortex 推断 |

Route request 还会携带 intent、required/preferred capability、confidence、
risk、failed target，以及 provider failure 或 invalid schema output 等 fallback
reason。低 confidence + 高 risk 的请求会向 `high_safety` 与 `deep_reasoning`
升级；schema-invalid fallback 会要求 `json_reliability`。

## 记忆行为

`[memory]` 控制持久记忆提取、召回、巩固、衰减和语义升级：

| 字段 | 默认值 | 用途 |
|------|--------|------|
| `max_recall` | `10` | 注入 Turn 的最大召回记忆数量 |
| `auto_extract` | `true` | 是否自动执行 Turn 后记忆提取 |
| `extract_min_turns` | `5` | 两次自动提取之间的最小 Turn 数 |
| `consolidate_interval_hours` | `24` | 巩固和衰减的维护周期 |
| `decay_rate` | `0.05` | 陈旧记忆的时间衰减率 |
| `consolidation_similarity_threshold` | `0.85` | 智能合并候选所需的嵌入相似度 |
| `semantic_upgrade_similarity_threshold` | `0.90` | 重复情景记忆升级为语义记忆所需的相似度 |

提取会记录来源、记忆类型和置信度。明确用户陈述、直接工具证据和模型推断保持分离。处于再巩固窗口内的稳定记忆会注入提取上下文，使新纠正能更新旧记忆，而不是产生互不连接的重复项。

## Turn 超时

`[turn].execution_timeout_secs` 控制整轮前台 Turn，包括 LLM 调用、工具调用、委派 worker 和最终投递。默认值是 `0`，表示禁用整轮超时。

`[turn].tool_timeout_secs` 控制单次工具调用。默认值是 `1800` 秒。工具可以为自身安全定义更严格的超时。

`[turn].llm_transient_retries` 控制在尚未输出任何用户可见文本前，Cortex 对瞬时 LLM 传输/供应商故障的重试次数。默认值是 `5`；设为 `0` 可禁用这层保护。

## 工具风险策略

`[risk.tools.<name>]` 为单个工具定义显式风险策略。审查插件和 MCP 工具能力后，用它为具体工具配置确认或阻断规则。

```toml
risk.allow = ["read", "memory_*", "word_count"]
risk.deny = ["deploy_*", "*_shell"]
auto_approve_up_to = "Review"

[risk.tools.word_count]
tool_risk = 0.1
blast_radius = 0.0
irreversibility = 0.0
allow_background = true

[risk.tools.deploy_production]
require_confirmation = true
blast_radius = 0.9
irreversibility = 0.8

[risk.tools.unknown_shell_bridge]
block = true
```

可用字段：

| 字段 | 用途 |
|------|------|
| `tool_risk` | 覆盖基础工具风险轴，范围 `0.0` 到 `1.0` |
| `file_sensitivity` | 覆盖文件/路径敏感度，范围 `0.0` 到 `1.0` |
| `blast_radius` | 覆盖潜在影响范围，范围 `0.0` 到 `1.0` |
| `irreversibility` | 覆盖不可逆风险，范围 `0.0` 到 `1.0` |
| `require_confirmation` | 强制至少进入 `RequireConfirmation` |
| `block` | 无论评分如何都阻断工具 |
| `allow_background` | 记录该工具是否适合后台执行 |

`risk.deny` 始终优先。如果 `risk.allow` 非空，未匹配的工具会被阻断。`auto_approve_up_to` 控制哪些非阻断风险级别无需确认即可执行：默认标准模式是 `Review`，更严格的模式是 `Allow`，设为 `RequireConfirmation` 则是常规执行中最宽松的设置。`Block` 仍然直接拒绝且不弹确认。后台执行还要求工具声明 `background_safe` capability，或该工具配置 `allow_background = true`。

CLI 提供静态 policy 检查：

```bash
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

`cortex policy lint` 会读取当前实例配置和已启用插件的 manifest，并在使用前报告危险组合：open 权限模式搭配 unreviewed plugin、插件 manifest 不可读、native/process plugin 缺少显式 `[risk.tools.<name>]` profile、请求 secret 的插件没有确认/阻断策略、`web_fetch` 与自动记忆提取同时启用、高影响工具被允许后台执行等。daemon 启动时也会记录同一套 finding。`cortex policy simulate` 会解释单个 tool/effect 决策：actor、生效风险级别、是否自动放行、是否需要确认、是否允许后台执行，以及触发这些结果的 policy reason。

实例目录是受保护 runtime root。普通 file/edit/write 工具不能访问实例 home 下的 Prompt、配置、会话、Journal、记忆或频道状态，即使路径通过符号链接抵达该目录也会被阻断。启用 protected runtime root 时，process 和 script 工具也会被阻断，因为 shell 命令可能绕过 Prompt 与状态治理。插件工具不能把 Prompt、配置、会话、Journal、记忆或 runtime state 的直接修改包装成 LLM 可调用捷径；自我演化类插件必须返回 proposal，再由受检查的 runtime 路径应用已验证变更。实例自身变更应使用受检查的 runtime 命令、PromptManager 流程，或受治理的 package 工作流。

## 运行时数据 (`data/`)

运行时管理的文件——不要直接编辑：

- `cortex.db` — 事件 Journal（SQLite WAL）
- `embedding_store.db` — 向量嵌入索引
- `memory_graph.db` — 记忆关系图谱
- `cortex.sock` — Unix 域套接字
- `actor_sessions.json`、`client_sessions.json` — 会话映射
- 模型和能力缓存

## 频道配置

每个频道目录（`channels/<platform>/`）将声明式认证与运行时状态分离：

| 文件 | 管理者 | 用途 |
|------|--------|------|
| `auth.json` | 你 | Bot 令牌和凭据 |
| `policy.json` | 运行时 | 访问策略（open / whitelist / pairing） |
| `paired_users.json` | 运行时 | 已批准用户列表 |
| `pending_pairs.json` | 运行时 | 待配对请求 |

## 安装时环境变量

`cortex install` 读取以生成初始配置：

| 变量 | 用途 |
|------|------|
| `CORTEX_API_KEY` | 主供应商 API Key |
| `CORTEX_PROVIDER` | 供应商名称 |
| `CORTEX_MODEL` | 模型标识符 |
| `CORTEX_BASE_URL` | 自定义供应商端点 |
| `CORTEX_LLM_PRESET` | 端点预设：`minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_PERMISSION_LEVEL` | 安装时权限模式：`strict` / `balanced` / `open` |
| `CORTEX_EMBEDDING_PROVIDER` | 嵌入供应商 |
| `CORTEX_EMBEDDING_MODEL` | 嵌入模型 |
| `CORTEX_BRAVE_KEY` | Brave Search API Key |
| `CORTEX_TELEGRAM_TOKEN` | Telegram 机器人令牌 |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp 令牌 |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ 机器人凭据 |

## 热重载

以下文件修改后无需重启 Daemon：

- `config.toml` — 所有运行时安全设置
- `providers.toml` — 供应商注册表
- `mcp.toml` — MCP 服务器定义
- `prompts/` — 所有 Prompt 文件
- `skills/` — Skill 定义和 SKILL.md 文件

变更在下一个 Turn 生效。活跃 Turn 以先前配置完成。

CLI 也会对以下操作直接热应用，无需在正常用户态服务路径中额外重启：

- `cortex permission ...`
- `cortex browser enable` / `cortex browser disable`
- `cortex plugin enable` / `cortex plugin disable`
- `cortex channel subscribe ...` / `cortex channel unsubscribe ...`

Plugin package governance 写在每个插件的 `manifest.toml`，不写在实例配置里。安装前使用 `cortex plugin review <dir>` 和 `cortex plugin test <dir>` 检查 capability request、sandbox profile、signature metadata、conformance state，以及推荐的 `[risk.tools.<name>]` policy。
