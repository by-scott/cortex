# 路线图评审

这份文档把当前的成熟度判断进一步压成一份工作路线图。它不是发布日期承诺，而是 Cortex 作为长期运行本地 Agent runtime 下一阶段的工程优先级声明。

核心规则很简单：下一轮不要再给运行时增加比它当前能稳稳承载更多的表面。Cortex 已经有足够大的系统边界；接下来的版本应优先加固那些真正构成差异化的边界：actor 归属、replay、权限控制、channel 连续性、插件契约和 operator trust。

## 当前位置

到 v1.2.0 为止，Cortex 已经不只是“有意思的研究型 runtime”。它已经形成了一套连贯的 operator surface：

- 带 replay 和 side-effect substitution 的事件溯源持久化
- 显式 turn state 和可操作中断
- actor 级 session、task、audit、memory 可见性
- 可热切换的权限模式和确认流
- process JSON 与 trusted native ABI 两条插件边界
- browser、plugin、channel 配置的热应用
- Telegram 和 QQ 在平台支持范围内优先使用卡片控制

这已经足以支撑在强信任本地机器上的 serious pilot，但还不足以把 Cortex 当成已加固的共享基础设施。

下一阶段的工作已经开始：runtime 现在已有围绕 actor/session 连续性的 deterministic 与 seeded ownership sequence tests，也已经补上 actor-scoped memory/task/audit store coverage、通过 memory id 恢复 embedding visibility 的校验、以及面向 `memory_search` / `memory_save` 的 actor-scoped memory tool tests，同时还有 transport binding 到 memory/task ownership、以及 transport rebind 后 memory/task/audit 归属语义的 runtime 校验；同时也落下了第一批针对 web、file、plugin、channel 形态 hostile input 的 structured red-team corpus；两条插件边界的 conformance coverage 也已起步，并开始通过 shared helper surface 覆盖 process plugin 边界和 trusted native ABI entrypoint；兼容性策略文档也已经落下，用来定义哪些 surface 被视为稳定、带版本或 best-effort；docs/runtime sync checks 也已经落下，用来校验中英文 README 和 operator 文档中的 event 数量、turn-state 数量、permission mode 指南、plugin boundary 与 hot-reload 表述、risk surface 指南、compatibility policy 入口，以及 attention / metacognition / memory recall 的硬表述是否仍与实际运行面一致。

## 下一阶段的原则

下一轮路线图应坚持五条原则：

1. **归属先于便利。** 跨客户端连续性只有在 actor 和 session 边界始终正确时才有价值。
2. **Replay 先于 folklore。** 重要的运行时行为，要么可检查，要么可重放，最好两者兼具。
3. **契约先于生态。** 插件和 channel 的扩展应建立在显式 conformance boundary 上，而不是临时兼容。
4. **Operator trust 先于功能数量。** status、audit、control 和文档必须跑在新增 runtime surface 前面。
5. **加固先于扩张。** 下一阶段最有价值的收益来自让当前行为在对抗输入和长期运行下更可靠。

## 1.3 范围

下一个正式版本应该是 `1.3.0`。下面所有边界加固工作都属于这一条发布线。它们是 `1.3.0` 内部的工作流，而不是三个不同的未来版本号。

### 工作流 1 —— 归属与边界加固

第一优先级是把 actor、session、memory ownership 做成整个系统最强的不变量。

#### 主要目标

- 将 actor/session 可见性做成 property-tested runtime invariant。
- 在 CLI、HTTP、Telegram、QQ 和本地 transport 间对 pairing、alias、session reuse、session switch、subscription routing 做压力测试。
- 收紧 memory、audit、task、embedding 的 ownership，使跨 actor 泄漏由测试兜底，而不是靠经验判断“应该没有”。
- 把 turn interruption 和 permission wait 测试扩展到 transport-level 场景，尤其覆盖 slash 命令和 callback 驱动路径。

#### 具体工作

