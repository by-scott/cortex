# 本地 Coding Agent

这个路径让新用户先运行一个有边界的本地 coding workflow，再考虑启用宽工具、插件或频道。它不新增沙箱隔离。它使用 Cortex 现有的 policy、permission、journal、replay、memory、RAG、tool effect、plugin governance 和 protected-root 规则。

## 创建 Fixture

```bash
cortex demo
```

该命令会创建：

- 当前 Cortex home 下的 `demo` 实例；
- 面向 `ollama` 和 `qwen2.5-coder:7b` 的配置；
- 通过 `risk.auto_approve_up_to = "Review"` 保持 `balanced` 权限姿态；
- 不启用任何插件；
- 一个 `local-coding-demo` skill；
- 位于 `~/.cortex/workspaces/demo` 的 workspace，且不在 protected runtime root 内。

可以用 `--id NAME` 指定其他实例 id，用 `--home PATH` 指定其他 Cortex home，用 `--force` 刷新 demo 自有文件。

## 检查就绪状态

```bash
ollama pull qwen2.5-coder:7b
cortex doctor --id demo
cortex doctor --id demo --json
cortex policy lint --id demo
```

`cortex doctor` 是只读 readiness 和 policy 姿态报告。`--json` 形式适合脚本和 issue report，因为它包含机器可读 findings 和 remediation hints。它默认不主动连接 provider，不启动 daemon，也不声称沙箱隔离。Ollama 和 vLLM 配置细节见[本地模型](local-models.md)。

## 运行 Demo

```bash
cortex install --id demo
cortex --id demo
```

然后输入：

```text
Use the local-coding-demo skill on ~/.cortex/workspaces/demo. Fix the formatter test and verify it with python3 -m unittest discover -s tests.
```

生成的 workspace 很小。期望循环是读取、计划、修改、验证，并报告变更文件、测试、风险和下一步。

## 边界

- Workspace 是项目数据；实例 home 是 protected runtime state。
- Policy 和 risk gate 是审查控制，不是 OS/container 隔离。
- Fixture 不启用任何插件。
- 如果之后启用 native plugin，它仍是强信任的进程内代码。
- Retrieved file、tool output 和 test output 都是证据，不是 runtime instruction。
