# 运维

## 安装与卸载

```bash
cortex install [--system] [--id NAME]
cortex uninstall [--purge] [--id NAME]
```

默认安装创建 systemd 用户服务。`--system` 安装系统级服务（专用用户运行）。`--id` 创建具有隔离配置、数据和服务单元的命名实例。

`--purge` 移除所有实例数据（包括记忆、会话和 Journal）。移除最后一个实例时同时清理基础目录（`~/.cortex/`）。

## 服务生命周期

```bash
cortex start [--id NAME]
cortex stop [--id NAME]
cortex restart [--id NAME]
cortex status [--id NAME]
cortex permission [strict|balanced|open] [--id NAME]
cortex ps
```

`cortex ps` 列出所有已安装实例及其当前状态。`cortex status` 除了服务健康和路径外，还会显示当前权限模式、最近一次 LLM 调用的 context usage，以及累计 token spend。从绑定会话的频道触发 `/status` 时，还会展示当前会话的累计 token spend。

## 浏览器扩展

```bash
cortex node setup          # 安装 Node.js 桥接
cortex node status         # 检查桥接健康
cortex browser enable      # 启用浏览器扩展
cortex browser disable     # 禁用浏览器扩展
cortex browser status      # 检查扩展状态
```

`browser enable` 和 `browser disable` 会在正常用户态服务路径中热应用。

## 频道操作

```bash
cortex channel pair [platform]                          # 查看配对状态
cortex channel approve <platform> <user_id>             # 只配对
cortex channel approve <platform> <user_id> --subscribe # 配对并订阅该用户
cortex channel subscribe <platform> <user_id>           # 为一个已配对用户开启订阅
cortex channel unsubscribe <platform> <user_id>         # 为一个已配对用户关闭订阅
cortex channel revoke <platform> <user_id>              # 撤销访问
cortex channel policy <platform> whitelist              # 设置访问策略
```

QQ 使用官方 Bot 回复流程。直接用户 Turn 不额外发送 Cortex 生成的处理中气泡，只投递完整最终响应。QQ 订阅其它接口发起的会话时，只接收最终 `done` 消息；增量文本会被抑制，避免完整答案前出现碎片气泡。

Telegram 和 QQ 在平台支持的情况下会优先用卡片承载 `/help`、`/status`、`/permission`、`/session`、`/config`；按钮交互会刷新当前卡片，而不是不断新增管理消息。文本 slash 命令仍保留为兜底路径。QQ 会先做 pairing，再进入命令路由；未配对用户首条发送 `/status` 时只会收到配对提示，不会附带命令卡片。

频道运行时状态位于 `channels/<platform>/`。认证配置（`auth.json`）是声明式的，由用户管理；策略和配对状态由运行时管理。

## Actor 操作

```bash
cortex actor alias list
cortex actor alias set telegram:123456789 user:alice
cortex actor alias unset telegram:123456789

cortex actor transport list
cortex actor transport set all user:alice    # 一次绑定所有传输
cortex actor transport set http user:alice   # 绑定单个传输
cortex actor transport unset http
```

Actor 别名实现跨接口会话连续性。Telegram 消息和 HTTP 请求来自同一个人时解析为同一规范 Actor，共享会话和记忆。

会话订阅是显式配置，按已配对用户绑定，默认关闭。配对提醒会同时给出两种选择：`cortex channel approve <platform> <user_id>` 只配对，`cortex channel approve <platform> <user_id> --subscribe` 配对并订阅。配对本身不会分配会话。用户配对后第一次发送真实消息时，如果同一个 canonical actor 已有可见会话，就复用它；否则此时才创建新会话。`cortex channel subscribe <platform> <user_id>` 为该已配对用户开启 watcher；`cortex channel unsubscribe <platform> <user_id>` 关闭。watcher 只跟随该客户端当前激活的会话，不会镜像同一 canonical actor 下其它无关会话。本地传输可通过别名或绑定加入同一连续性。`actor alias` 用于身份等价，`actor transport` 用于传输级默认归属。

