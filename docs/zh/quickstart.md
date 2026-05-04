# 快速开始

从零到运行实例。

## 前置条件

- Linux (x86_64)
- systemd（服务管理）
- 一个 LLM 供应商 API Key

## 首次运行

首次启动时，Cortex 会运行一次 bootstrap 对话——这是真正的首次会面。Bootstrap 建立实例的初始名称或明确的未命名状态，并收集你的偏好语言、工作、环境、沟通方式、自主权预期、审批边界和第一个工作上下文。所有信息会初始化 Executive Prompt 状态，塑造实例之后的思考和沟通方式。

## 安装

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

安装脚本下载最新发布二进制，运行 `cortex install`，并以 systemd 用户服务启动 Daemon。
官方预构建安装资产目前只发布 Linux x86_64（`linux-amd64`）。
在 macOS、ARM Linux 或其他平台上，请先从源码构建，直到对应 release asset 发布。

环境变量必须放在 `bash -s -- install` 这一侧。若放在 `curl` 前面，只会作用于下载步骤，不会传给 `cortex install`。

### 安装变体

```bash
# 命名实例（隔离配置、数据和服务）
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install --id work

# 系统级服务（专用用户运行，注销后存活）
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install --system
```

### 完整体验

如果希望一次性完成 Daemon、供应商配置、浏览器支持、消息频道凭据和官方开发插件安装，可以使用这个形式。将所有占位值替换为你自己的值；不要把密钥粘贴到共享日志或截图中。

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_PROVIDER="anthropic" \
  CORTEX_API_KEY="your-llm-api-key" \
  CORTEX_MODEL="your-model" \
  CORTEX_LLM_PRESET="full" \
  CORTEX_PERMISSION_LEVEL="balanced" \
  CORTEX_EMBEDDING_PROVIDER="openai" \
  CORTEX_EMBEDDING_MODEL="text-embedding-3-small" \
  CORTEX_BRAVE_KEY="your-brave-key" \
  CORTEX_TELEGRAM_TOKEN="your-telegram-bot-token" \
  CORTEX_QQ_APP_ID="your-qq-app-id" \
  CORTEX_QQ_APP_SECRET="your-qq-app-secret" \
  bash -s -- install && \
  "$HOME/.local/bin/cortex" browser enable && \
  "$HOME/.local/bin/cortex" plugin install by-scott/cortex-plugin-dev --yes
```

`browser enable` 会立即热应用。进程隔离插件安装也会热应用。新安装的强信任 native 插件第一次加载共享库时，可能仍需要一次 daemon 重启。

### 从源码构建

```bash
docker compose run --rm dev cargo build --release
./target/release/cortex install
```

## 本地代码 Demo

二进制可用后，`cortex demo` 会创建一个不启动服务的首次使用 fixture。它默认使用 `demo` 实例 id，写入偏向 Ollama 的配置，保持插件禁用，加入一个本地 coding skill，并把示例 workspace 放在 protected runtime root 之外。

```bash
cortex demo
cortex doctor --id demo
cortex doctor --id demo --json
cortex policy lint --id demo
```

完整 demo 路径见[本地 Coding Agent](local-coding-agent.md)，Ollama/vLLM 配置见[本地模型](local-models.md)。

## 安装时变量

`cortex install` 读取的环境变量：

| 变量 | 用途 |
|------|------|
| `CORTEX_API_KEY` | 主供应商 API Key |
| `CORTEX_PROVIDER` | 供应商名称（默认：`anthropic`） |
| `CORTEX_MODEL` | 模型标识符 |
| `CORTEX_LLM_PRESET` | 端点预设：`minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_PERMISSION_LEVEL` | 安装时确认策略：`strict` / `balanced` / `open`（默认 `balanced`） |
| `CORTEX_EMBEDDING_PROVIDER` | 嵌入供应商 |
| `CORTEX_EMBEDDING_MODEL` | 嵌入模型 |
| `CORTEX_BRAVE_KEY` | Brave Search API Key |
| `CORTEX_TELEGRAM_TOKEN` | Telegram 机器人令牌 |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp 令牌 |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ 机器人凭据 |

推荐权限级别：

- `balanced`：默认且推荐。自动放行 `Allow`，对 `Review` 及以上要求确认。
- `strict`：更保守。只有 `Allow` 无需确认。
- `open`：仅适用于强信任的单用户本机。所有非阻断工具默认直接执行。

也可以在安装后热切换：

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

## 验证

```bash
cortex status          # 检查 Daemon 健康
cortex doctor          # 检查本地就绪状态和 policy 姿态
cortex doctor --json   # 输出机器可读 finding 和 remediation hints
cortex                 # 启动交互 REPL
```

`cortex status` 现在还会显示当前权限模式、最近一次 LLM 调用的 context usage，以及累计 token spend。`cortex doctor` 是只读就绪报告，覆盖 service state、config、provider key 姿态、权限模式、插件、频道、policy finding、protected runtime root 和本地模型端点线索。`--json` 形式面向脚本和 issue report，仍然是只读。

## 浏览器扩展与插件

官方开发插件 [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev) 提供项目维护工具；Cortex 有意把这些高层开发能力放在插件里，而不是塞进 daemon core。

```bash
cortex browser enable
cortex plugin install by-scott/cortex-plugin-dev --yes
```

打包插件安装需要签名。`--yes` 会在签名验证通过后，把该 publisher key 记录到本机信任库；如果希望手动确认 key fingerprint，可以在交互式终端里省略它。只有在安装的插件第一次加载强信任 native 共享库时才需要额外重启。

## Actor 映射

将多个传输映射到一个身份，实现跨接口会话连续性：

```bash
cortex actor alias set telegram:123456789 user:alice
cortex actor transport set all user:alice
```

## 频道订阅

消息频道需要先配对。配对提醒会给出两种形式：

```bash
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
```

订阅绑定到这个已配对用户，而不是整个平台。后续修改使用：

```bash
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
```

这些订阅变更会热应用，无需重启，且 watcher 只跟随该客户端当前激活的会话。

## 常用命令

```bash
cortex start                  # 启动 Daemon
cortex stop                   # 停止 Daemon
cortex restart                # 重启 Daemon
cortex ps                     # 列出所有实例
cortex demo                   # 创建本地首次使用 fixture
cortex status                 # 实例健康
cortex doctor                 # 就绪状态和 policy 姿态
cortex doctor --json          # 机器可读就绪报告
cortex permission balanced    # 热切换权限模式
cortex plugin list            # 已安装插件
cortex actor alias list       # 身份映射
cortex actor transport list   # 传输绑定
```

## 下一步

- [安全使用](safe-use.md)：推荐本地运行姿态、插件信任和当前非目标
- [本地 Coding Agent](local-coding-agent.md) - 生成式 demo fixture 和有边界的 coding loop
- [本地模型](local-models.md) - Ollama 和 vLLM 配置
- [配置](config.md) — 配置布局、供应商、权限模式、热重载
- [Executive](executive.md) — Prompt 状态、bootstrap、运行时策略上下文
- [运维](ops.md) — 服务生命周期、频道、诊断
- [插件开发](plugins.md) — 插件边界、manifest、打包
