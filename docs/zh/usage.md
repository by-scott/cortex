# 使用参考

## 运行模式

| 模式 | 命令 / URL | 传输方式 | 备注 |
|------|-----------|----------|------|
| 交互式 REPL | `cortex` | Unix socket | 默认；支持斜杠命令 |
| 单次提示 | `cortex "question"` | Unix socket | 单次 RPC 调用，返回后退出 |
| 管道输入 | `echo "content" \| cortex` | Unix socket | 读取 stdin，作为提示发送 |
| 命名会话 | `cortex --session name "prompt"` | Unix socket | 切换到或创建命名会话 |
| 命名实例 | `cortex --id work "prompt"` | Unix socket | 指向命名实例 |
| Web UI | `http://127.0.0.1:PORT/` | HTTP | 内嵌于二进制文件的聊天界面 |
| 仪表盘 | `http://127.0.0.1:PORT/dashboard.html` | HTTP | 指标和状态 |
| 审计追踪 | `http://127.0.0.1:PORT/audit.html` | HTTP | 决策审计日志查看器 |
| ACP 代理 | `cortex --acp` | stdio | Agent Communication Protocol (JSON-RPC 2.0) |
| MCP 服务器 | `cortex --mcp-server` | stdio | Model Context Protocol (JSON-RPC 2.0) |
| 守护进程 | `cortex --daemon` | HTTP + socket + stdio | 完整运行时；所有传输方式 |

---

## CLI 子命令

### 服务

| 命令 | 描述 |
|------|------|
| `cortex install [--system] [--id ID]` | 安装为 systemd 服务 |
| `cortex uninstall [--purge]` | 移除服务（--purge 删除所有数据） |
| `cortex start` | 启动守护进程 |
| `cortex stop` | 停止守护进程 |
| `cortex restart` | 重启守护进程 |
| `cortex status` | 显示守护进程状态 |
| `cortex ps` | 列出运行中的实例 |
| `cortex reset [--factory] [--force]` | 重置实例数据 |
| `cortex help [subcommand]` | 显示帮助 |

### Node 和浏览器

| 命令 | 描述 |
|------|------|
| `cortex node setup` | 安装 Node.js + pnpm（用于 MCP 服务器） |
| `cortex node status` | 显示 Node.js 环境状态 |
| `cortex browser enable` | 配置 Chrome DevTools MCP 服务器 |
| `cortex browser status` | 显示浏览器集成状态 |

### 插件

| 命令 | 描述 |
|------|------|
| `cortex plugin install <source>` | 从 GitHub、URL、.cpx 或目录安装 |
| `cortex plugin uninstall <name> [--purge]` | 移除插件 |
| `cortex plugin list` | 列出已安装的插件 |
| `cortex plugin pack <dir> [output.cpx]` | 将插件目录打包 |

### 通道

| 命令 | 描述 |
|------|------|
| `cortex channel telegram` / `whatsapp` | 显示通道配置信息 |
| `cortex channel pair [platform]` | 列出待配对和已配对用户 |
| `cortex channel approve <plat> <id>` | 批准待配对用户 |
| `cortex channel revoke <plat> <id>` | 移除已配对用户 |
| `cortex channel allow <plat> <id>` | 添加到白名单 |
| `cortex channel deny <plat> <id>` | 添加到黑名单 |
| `cortex channel policy <plat> [mode]` | 获取或设置策略（pairing/whitelist/open） |

### 实例指向

任何命令都接受 `--id <instance>` 来指向命名实例而非默认实例。

---

## REPL 命令

| 命令 | 描述 |
|------|------|
| `/help` | 显示可用命令 |
| `/status` | 显示运行状态（token、turn、运行时间、配置） |
| `/stop` | 取消当前运行的 turn |
| `/quit` 或 `/exit` | 退出 REPL |
| `/session list` | 列出所有会话 |
| `/session new` | 创建新会话 |
| `/session switch <id>` | 切换到指定会话 |
| `/config list` | 列出配置段落 |
| `/config get <section>` | 显示某个配置段落 |