频道订阅开关会在 daemon 运行中热应用。`/stop` 会解析到当前 Actor 的活跃会话，中断运行中的 turn，并清掉该 turn 的待确认项。

## 诊断

多种路径到同一运行时状态：

| 方法 | 范围 |
|------|------|
| `cortex status` | CLI——实例健康、运行时间、权限模式、last-call context、累计 tokens |
| `/status` | Slash 命令——从会话内部获取相同数据 |
| `GET /api/daemon/status` | HTTP——程序化访问 |
| `GET /api/operator/dashboard?limit=80` | HTTP——operator dashboard，包含状态、metrics、会话、provider profile、backlog 和归一化 turn timeline |
| `command/dispatch` + `/status` | JSON-RPC——远程诊断 |
| `operator/dashboard` | JSON-RPC——通过 HTTP、socket、WebSocket 或 stdio 获取同一 operator dashboard |

所有路径反映相同底层状态：Actor 映射、会话计数、传输健康、记忆统计、模型路由画像、backlog、元认知警报，以及按 lifecycle、message、LLM、tool、permission、workspace、retrieval、memory、control、guardrail、scheduler 或其它 runtime 类别归一化后的最近 Journal timeline。

## 备份与重置

### 关键备份路径

| 路径 | 内容 |
|------|------|
| `~/.cortex/<instance>/config.toml` | 实例配置 |
| `~/.cortex/<instance>/actors.toml` | 身份映射 |
| `~/.cortex/<instance>/mcp.toml` | MCP 服务器定义 |
| `~/.cortex/<instance>/prompts/` | 自定义 Prompt 文件 |
| `~/.cortex/<instance>/skills/` | 自定义 Skills |
| `~/.cortex/<instance>/data/` | Journal、嵌入、记忆图谱、task 与 goal 状态 |
| `~/.cortex/<instance>/memory/` | 持久记忆存储 |
| `~/.cortex/<instance>/sessions/` | 会话历史 |

### 重置

```bash
cortex reset                   # 重置运行时状态，保留配置
cortex reset --factory         # 重置为安装默认值
cortex reset --force           # 跳过确认提示
```

## 验证

```bash
# 使用本仓库 docker-compose.yml 的权威 gate
./scripts/gate.sh --docker

# 发布 commit 存在后使用的 release gate
./scripts/gate.sh --docker --require-clean
```

规范使用 Docker Compose：统一通过仓库入口运行验证。`./scripts/gate.sh --docker` 是标准验证命令，
也是唯一具备发布权威的路径。它会运行本仓库 `docker-compose.yml` 的 `dev` 服务，该服务由仓库
`Dockerfile` 基于 `rust:latest` 构建。直接在宿主机跑 `cargo` 只能用于定位问题，不能替代仓库 Docker Compose gate。

Gate 会检查 Rust 警告抑制、编译器警告抑制 flag、格式、文档/包表面漂移、secret 与个人路径、
严格 clippy，以及完整 workspace 测试。不存在可以忽略的 warning。

Release candidate 还应生成行为证据报告：

```bash
docker compose run --rm dev ./scripts/release-behavior-report.sh --run
docker compose run --rm dev ./scripts/soak-fault-harness.sh --run
```

报告会记录 memory、retrieval/RAG、tool、safety、operator timeline、long-task recovery、replay 和 soak posture 的目标行为套件。Bounded soak/fault harness 覆盖 provider、channel、SQLite、plugin、disk/config、rate-limit/backpressure、replay determinism 和 reconnect 证据。它们都需要和严格 gate 输出一起附到 release review；24h/72h/7d 长时间 soak 在有条件时作为单独附件。

用于在同一仓库 Docker Compose `dev` 服务内排查单项失败的等价命令：

```bash
docker compose run --rm dev cargo fmt --all --check
docker compose run --rm dev cargo clippy --workspace --all-targets --all-features -- \
  -D warnings -W clippy::pedantic -W clippy::nursery
docker compose run --rm dev cargo test --workspace --all-features
```
