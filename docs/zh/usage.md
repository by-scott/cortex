# 使用指南

## 运行模式

| 模式 | 命令 | 说明 |
|------|------|------|
| 交互式 CLI | `cortex` | 带行编辑和补全的 REPL |
| 单次提问 | `cortex "question"` | 一轮对话后退出 |
| 管道 | `cat file \| cortex "summarize"` | 读取 stdin 作为上下文 |
| 命名实例 | `cortex --id work` | 连接到指定实例 |
| ACP bridge | `cortex --acp` | 通过 stdio JSON-RPC 暴露正在运行的 Cortex daemon |
| MCP 服务端 | `cortex --mcp-server` | 通过 Model Context Protocol 暴露工具 |

## CLI 参考

### 服务

```bash
cortex install [--system] [--id NAME] [--permission-level strict|balanced|open]
cortex uninstall [--purge] [--id NAME]
cortex start [--id NAME]
cortex stop [--id NAME]
cortex restart [--id NAME]
cortex status [--id NAME]
cortex demo [--id NAME] [--home PATH] [--force]
cortex doctor [--id NAME] [--json]
cortex permission [strict|balanced|open] [--id NAME]
cortex ps
```

推荐权限模式：

- `balanced`：默认且推荐。自动放行 `Allow`，对 `Review` 及以上要求确认。
- `strict`：更保守。只有 `Allow` 无需确认。
- `open`：最宽松。所有非阻断工具默认直接执行，只适用于强信任的单用户本机。

`cortex permission` 会更新当前实例配置，并对用户态 daemon 热应用新模式。

`cortex config get <section>` 会打印配置段。`cortex config set turn.show_thinking true|false` 控制是否请求并显示供应商 thinking；默认隐藏/关闭。`cortex config set embedding.api_key <key>` 会写入嵌入供应商 key。对应的斜杠命令是 `/think show|hide` 和 `/config set ...`；用户态 daemon 会热加载这些改动。

`cortex demo` 创建用户本地的首次使用 fixture。它默认使用 `demo` 实例 id，写入偏向 Ollama 的配置，保持插件禁用，创建本地 coding skill，并把示例 workspace 放在 protected runtime root 之外。它不会启动服务，也不会放宽权限。

`cortex doctor` 是只读命令。它报告本地就绪状态和 policy 姿态：OS/systemd 可用性、实例路径、service 与 socket 状态、config 解析状态、provider key 姿态、权限模式、已启用插件、频道 auth、policy lint finding、protected runtime root 和本地模型端点线索。使用 `--json` 可以输出带 remediation hints 的机器可读报告。它不声称沙箱隔离，也不会修改 runtime state。

### ACP

`cortex --acp` 是面向编辑器或 harness client 的 bridge 入口，用 stdio JSON-RPC 连接正在运行的 daemon。Cortex 也可以作为 ACP client：在 `config.toml` 的 `[acp].clients` 下声明外部 agent，然后用 `acp_agent` 工具按 `agent_id` 调用。该工具把外部进程约束在有界的 initialize/session/prompt 生命周期内，并复用 ACP session，除非显式请求 `new_session`。

### JSON-RPC 状态面

Daemon JSON-RPC 状态面按 Actor 过滤。Session、memory、task、goal 和 audit 查询都解析为当前传输配置的 Actor；`local:default` 是 operator 身份，非本地 Actor 只能看到自己的状态。

Task 方法：

| 方法 | 用途 |
|------|------|
| `task/create` | 创建归属当前 Actor 的 task，参数为 `description`，可选 `priority`、`parent_task_id` 和 RFC3339 `deadline` |
| `task/list` | 列出可见 task，可按 `status` 过滤 |
| `task/get` | 按 `id` 读取一个可见 task |
| `task/delete` | 按 `id` 删除一个可见 task |
| `task/claim` | 原子地将可见的 `Pending` task 分配给一个 `instance_id`，状态变为 `Assigned` |
| `task/update` | 执行受检查的状态迁移，例如 `Assigned -> InProgress -> Completed` |

Goal 方法：

| 方法 | 用途 |
|------|------|
| `goal/create` | 创建归属当前 Actor 的 goal，参数为 `description`，可选 `level`、`status`、`success_criteria`、`priority`、引用、关联 task 和 RFC3339 `deadline` |
| `goal/list` | 列出可见 goal，可按 `status` 或 `open_only` 过滤 |
| `goal/get` | 按 `id` 读取一个可见 goal |
| `goal/delete` | 按 `id` 删除一个可见 goal |
| `goal/update` | 更新受检查的 goal 字段，并执行 `Active -> Blocked -> Completed` 这类状态迁移 |