所有平台（REPL、Telegram、WhatsApp、WebSocket）均支持 turn 执行期间使用 `/stop` 和 `/status`。Turn 期间发送的消息会被注入到当前 turn 中，LLM 在下一次迭代中可见。REPL 中 Ctrl+C 自动发送 `/stop`。

配置段落：`daemon`、`api`、`embedding`、`web`、`plugins`、`llm_groups`、`mcp`、`memory`、`turn`、`metacognition`、`autonomous`、`context`、`skills`、`auth`、`tls`、`rate_limit`、`tools`、`health`、`evolution`、`router`、`ui`、`memory_share`

---

## HTTP API

### 标准轮次

```
POST /api/turn
Content-Type: application/json

{
  "session_id": "my-session",
  "input": "What is the weather?",
  "images": [
    {
      "media_type": "image/jpeg",
      "data": "<base64>"
    }
  ]
}
```

`images` 字段可选。每个条目需要 `media_type` 和 `data`（base64 编码）。

### SSE 流式传输

```
POST /api/turn/stream
Content-Type: application/json

{ "session_id": "my-session", "input": "Explain recursion" }
```

事件类型：`text`、`tool`、`trace`、`done`

### WebSocket

```
GET /api/ws
```

通过 WebSocket 的双向 JSON-RPC。

### REST 端点

| 方法 | 路径 | 描述 | 认证/限流豁免 |
|------|------|------|:---:|
| GET | `/api/health` | 健康检查 | 是 |
| GET | `/api/metrics/structured` | 结构化指标 | 是 |
| GET | `/api/daemon/status` | 守护进程状态和运行时长 | 否 |
| POST | `/api/turn` | 执行一个轮次 | 否 |
| POST | `/api/turn/stream` | SSE 流式轮次 | 否 |
| GET | `/api/ws` | WebSocket 升级 | 否 |
| GET | `/api/sessions` | 列出会话（limit、offset 参数） | 否 |
| POST | `/api/session` | 创建会话 | 否 |
| GET | `/api/session/:id` | 会话详情 | 否 |
| GET | `/api/memory` | 列出记忆 | 否 |
| POST | `/api/memory` | 保存记忆 | 否 |
| GET | `/api/audit/summary` | 审计摘要 | 否 |
| GET | `/api/audit/health` | 审计健康分数 | 否 |
| GET | `/api/audit/decision-path/:id` | 决策追踪（未找到返回 404） | 否 |
| POST | `/api/rpc` | JSON-RPC 2.0 端点 | 否 |

### 约束

- `session_id`：最多 256 字符，字母数字加连字符、下划线和点
- 空输入返回 400
- 请求体限制：2 MB（超出返回 413）
- 最大 10,000 个并发会话

---

## JSON-RPC 2.0

可通过 `/api/rpc`（HTTP）、`/api/ws`（WebSocket）、Unix socket 和 stdio（ACP/MCP）访问。

8 个命名空间共 21 个方法：

| 命名空间 | 方法 | 参数 |
|----------|------|------|
| `session` | `prompt` | `prompt`（string）、`session_id`（可选） |
| `session` | `new` | -- |
| `session` | `list` | `limit`（可选）、`offset`（可选） |
| `session` | `end` | `session_id` |
| `session` | `get` | `session_id` |
| `session` | `initialize` | -- |
| `session` | `cancel` | -- |
| `command` | `dispatch` | `command`（string） |
| `daemon` | `status` | -- |
| `skill` | `list` | -- |
| `skill` | `invoke` | `name`、`params` |
| `skill` | `suggestions` | -- |
| `memory` | `list` | -- |
| `memory` | `get` | `id` |
| `memory` | `save` | `type`（"User"/"Feedback"/"Project"/"Reference"）、`content`、`description` |
| `memory` | `delete` | `id` |
| `memory` | `search` | `query` |
| `health` | `check` | -- |
| `meta` | `alerts` | `session_id` |
| `mcp` | `prompts/list` | -- |
| `mcp` | `prompts/get` | `name` |

