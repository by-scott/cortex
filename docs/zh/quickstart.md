# 快速开始

从零到运行实例。

## 前置条件

- Linux (x86_64)
- systemd（服务管理）
- 一个 LLM 供应商 API Key（Anthropic、OpenAI 或 Ollama）

## 安装

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install
```

安装脚本下载最新发布二进制，运行 `cortex install`，并以 systemd 用户服务启动 Daemon。

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
  CORTEX_EMBEDDING_PROVIDER="openai" \
  CORTEX_EMBEDDING_MODEL="text-embedding-3-small" \
  CORTEX_BRAVE_KEY="your-brave-key" \
  CORTEX_TELEGRAM_TOKEN="your-telegram-bot-token" \
  CORTEX_QQ_APP_ID="your-qq-app-id" \
  CORTEX_QQ_APP_SECRET="your-qq-app-secret" \
  bash -s -- install && \
  "$HOME/.local/bin/cortex" browser enable && \
  "$HOME/.local/bin/cortex" plugin install by-scott/cortex-plugin-dev && \
  "$HOME/.local/bin/cortex" restart
```

### 从源码构建

```bash
docker compose run --rm dev cargo build --release
./target/release/cortex install
```

## 安装时变量

`cortex install` 读取的环境变量：

| 变量 | 用途 |
|------|------|
| `CORTEX_API_KEY` | 主供应商 API Key |
| `CORTEX_PROVIDER` | 供应商名称（默认：`anthropic`）|
| `CORTEX_MODEL` | 模型标识符 |
| `CORTEX_LLM_PRESET` | 端点预设：`minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_EMBEDDING_PROVIDER` | 嵌入供应商 |
| `CORTEX_EMBEDDING_MODEL` | 嵌入模型 |
| `CORTEX_BRAVE_KEY` | Brave Search API Key（用于 `web_search` 工具）|
| `CORTEX_TELEGRAM_TOKEN` | Telegram 机器人令牌 |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp 令牌 |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ 机器人凭据 |

## 首次运行

首次启动时，Cortex 运行一次 bootstrap 对话——你与实例之间的真正首次会面。Bootstrap 建立实例的初始名称或明确的未命名状态，并收集你的偏好语言、工作、环境、沟通方式、自主权预期、审批边界和第一个工作上下文。所有信息会初始化 Executive Prompt 层，塑造实例之后的思考和沟通方式。

## 验证

```bash
cortex status          # 检查 Daemon 健康
cortex                 # 启动交互 REPL
```

## 浏览器扩展与插件

```bash
cortex browser enable
cortex plugin install by-scott/cortex-plugin-dev
cortex restart
```

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

## 常用命令

```bash
cortex start                  # 启动 Daemon
cortex stop                   # 停止 Daemon
cortex restart                # 重启 Daemon
cortex ps                     # 列出所有实例
cortex status                 # 实例健康
cortex plugin list            # 已安装插件
cortex actor alias list       # 身份映射
cortex actor transport list   # 传输绑定
```

## 下一步

- [配置](config.md) — 配置布局、供应商、热重载
- [Executive](executive.md) — Prompt 层、bootstrap、Skills、LLM 输入面
- [运维](ops.md) — 服务生命周期、频道、诊断
- [插件开发](plugins.md) — SDK、清单、分发
