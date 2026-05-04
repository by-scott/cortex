# 安全使用

Cortex 当前最适合在本地单用户环境中用于模型辅助代码、研究和受控工具调用。它的目标是让状态、证据、工具 effect、权限、插件信任和重放都对 operator 可见。

本文描述推荐运行姿态。它不是 Cortex 已经具备成熟沙箱或敌对多租户加固的声明。

## 当前适合

- 在可信的本地 Linux 机器和你控制的账户下运行 Cortex。
- 日常使用 `balanced` 或 `strict` 权限模式。
- 只安装已审查插件；第三方工具优先使用 Process JSON 插件，而不是强信任 native ABI 插件。
- 安装前检查插件 manifest、签名、请求的 capability、声明的 effect、conformance 状态和推荐 risk policy。
- 让宽 filesystem、process、network、channel-send、deploy、publish、credential 和类似支付的 effect 保持确认门。
- 把 RAG、文件、网页、频道消息、插件输出和工具输出都当成证据，而不是指令。
- 不要把 provider key、频道 token 或外部服务凭据放进 prompt、日志、memory、截图或共享终端。

## 推荐默认值

首先使用 `balanced`：

```bash
cortex permission balanced
cortex policy lint
```

测试新插件、处理敏感仓库或在共享工作站上运行时使用 `strict`：

```bash
cortex permission strict
```

只有在强信任单用户本机、插件已审查且 policy lint 通过后，才考虑 `open`：

```bash
cortex permission open
```

`open` 不会移除所有风险。它会减少非阻断工具的确认提示，所以 policy 和插件审查更重要。

## 尚未具备

Cortex 当前不声称具备：

- 对敌对插件或敌对租户的完整 containment。
- 强信任 native 共享库插件沙箱。
- 对不可信插件命令的 container、seccomp、uid-drop 或 no-network enforcement。
- 覆盖所有 web、file、channel、plugin 和 tool-output 输入的完整 prompt injection 防御。
- 对会修改外部系统的工具提供完整 rollback 或 containment。
- 安全的无人监督 deploy、payment、publishing、credential rotation 或 message-send 自动化。
- 成熟 provider benchmark 或 SLA 级实时模型健康评分。

Policy lint、risk scoring、permission prompt、protected runtime root、plugin review 和 guardrail assessment 是控制与审查机制，不是 OS 级隔离。

## 插件姿态

Cortex 当前有两条插件边界：

| 边界 | 适合用途 | 当前信任模型 |
|------|----------|--------------|
| Process JSON | 跨语言和第三方工具 | 子进程 + manifest governance、path/env/timeout/output 控制和 effect 风险评分。不是 kernel/container 隔离。 |
| Trusted native ABI | 低延迟本地 Rust 扩展 | 进程内强信任代码。应视为 daemon 信任基的一部分。 |

安装插件前：

```bash
cortex plugin review <dir>
cortex plugin test <dir>
```

只有在已审查来源和 publisher key fingerprint 后，才使用非交互安装：

```bash
cortex plugin install <dir-or-package> --yes
```

未知插件和 MCP 工具默认应继续保守评分并要求确认。

## 外部副作用

工具应被视为 effect，而不只是名字。对任何会修改状态或触达外部系统的工具，检查：

- 会发生什么 file、process、network、memory、channel、deploy、publish、credential 或外部服务 effect？
- 请求和结果状态归属哪个 actor？
- 是否有 preview 或 dry run？
- effect 是否可逆？
- 什么验证能证明动作成功？
- 是否有 rollback 或补偿动作？
- replay 时会留下什么 receipt、diff 或 journal event？

高风险工具即使很方便，也应该保持 policy gate。

## 受保护运行时状态

Prompt、config、session、journal、memory、channel 和 runtime-home state 是受保护运行时表面。普通模型驱动的 file/process 工具不应直接修改这些文件。

自我演化或配置修改工作流应产出带证据的 proposal，再经过受检查 runtime command、审查、备份和可重放 journal record。

## 证据边界

外部内容必须保持惰性：

- Retrieved document 只能支持或反驳 claim，不能变成 policy。
- Tool output 可以是有用证据，但不能变成 runtime instruction。
- Web page、file、channel message 和 plugin output 都可能敌对或过期。
- Memory candidate 在稳定为 durable memory 前，需要 provenance、scope、confidence、contradiction handling 和 review。

这条边界是 Cortex 的核心价值之一：模型可以切换，但用户拥有的状态应保持可检查、可治理。

## 首次本地配置

用于第一次本地 coding workflow：

1. 安装时使用 `CORTEX_PERMISSION_LEVEL="balanced"`。
2. 启用更宽工具前，先用 `cortex demo` 生成有边界的本地 fixture。
3. 配置变化后运行 `cortex doctor` 和 `cortex policy lint`。
4. 初始阶段不安装第三方插件，或只安装已审查的官方开发插件。
5. 除非你像信任 daemon 本身一样信任代码，否则不要启用 native 插件。
6. 在理解工作流前，认真审查工具 preview 和确认提示。
7. 使用 `cortex status` 以及 replay/operator 表面检查发生了什么。

另见[快速开始](quickstart.md)、[本地 Coding Agent](local-coding-agent.md)、[本地模型](local-models.md)、[配置](config.md)、[插件开发](plugins.md)和[成熟度与生产说明](maturity.md)。