### Policy

```bash
cortex policy lint [--id NAME]
cortex policy simulate <tool> [--effect KIND[:TARGET]] [--actor ACTOR] [--background]
```

`cortex policy lint` 检查当前实例配置和已启用插件的 manifest。存在发布阻断级 policy 错误时会返回非零退出码。`cortex policy simulate` 会在工具运行前解释当前 policy 如何处理某个工具及其声明的 effects。

### 插件

```bash
cortex plugin install owner/repo
cortex plugin install owner/repo@1.6.4
cortex plugin install owner/repo --yes
cortex plugin install ./plugin-dir
cortex plugin install ./plugin.cpx
cortex plugin review ./plugin-dir
cortex plugin test ./plugin-dir
cortex plugin keygen ~/.config/cortex/plugin-signing/publisher.ed25519
cortex plugin sign ./plugin-dir --key ~/.config/cortex/plugin-signing/publisher.ed25519 --publisher example.dev
cortex plugin pack ./plugin-dir
cortex plugin enable NAME
cortex plugin disable NAME
cortex plugin uninstall NAME
cortex plugin list
```

从 `.cpx`、URL 或 GitHub release 名称安装的打包插件必须通过 Ed25519 package signature 验证。首次遇到新的已验签 publisher key 时，交互式终端会询问是否在本机信任它。只有在已经审阅 package 来源和 key fingerprint 后，才应使用 `--yes`。`--yes` 不会绕过签名、hash 或 package safety 检查。

### 浏览器

```bash
cortex browser enable
cortex browser disable
cortex browser status
```

### Actor

```bash
cortex actor alias list
cortex actor alias set <from> <to>
cortex actor alias unset <from>

cortex actor transport list
cortex actor transport set <transport> <actor>
cortex actor transport unset <transport>
```

### 频道

```bash
cortex channel pair [platform]
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
cortex channel revoke <platform> <user_id>
cortex channel policy <platform> whitelist
```

频道订阅开关会在 daemon 运行中热应用。

## Slash 命令

三组：

- **控制** — `/help`、`/status`、`/stop`、`/permission ...`、`/think ...`、`/approve <id>`、`/deny <id>`。
- **会话/配置** — `/session ...`、`/config ...`。
- **Turn 绑定** — Skill 和 Prompt 命令，注入活跃 Turn 的执行上下文。

`/stop` 会立即执行，解析到当前 Actor 的活跃会话，中断当前 turn，并清掉该 turn 的待确认项。

Telegram 和 QQ 在平台支持的情况下会优先把 `/help`、`/status`、`/permission`、`/think`、`/session`、`/config` 呈现为卡片交互；文本 slash 命令仍保留为兜底路径。

## 会话归属

基于身份的访问控制：

| Actor | 范围 |
|-------|------|
| `local:default` | 管理员——可见所有会话 |
| `user:alice` | 规范用户——可见自己的会话 |
| `telegram:<user_id>` | 频道 Actor——可见自己的会话 |
| `whatsapp:<user_id>` | 频道 Actor——可见自己的会话 |
| `qq:<user_id>` | 频道 Actor——可见自己的会话 |

传输和频道 Actor 可通过 `cortex actor alias set` 别名到规范 Actor，实现跨接口会话连续性。一个 `http` 请求和一条 Telegram 消息可以解析到同一用户，共享历史和记忆。

频道投递遵循平台能力。Web、SSE、WebSocket、CLI 和 Telegram 可以接收实时用户可见文本。Telegram 会编辑实时草稿消息，并在完成时替换为最终响应。QQ 直接 Turn 不额外发送 Cortex 生成的处理中气泡，只投递完整最终回复；QQ 订阅其它客户端会话广播时忽略增量文本，只发送最终 `done` 响应。Telegram 和 QQ 在平台支持的情况下也会用按钮驱动权限、thinking 请求/输出、会话、配置和状态交互。Pairing 会先于 slash-command routing 执行，因此未配对频道用户只会看到配对提示，不会收到命令卡片。

