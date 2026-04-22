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
cortex ps
```

`cortex ps` 列出所有已安装实例及其当前状态。

## 浏览器扩展

```bash
cortex node setup          # 安装 Node.js 桥接
cortex node status         # 检查桥接健康
cortex browser enable      # 启用浏览器扩展
cortex browser status      # 检查扩展状态
```

## 频道操作

```bash
cortex channel pair [platform]                         # 查看配对状态
cortex channel approve <platform> <user_id>            # 只配对
cortex channel approve <platform> <user_id> --subscribe # 配对并订阅该用户
cortex channel subscribe <platform> <user_id>          # 为一个已配对用户开启订阅
cortex channel unsubscribe <platform> <user_id>        # 为一个已配对用户关闭订阅
cortex channel revoke <platform> <user_id>             # 撤销访问
cortex channel policy <platform> whitelist             # 设置访问策略
```

QQ 使用官方 Bot 回复流程。直接用户 Turn 不额外发送 Cortex 生成的处理中气泡，只投递完整最终响应。QQ 订阅其它接口发起的会话时，只接收最终 `done` 消息；增量文本会被抑制，避免完整答案前出现碎片气泡。

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

会话订阅是显式配置，按已配对用户绑定，默认关闭。配对提醒会同时给出两种选择：`cortex channel approve <platform> <user_id>` 只配对，`cortex channel approve <platform> <user_id> --subscribe` 配对并订阅。`cortex channel subscribe <platform> <user_id>` 为该已配对用户开启 watcher；`cortex channel unsubscribe <platform> <user_id>` 关闭。本地传输可通过别名或绑定加入同一连续性。`actor alias` 用于身份等价，`actor transport` 用于传输级默认归属。

## 诊断

多种路径到同一运行时状态：

| 方法 | 范围 |
|------|------|
| `cortex status` | CLI——实例健康、运行时间、活跃会话 |
| `/status` | Slash 命令——从会话内部获取相同数据 |
| `GET /api/daemon/status` | HTTP——程序化访问 |
| `command/dispatch` + `/status` | JSON-RPC——远程诊断 |

所有路径反映相同底层状态：Actor 映射、会话计数、传输健康、记忆统计和元认知警报。

## 备份与重置

### 关键备份路径

| 路径 | 内容 |
|------|------|
| `~/.cortex/<instance>/config.toml` | 实例配置 |
| `~/.cortex/<instance>/actors.toml` | 身份映射 |
| `~/.cortex/<instance>/mcp.toml` | MCP 服务器定义 |
| `~/.cortex/<instance>/prompts/` | 自定义 Prompt 层 |
| `~/.cortex/<instance>/skills/` | 自定义 Skills |
| `~/.cortex/<instance>/data/` | Journal、嵌入、记忆图谱 |
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
# 代码格式
cargo fmt --all -- --check

# Lint
docker compose run --rm dev cargo clippy --workspace --all-targets -- \
  -D warnings -W clippy::pedantic -W clippy::nursery

# 测试
docker compose run --rm dev cargo test --workspace
```