### 错误码

| 代码 | 含义 |
|------|------|
| -32700 | 解析错误 |
| -32600 | 无效请求 |
| -32601 | 方法未找到 |
| -32602 | 无效参数 |
| 1000 | 会话未找到 |
| 1001 | 会话已结束 |
| 1100 | 轮次执行失败 |
| 1200 | 命令分发失败 |
| 1300 | 记忆未找到 |
| 1301 | 记忆操作失败 |

---

## 消息通道

### Telegram

**轮询模式**（不需要公网 IP）：

创建 `~/.cortex/<instance>/channels/telegram/auth.json`：

```json
{
  "bot_token": "123456:ABC-DEF...",
  "mode": "polling"
}
```

**Webhook 模式**（需要公网 IP 或反向代理）：

```json
{
  "bot_token": "123456:ABC-DEF...",
  "mode": "webhook",
  "webhook_addr": "127.0.0.1:8443"
}
```

### WhatsApp

仅 Webhook 模式。创建 `~/.cortex/<instance>/channels/whatsapp/auth.json`：

```json
{
  "access_token": "...",
  "phone_number_id": "...",
  "verify_token": "...",
  "mode": "webhook",
  "webhook_addr": "127.0.0.1:8444"
}
```

### 共享行为

两个通道共享守护进程状态，自动管理会话（Telegram 为 `tg-{uid}`，WhatsApp 为 `wa-{phone}`），支持斜杠命令，遵守速率限制，并拆分长消息以适配平台消息大小限制。

---

## ACP 和 MCP

### ACP（Agent Communication Protocol）

通过 `cortex --acp` 启动。使用 JSON-RPC 2.0 通过 stdio 通信。

典型生命周期：

1. `session/initialize` -- 握手
2. `session/new` -- 创建会话
3. `session/prompt` -- 发送提示，接收响应

### MCP（Model Context Protocol）

通过 `cortex --mcp-server` 启动。使用 JSON-RPC 2.0 通过 stdio 通信。

典型生命周期：

1. `initialize` -- 能力协商
2. `tools/list` -- 发现可用工具
3. `tools/call` -- 调用工具

还支持 `prompts/list` 和 `prompts/get` 用于提示模板发现。

ACP 和 MCP 都需要运行中的守护进程，并通过 Unix socket 桥接到守护进程。

---

## 核心工具（17 个）

| 工具 | 描述 |
|------|------|
| `bash` | 带风险评估的 Shell 执行 |
| `read` | 支持部分范围的文件读取 |
| `write` | 文件创建和替换 |
| `edit` | 搜索替换编辑 |
| `memory_search` | 跨记忆存储的 6D 混合召回 |
| `memory_save` | 持久化到长期记忆（类型：User、Feedback、Project、Reference） |
| `agent` | 生成子代理（模式：readonly、full、fork、teammate；最大深度 3） |
| `skill` | 调用推理协议 |
| `cron` | 调度循环或一次性任务 |
| `web_search` | 支持域名过滤的 Brave Search API |
| `web_fetch` | URL 内容提取（10 MB 限制，60 秒超时） |
| `image_gen` | 根据文字提示生成图像 |
| `tts` | 文字转语音合成 |
| `video_gen` | 根据文字提示生成视频 |
| `audit` | 查询审计日志 -- 事件计数、健康分数、近期事件 |
| `prompt_inspect` | 读取和检查提示层（soul、identity、behavioral、user） |
| `memory_graph` | 查询实体关系图 -- 节点、边、邻居 |

### 自省工具

三个自省工具（`audit`、`prompt_inspect`、`memory_graph`）为 LLM 提供对 Cortex 自身内部状态的只读访问。它们打开日志、提示文件和图数据库的只读句柄，不与守护进程共享可变状态。

- `audit` -- 命令：`summary`、`health`、`recent`
- `prompt_inspect` -- 按名称读取提示层内容
- `memory_graph` -- 命令：`stats`、`neighbors`、`search`

