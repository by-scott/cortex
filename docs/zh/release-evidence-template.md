# Release Evidence 模板

本模板用于 release candidate review 附件。它是证据清单，不是 release
note。不要把未运行或未审阅的项目标成 passed。

## 候选版本

```text
Release target:
Git revision:
Generated at:
Reviewed by:
Docker gate image:
```

## 必需附件

| Evidence | Status | Command or file | Notes |
|----------|--------|-----------------|-------|
| Strict Docker gate | not run | `./scripts/gate.sh --docker --require-clean` | release-ready claim 前必须运行。 |
| Release behavior report | not run | `./scripts/release-behavior-report.sh --run` | 覆盖 memory、retrieval/RAG、tools、safety、operator timeline、recovery、replay 和 soak 姿态。 |
| Bounded soak/fault report | not run | `./scripts/soak-fault-harness.sh --run` | 覆盖有界 provider、channel、SQLite、plugin crash、disk/config、rate-limit/backpressure、replay determinism 和 reconnect 证据。 |
| Release audit table | not checked | `docs/release-audit-<version>.md` | 每个 partial 行都需要明确 limitation 或 acceptance evidence。 |
| First-run readiness output | not run | `cortex doctor --json` | 只读诊断；不是沙箱，也不是 provider 可达性证明。 |
| Policy posture | not run | `cortex policy lint` | 记录 warning/error 和 remediation decision。 |
| Plugin conformance attachment | not run | `docs/zh/plugin-conformance-template.md` | 对 candidate 依赖的每个插件填写一份。 |
| Prompt-injection corpus review | not run | `scenarios/prompt-injection/corpus.json` | Review hostile evidence case，并附上 candidate-specific 新增项或 limitation。 |

Status values:

```text
pass
fail
not run
not applicable
blocked
```

## 安全边界复核

确认 release claim 仍保持这些边界：

- Policy/risk gates 只描述为 review/control 机制，不描述成 sandbox containment。
- Native plugins 描述为 trusted in-process code，不描述成 sandboxed code。
- Retrieved evidence、文件、网页、频道消息、plugin output 和 tool output 保持为 evidence，不成为 runtime instruction。
- 普通工具不能直接修改 prompts、config、sessions、journal、memory、channel state 或 protected runtime roots。
- Demo 和 first-run 路径不会放宽权限、启用插件，或在没有 operator action 的情况下启动外部副作用。
- 如果候选版本没有运行 24h/72h/7d soak evidence，必须把它列为 limitation。
- Prompt-injection corpus review 是证据覆盖，不是完整防御声明。

## 行为证据摘要

| Area | Evidence source | Result | Limitation |
|------|-----------------|--------|------------|
| Memory ownership and memory tools |  |  |  |
| Retrieval/RAG and support verification |  |  |  |
| Tool risk and permission behavior |  |  |  |
| Prompt injection and hostile evidence corpus |  |  |  |
| Actor/session isolation |  |  |  |
| Replay and journal recovery |  |  |  |
| Plugin governance and conformance |  |  |  |
| Operator status/timeline observability |  |  |  |
| Bounded soak/fault harness |  |  |  |
| Long soak evidence |  |  |  |

## 已知限制

列出必须保持公开、不能升级成 release claim 的限制：

```text
- 
```

## 决策

```text
Release decision: pass / fail / hold
Reason:
Follow-up issues:
```