会话订阅是显式设置，按已配对用户绑定，默认关闭。配对提醒会给两条管理员命令：`cortex channel approve <platform> <user_id>` 表示只配对，`cortex channel approve <platform> <user_id> --subscribe` 表示配对并订阅。配对本身不会创建会话。用户配对后第一次发送真实消息时，如果同一个 canonical actor 已有可见会话，就复用它；否则此时才创建新会话。也可以之后用 `cortex channel subscribe <platform> <user_id>` 开启，用 `cortex channel unsubscribe <platform> <user_id>` 关闭。开启后，该用户的 watcher 只订阅该客户端当前激活的会话，并在该客户端切换会话时自动重新订阅，不会把同一 canonical actor 下其它无关会话也同步过来。要让多个客户端共享同一个活跃会话，用 `cortex actor alias set` 映射到同一规范 Actor，然后显式把各客户端切换到同一个会话。

## HTTP API

### 创建会话

```http
POST /api/session
```

### 标准 Turn

```http
POST /api/turn
Content-Type: application/json

{
  "session_id": "session-id",
  "input": "解释这个变更",
  "images": [],
  "attachments": []
}
```

响应包含 `response`、`response_format` 和 `response_parts`。文本和媒体都以结构化 part 表示；媒体通过活跃传输投递，不通过文本标记触发。

### 多模态 Turn

纯文本 Turn 使用文本 LLM 端点。带图片附件的 Turn 首次 LLM 调用使用解析出的视觉端点，然后将模型的视觉理解作为文本写入后续工具循环。同一 Turn 内的后续工具调用和 LLM 调用不会持续重发图片 block，除非用户再次发送新媒体。如果视觉调用失败，Cortex 会在返回错误前从 live history 中剥离图片 block，避免单个坏媒体载荷污染同一会话的后续 Turn。

### 流式 Turn

```http
POST /api/turn/stream
Content-Type: application/json

{
  "session_id": "session-id",
  "input": "解释这个变更"
}
```

返回服务端推送事件流，三类：用户可见文本、观察者文本、工具进度。

最终 `done` 事件携带与标准 Turn 端点相同形状的结构化 `response_parts`。

发送到供应商前，Cortex 会将 live history 投影为 API 安全的消息序列。投影保留对话顺序，同时修复供应商不接受的形状，例如缺失工具结果、孤儿工具结果、重复 tool-use ID、空消息和 assistant 开头历史。

上下文压力达到配置的压缩阈值时，Cortex 会向 Journal 写入显式 compact boundary。该边界记录摘要元数据和完整替换消息历史，因此确定性重放会恢复压缩后的对话，而不是只从松散摘要重建。

## JSON-RPC

四种传输可用：HTTP (`/api/rpc`)、Unix socket、WebSocket、stdio。

### 方法

| 命名空间 | 方法 |
|----------|------|
| Session | `session/new`、`session/prompt`、`session/list`、`session/end`、`session/initialize`、`session/cancel`、`session/get` |
| Command | `command/dispatch` |
| Skill | `skill/list`、`skill/invoke`、`skill/suggestions` |
| Memory | `memory/list`、`memory/get`、`memory/save`、`memory/delete`、`memory/search` |
| Goal | `goal/create`、`goal/list`、`goal/get`、`goal/delete`、`goal/update` |
| Health | `health/check` |
| Meta | `meta/alerts` |
| Operator | `daemon/status`、`operator/dashboard`、`admin/reload-config` |
| MCP | 从 JSON-RPC 桥接到 MCP 协议 |

Operator 方法要求本地 operator 身份。`operator/dashboard` 返回结构化状态、metrics、会话、provider 模型画像、backlog，以及按 runtime 类别归一化后的最近 Journal timeline。

## Turn 事件

流式传输接收两条通道上的事件：

- **UserVisible** — 面向终端用户的最终文本、工具结果和状态更新。
- **Observer** — 内部推理轨迹、子 Turn 输出和诊断信息。

子 Turn 输出留在其父 Turn 的观察者通道中——不会泄漏到频道或用户可见流。

## 插件运行时表面

通过 `cortex-sdk` 构建的插件参与 Turn 运行时：

- 读取会话 ID、规范 Actor、来源传输、执行作用域
- 感知前台还是后台上下文
- 发出长操作的进度更新
- 向父 Turn 发送观察者文本
- 从 `ToolResult` 返回结构化媒体附件

插件仅依赖 `cortex-sdk`——与 Cortex 内部零耦合。