### 插件工具

额外的工具可通过插件安装。例如，官方 [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 插件提供开发工具，包括代码导航（tree-sitter）、git 操作、docker 管理、任务追踪等。

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

插件工具通过 `cortex-sdk` 中定义的 `MultiToolPlugin` FFI 接口加载。详见 [docs/plugins.md](plugins.md)。

MCP 桥接的工具在运行时进一步扩展工具集（命名为 `mcp_{server}_{tool}`）。

---

## 认知技能（5 个）

| 技能 | 用途 |
|------|------|
| `deliberate` | 面向高风险决策的结构化证据积累 |
| `diagnose` | 通过假设检验追踪症状到根因 |
| `review` | 视角转换的对抗性审视 |
| `orient` | 快速理解陌生领域 |
| `plan` | 层次化任务分解 |

技能按 3 层优先级加载：system < plugin < instance（实例覆盖优先）。插件可提供额外技能；详见 [docs/plugins.md](plugins.md)。

### 自定义技能

在 `~/.cortex/<instance>/skills/<name>/SKILL.md` 中创建技能，使用 YAML frontmatter：

```yaml
---
description: What this skill does
when_to_use: When the LLM should activate it
required_tools: [bash, read]
tags: [analysis]
activation:
  input_patterns: ["regex patterns"]
---
```

---

## 记忆系统

### 类型

| 类型 | 用途 |
|------|------|
| User | 用户偏好、纠正、个人上下文 |
| Feedback | 交互中的行为调整 |
| Project | 项目特定知识和约定 |
| Reference | 持久参考资料 |

### 种类

- **情景记忆（Episodic）** -- 随时间衰减的时间绑定记忆
- **语义记忆（Semantic）** -- 持久的泛化知识

### 生命周期阶段

Captured --> Materialized --> Stabilized --> Deprecated

### 6D 召回评分

| 维度 | 权重 |
|------|------|
| BM25（关键词匹配） | 0.25 |
| 语义余弦相似度 | 0.40 |
| 时近性 | 0.15 |
| 状态 | 0.10 |
| 访问频率 | 0.05 |
| 图连通性 | 0.10 |

---

## 提示演进

### 4 个层级

| 层级 | 用途 |
|------|------|
| Soul | 存在导向 -- 通过持续经验成长，变化最慢 |
| Identity | 角色和能力 |
| Behavioral | 交互模式和风格 |
| User | 个性化适配 |

### 6 个演进信号

| 信号 | 权重 |
|------|------|
| 纠正 | 1.0 |
| 偏好 | 0.8 |
| 新领域 | 0.6 |
| 首次会话 | 0.5 |
| 工具密集型轮次 | 0.4 |
| 长输入 | 0.3 |

---

## 轮次追踪

每个轮次产生包含 6 个类别事件的追踪：

`phase`、`llm`、`tool`、`meta`、`memory`、`context`

---

## LLM 分组

### 默认层级

| 层级 | 用途 |
|------|------|
| `heavy` | 主推理（最大模型） |
| `medium` | 分析和提取 |
| `light` | 简单提取任务 |

### 预设

4 个内置预设：`minimal`、`standard`、`cognitive`、`full`

### 子端点

7 个可路由到不同层级的专用子端点：

`memory_extract`、`entity_extract`、`compress`、`summary`、`self_update`、`causal_analyze`、`autonomous`

---

## 多实例

每个实例完全隔离，拥有独立的配置、数据、记忆、提示、技能、会话和套接字。

```
cortex --id work "prompt"
cortex --id work status
cortex --id work install
```

实例 ID：字母数字字符、连字符和下划线。最长 64 字符。

---

## 视觉

可通过 HTTP API 的 `images` 字段在轮次请求中发送图像。模型选择的解析优先级：

1. 显式 `[api.vision]` 配置设置
2. 提供商 `vision_model` 字段
3. 自动发现视觉能力的模型
4. 主模型探测
