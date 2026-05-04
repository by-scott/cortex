# Policy Profiles

`profiles/` 包含常见本地优先姿态的 `config.toml` 示例片段。当前还没有自动加载这些文件的 `cortex profile` 命令。需要把相关片段复制进 `~/.cortex/<instance>/config.toml`，然后验证实例：

```bash
cortex doctor --json
cortex policy lint
```

Policy profile 会调整 risk scoring、确认门、allowlist、denylist、禁用工具、插件启用、memory extraction 和 provider 默认值。它们是审查控制，不是沙箱隔离。

## 可用 Profile

| 文件 | 用途 |
|------|------|
| `profiles/personal-local.toml` | 受信任单用户本地工作，balanced review 姿态，插件默认禁用。 |
| `profiles/coding-agent.toml` | 本地仓库工作，禁用 web/channel 工具，插件禁用，写入和进程动作保持确认门。 |
| `profiles/local-vllm.toml` | 本地 OpenAI-compatible vLLM 配置，配合保守 coding-tool policy。 |
| `profiles/strict-safe.toml` | 高谨慎读/审查姿态，拒绝 mutation、process、web、MCP 和 memory-save 工具。 |
| `profiles/mcp-gateway.toml` | 已审查 MCP/plugin gateway 的起点；使用前把示例工具名替换成实际注册工具名。 |

## 应用方式

1. 安装或创建目标实例。
2. 打开 `~/.cortex/<instance>/config.toml`。
3. 只复制你打算使用的 profile section。
4. 保留复制过来的 policy 注释，方便未来 operator 理解意图。
5. 运行 `cortex doctor --json` 和 `cortex policy lint`。
6. 对高影响工具，使用前运行 `cortex policy simulate <tool> --effect <effect> --actor <actor>`。

本地 vLLM 还需要添加[本地模型](local-models.md)中展示的 provider registry 条目。`profiles/local-vllm.toml` 引用了 `local-vllm`，但 provider registry 仍在 `~/.cortex/providers.toml`。

## 安全边界

- 不要在共享机器、插件多、或外部副作用重的工作流中使用 `open` permission。
- 未审查插件保持禁用。对已审查插件，为每个暴露工具添加具体 `[risk.tools.<name>]` 条目。
- 对会写文件、发消息、部署、发布、轮换凭据或花钱的工具，避免宽泛 wildcard allowlist。
- 会接收 network、MCP、plugin 或其它 hostile evidence 的 profile，默认关闭自动 durable memory extraction。
- Native plugin 是受信任的进程内代码。这些 profile 不会让 native code 变成沙箱化代码。
- 如果需要 containment，必须另行使用 OS/container 隔离。
