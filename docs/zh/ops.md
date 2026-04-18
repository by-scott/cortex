# 运维指南

## 安装

### 二进制安装

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install
```

二进制文件安装到 `~/.local/bin/cortex`。

### 实例安装

```bash
cortex install              # 默认实例的用户级 systemd 服务
cortex install --system     # 系统级 systemd 服务（需要 sudo）
cortex install --id work    # 命名实例的用户级服务
```

**用户级**安装 systemd 用户服务，通过 `systemctl --user` 管理。

**系统级**安装 systemd 系统服务，通过 `sudo systemctl` 管理。

### 安装时的环境变量

安装时设置的环境变量会被写入 systemd 服务单元：

```bash
CORTEX_API_KEY="sk-..." \
CORTEX_PROVIDER="zai" \
CORTEX_MODEL="glm-5.1" \
CORTEX_LLM_PRESET="full" \
CORTEX_EMBEDDING_PROVIDER="ollama" \
CORTEX_EMBEDDING_MODEL="nomic-embed-text" \
CORTEX_BRAVE_KEY="BSA..." \
cortex install
```

---

## 服务管理

### 生命周期

```
install --> start <--> stop --> restart --> uninstall [--purge] / reset [--factory]
```

### 命令

| 命令 | 描述 |
|------|------|
| `cortex install [--system] [--id ID]` | 安装为 systemd 服务 |
| `cortex start [--id ID]` | 启动守护进程 |
| `cortex stop [--id ID]` | 停止守护进程 |
| `cortex restart [--id ID]` | 重启守护进程 |
| `cortex status [--id ID]` | 显示守护进程状态 |
| `cortex ps` | 列出所有运行中的实例 |
| `cortex uninstall [--purge] [--id ID]` | 移除服务；--purge 删除所有数据 |
| `cortex reset [--factory] [--force] [--id ID]` | 重置实例数据；--factory 删除全部 |
| `cortex node setup` | 下载并配置 Node.js 运行时 |
| `cortex node status` | 显示 Node.js 运行时状态 |
| `cortex browser enable` | 启用无头浏览器子系统 |
| `cortex browser status` | 显示浏览器子系统状态 |
| `cortex help [subcommand]` | 显示帮助 |

所有命令接受 `--id <instance>` 来指向命名实例。

---

## 插件管理

默认不安装任何插件。核心运行时自带 17 个工具和 5 个技能。额外的工具和技能可通过插件获取。

### 官方插件

[cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) 插件提供开发工具（代码导航、git、docker、任务等）和工作流技能：

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

### 安装来源

| 来源 | 示例 |
|------|------|
| GitHub | `cortex plugin install owner/repo` |
| URL | `cortex plugin install https://example.com/plugin.cpx` |
| 本地 .cpx | `cortex plugin install ./my-plugin.cpx` |
| 本地目录 | `cortex plugin install ./my-plugin/` |

### 命令

| 命令 | 描述 |
|------|------|
| `cortex plugin install <source>` | 从 4 种来源之一安装 |
| `cortex plugin uninstall <name> [--purge]` | 移除插件；--purge 删除所有文件 |
| `cortex plugin list` | 列出已安装的插件 |
| `cortex plugin pack <dir> [output.cpx]` | 将目录打包为 .cpx 归档 |

### 存储和启用

插件全局安装到 `~/.cortex/plugins/<name>/`。

通过 `config.toml` 按实例启用：

```toml
[plugins]
enabled = ["cortex-plugin-dev"]
```

更改已启用插件后重启守护进程。

---

## 多实例

每个命名实例完全隔离：

- 配置、数据、记忆、提示、技能、会话
- 独立的 Unix socket
- 独立的 systemd 服务

服务命名：

| 实例 | 服务名称 |
|------|----------|
| 默认 | `cortex` |
| 命名 | `cortex@<id>` |

---

## 目录布局

```
~/.cortex/
  providers.toml                          # 全局提供商凭据
  plugins/                                # 全局插件存储（默认为空）
  default/                                # 默认实例
    config.toml                           # 实例配置
    mcp.toml                              # MCP 服务器定义
    data/
      cortex.db                           # 主数据库
      cortex.sock                         # Unix 域套接字
      embedding_store.db                  # 嵌入向量
      memory_graph.db                     # 记忆关系图
      cron_queue.json                     # 计划任务
      model_info.json                     # 缓存的模型元数据
      vision_caps.json                    # 缓存的视觉能力
      defaults.toml                       # 运行时默认值
      node/                               # Node.js 运行时和模块
    memory/                               # 记忆存储
    prompts/
      soul.md                             # Soul 层提示
      identity.md                         # Identity 层提示
      behavioral.md                       # Behavioral 层提示
      user.md                             # User 层提示
      .initialized                        # 初始化标记
      system/                             # 18 个系统提示模板
    sessions/                             # 会话数据
    skills/
      system/                             # 内置技能
        deliberate/
        diagnose/
        review/
        orient/
        plan/
    channels/
      telegram/
        auth.json                         # 机器人令牌和模式
        paired_users.json                 # 已批准用户
        pending_pairs.json                # 待批准
        policy.json                       # 访问策略
      whatsapp/
        auth.json                         # 访问令牌和配置
        ...
  work/                                   # 命名实例（相同结构）
    config.toml
    mcp.toml
    data/
    memory/
    prompts/
    sessions/
    skills/
    channels/
```

