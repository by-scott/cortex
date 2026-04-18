# 快速开始

从零开始运行 Cortex 的指南。

## 前提条件

- 带有 systemd 的 Linux（用于服务管理）
- 支持的提供商 API 密钥：Anthropic、OpenAI、ZhipuAI（国际版或国内版）、Kimi、MiniMax、OpenRouter，或本地 Ollama 实例

## 安装二进制文件

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install
```

二进制文件安装到 `~/.local/bin/cortex`。可通过 `CORTEX_INSTALL_DIR` 覆盖。

指定版本：`bash -s install --version 1.0.0`

## 快速设置

环境变量仅在 `cortex install` 期间读取，用于生成 config.toml：

| 变量 | 用途 | 默认值 |
|------|------|--------|
| CORTEX_API_KEY | 提供商 API 密钥 | （必填） |
| CORTEX_PROVIDER | 提供商名称 | anthropic |
| CORTEX_MODEL | 模型标识符 | （提供商默认值） |
| CORTEX_BASE_URL | 自定义端点 URL | （提供商默认值） |
| CORTEX_LLM_PRESET | 子端点预设 | full |
| CORTEX_EMBEDDING_PROVIDER | 嵌入提供商 | ollama |
| CORTEX_EMBEDDING_MODEL | 嵌入模型 | nomic-embed-text |
| CORTEX_BRAVE_KEY | Brave Search API 密钥 | （空） |
| CORTEX_TELEGRAM_TOKEN | Telegram 机器人令牌 | （空） |
| CORTEX_WHATSAPP_TOKEN | WhatsApp 访问令牌 | （空） |

示例：`CORTEX_API_KEY="sk-ant-..." cortex install`

使用 OpenAI：`CORTEX_API_KEY="sk-..." CORTEX_PROVIDER="openai" CORTEX_MODEL="gpt-4o" cortex install`

安装将创建：

- 包含提供商设置的 config.toml
- providers.toml（全局提供商注册表）
- 4 个提示层（Soul、Identity、Behavioral、User）
- 系统模板和认知技能
- 一个 systemd 用户服务

安装后，17 个核心工具和 5 个认知技能即可使用。要添加开发工具（代码导航、git 工作流、docker 等），请安装官方插件：

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

## 首次对话 -- 引导启动

首次启动时，Cortex 运行引导启动序列 -- 一次真实的初次相遇：

1. 交换姓名（你的和实例的）
2. 实例个性通过对话涌现
3. 分享角色、专业领域、环境、偏好
4. 建立工作协议和目标

这不是表单 -- 而是一次对话。回答会填充 Identity 和 User 提示层。

## 运行模式

- 交互式 REPL：`cortex`
- 单次提示：`cortex "your question"`
- 管道输入：`cat file.txt | cortex "summarize"`
- 命名会话：`cortex --session name "prompt"`
- 命名实例：`cortex --id work "prompt"`
- Web UI：http://127.0.0.1:PORT/（端口通过 `cortex status` 查看）
- 仪表盘：http://127.0.0.1:PORT/dashboard.html
- 审计追踪：http://127.0.0.1:PORT/audit.html

## 服务管理

```bash
cortex status    # 服务健康状态、HTTP 地址、LLM 信息
cortex start     # 启动守护进程
cortex stop      # 停止守护进程
cortex restart   # 重启守护进程
```

## Node 和浏览器设置

MCP 服务器通常需要 Node.js。Cortex 提供内置命令来管理 Node.js 环境和浏览器集成：

```bash
cortex node setup     # 安装 Node.js + pnpm（用于 MCP 服务器）
cortex node status    # 显示 Node.js 环境状态
cortex browser enable # 配置 Chrome DevTools MCP 服务器
cortex browser status # 显示浏览器集成状态
```

在添加任何使用 `npx` 的 MCP 服务器之前，请先运行 `cortex node setup`。

## 插件

Cortex 自带 17 个核心工具。额外的工具和技能可通过插件获取：

```bash
cortex plugin install owner/repo  # 从 GitHub 安装
cortex plugin list                # 列出已安装的插件
cortex plugin uninstall name      # 移除插件
```

官方 [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 插件添加开发工具（代码导航、git、docker、任务等）和工作流技能。参见 [docs/plugins.md](plugins.md) 了解如何开发自己的插件。

## MCP 服务器

添加到 `~/.cortex/<instance>/mcp.toml`：

```toml
[[servers]]
name = "fs"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
```

添加后重启守护进程。工具以 `mcp_{server}_{tool}` 形式出现。

## 多实例

```bash
CORTEX_API_KEY="key" cortex install --id work
cortex --id work
cortex ps
```

每个实例拥有独立的配置、数据、记忆、提示、技能、服务和套接字。

## 后续步骤

- 配置参考：[docs/config.md](config.md)
- 使用参考：[docs/usage.md](usage.md)
- 运维指南：[docs/ops.md](ops.md)
- 插件指南：[docs/plugins.md](plugins.md)