- 为 canonical actor 映射、paired-user 可见性、session reuse 规则加入 property tests
- 针对 pairing state、subscription toggle、alias rewrite、per-client active-session 变化做 fuzzing
- 补齐 lazy session creation 和 per-client subscription routing 的 transport matrix tests
- 强化 session/task/audit/memory store API 的可见性断言
- 补强 `/stop`、pending confirmation、channel interaction callback 的回归测试

### 工作流 2 —— 对抗输入与插件契约

归属边界更稳后，下一层主要风险就是外部输入：web、文件、插件和 channel 最终都会进入同一个 runtime。

#### 主要目标

- 将 guardrails 从 baseline coverage 提升到可重复运行的 red-team harness。
- 为两条插件边界定义显式兼容性预期。
- 减少 process plugin 和 trusted native plugin 对 host 的隐式假设。

#### 具体工作

- 为 web/file/plugin/channel 输入上的 prompt injection、role override、exfiltration、policy-conflict 场景建立 red-team harness
- 为以 untrusted evidence 进入 LLM history 的外部工具输出补 hostile-output suites，并覆盖 `ExternalInputObserved`、`GuardrailTriggered` 以及不可信 tool-result 包装的运行时可观测性
- 为 process plugin 建 conformance kit，覆盖 manifest 校验、路径约束、timeout 行为、环境继承和输出限制
- 为 trusted native ABI 建 conformance kit，覆盖 entrypoint 行为、ABI versioning、host callback 和失败回报
- 补强通过 `[risk.tools.<name>]` 管理已审查工具策略的文档和例子

### 工作流 3 —— 长期升级与运行时信任

`1.3.0` 的最后一个工作流是“时间”这一层：升级、schema 漂移、长期 Journal 和第三方扩展在几周尺度上的表现，而不只是几小时。

#### 主要目标

- 把 replay、event schema 和公开 runtime 语义都当成兼容性 surface。
- 让 upgrade 和 migration 行为可观察、可测试。
- 在 drift 演化成损坏或混乱前，把它暴露给 operator。

#### 具体工作

- 为 event schema 兼容性和跨版本 replay projection 建回归套件
- 为 event counts、turn states、permission modes、plugin contracts 等关键 surface 引入自动化 docs/spec 生成
- 基于现有 Journal、prompts、actor mapping、plugin state 跑 upgrade/migration tests
- 为 daemon lifecycle、channel reconnect、provider failure、SQLite recovery 增加更长时间的 soak tests
- 为 trusted native ABI 和 process plugin protocol 建明确的 compatibility policy

### 1.3 退出标准

`1.3.0` 不应在这三个工作流都建立起来之前发布。

- 不存在已知路径让一个 actor 看见或切进另一个 actor 的 session
- 不存在已知路径让 subscription 镜像无关会话
- hostile input 回归有稳定的自动化套件
- 两条插件边界都有显式兼容性检查，而不是只靠 prose
- 已发布文档和 shipped runtime surface 保持同步
- upgrade 回归能用历史数据提前发现，而不只是在全新安装里发现
- replay 能持续作为可信的调试和审计工具使用

## 现在不该优先做的事

这些方向并非没价值，但不应盖过上面的优先级：

- 继续增加认知科学命名
- 在没有对应策略覆盖的情况下继续扩内置工具面
- 提前扩大到超出 trusted local Linux/systemd 的部署宣称
- 把 trusted native ABI 包装成沙箱边界
- 在契约和兼容性工具没成型前就推动大型第三方插件生态

## 下一阶段的成功标准

下一阶段应让 Cortex 更值得信任，而不只是更大。成功信号包括：

- channel 连续性是稳定的，而不是偶尔令人意外
- replay 成为日常调试表面
- operator control 在 CLI、HTTP、Telegram、QQ 上表现一致
- 插件作者拿到的是显式 contract tests，而不是靠猜
- 文档对 shipped runtime 的描述足够准确，足以支撑 serious operator use

如果 Cortex 能把这些做好，它就会从“很有前景的本地 runtime”走到“别人可以认真构建在其之上的可信本地 agent 内核”。