---

## 监控

### CLI

- `cortex status` -- 守护进程运行时间、会话数、记忆统计
- `cortex ps` -- 列出所有运行中的实例

### HTTP 端点

| 端点 | 描述 |
|------|------|
| `GET /api/health` | 健康检查（认证/限流豁免） |
| `GET /api/metrics/structured` | 结构化指标（认证/限流豁免） |
| `GET /api/daemon/status` | 守护进程状态和运行时长 |
| `GET /api/audit/summary` | 审计事件摘要 |
| `GET /api/audit/health` | 审计健康分数 |

### Web 界面

全部内嵌于二进制文件，无外部依赖：

| URL | 描述 |
|-----|------|
| `/` | 聊天界面 |
| `/dashboard.html` | 指标和状态仪表盘 |
| `/audit.html` | 决策审计日志查看器 |

---

## 心跳引擎

守护进程运行后台心跳循环，按优先级顺序执行维护任务。

### 非 LLM 操作

无条件按时执行：

- 记忆整合
- 记忆衰减处理
- 待嵌入向量生成
- 技能演进检查
- 状态检查点

### LLM 操作

受限制以避免过度 API 使用：

- 提示演进
- 深度反思
- 定时任务执行

### 紧迫性排序

值越低 = 优先级越高：

| 任务 | 紧迫性 |
|------|--------|
| DeprecateExpired | 10 |
| EmbedPending | 20 |
| Consolidate | 30 |
| EvolveSkills | 40 |
| Checkpoint | 50 |
| CronDue | 60 |
| SelfUpdate | 70 |
| DeepReflection | 80 |

---

## 网络

### 传输方式

| 传输方式 | 绑定 | 用途 |
|----------|------|------|
| HTTP | `127.0.0.1:PORT` | Web UI、REST API、SSE、WebSocket |
| Unix socket | `~/.cortex/<instance>/data/cortex.sock` | CLI、ACP/MCP 桥接（模式 0700） |
| stdio | stdin/stdout | ACP 和 MCP 协议 |

### CORS

- 允许的源：仅 localhost
- 显式的方法和头部白名单

### 信号处理

- `SIGHUP`：优雅处理（记录日志并继续，不重启）

---

## 备份和恢复

### 运行时备份

使用 SQLite 的 `.backup` 命令获取一致的数据库快照：

```bash
sqlite3 ~/.cortex/default/data/cortex.db ".backup /path/to/backup.db"
```

### 停止时备份

完整目录复制：

```bash
cp -r ~/.cortex/default /path/to/backup/
```

### 恢复

用备份替换实例目录并重启：

```bash
cortex stop
rm -rf ~/.cortex/default
cp -r /path/to/backup/ ~/.cortex/default
cortex start
```

---

## 安全

### 网络隔离

- HTTP 仅绑定到 `127.0.0.1`（不暴露到网络）
- Unix socket 创建时模式为 `0700`（仅所有者访问）

### HTTP 安全头

- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin`

### 输入验证

- `session_id`：字母数字、连字符、下划线、点；最长 256 字符
- 请求体限制：2 MB
- 需要 JSON content type

### 可选安全功能

- **JWT 认证**：通过 `[auth]` 配置段落启用
- **TLS**：通过 `[tls]` 配置段落启用
- **速率限制**：通过 `[rate_limit]` 配置段落配置

### 工具风险评估

每次工具调用在 4 个轴上评估：

- **范围**：影响系统的范围大小
- **可逆性**：操作是否可撤销
- **副作用**：外部或持久性变更
- **数据敏感性**：对敏感信息的访问

---

## 故障排除

### 查看日志

```bash
journalctl --user -u cortex          # 默认实例
journalctl --user -u cortex@work     # 命名实例
```

### 常见问题

**守护进程无响应**

检查套接字文件是否存在：

```bash
ls -la ~/.cortex/default/data/cortex.sock
```

如果不存在，守护进程未运行。使用 `cortex start` 启动。

**端口冲突**

如果 HTTP 端口已被占用，在配置中设置地址为 `0.0.0.0:0` 自动分配空闲端口，或在 `[daemon].addr` 中选择不同端口。

**过期数据**

重置非关键运行时数据：

```bash
cortex reset
```

**完全工厂重置**

删除所有实例数据并从默认值重新生成：

```bash
cortex reset --factory --force
```

**嵌入模型不可用**

验证嵌入提供商可达且 `[embedding]` 配置中的模型名称正确。检查日志中的连接错误。
