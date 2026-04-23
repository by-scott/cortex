# 成熟度与生产说明

Cortex 更适合被理解为一个早期本地 Agent runtime：它已经有不少严肃的系统工程实现，但还不是成熟的多租户平台。它的架构野心很高，许多关键机制也已经落到代码里；不过，在把它当成强安全、强兼容的基础设施之前，还需要真实负载、对抗输入和第三方扩展的长期验证。

## 已实现内容

- 基于 SQLite WAL 的事件溯源 Journal，包括大 payload 外部化、checkpoint、replay helper 和上下文压缩边界。
- 显式 Turn 状态机，约束 processing、tool wait、permission wait、human input、compaction、consolidation、completion、interruption、suspension 等迁移。
- 分层记忆模型，包含生命周期状态、衰减、再巩固、图关系、混合召回和 consolidation 路径。
- 运行时元认知：注意力通道、置信度追踪、doom loop/fatigue/frame 检测、自适应阈值和工具效用追踪。
- 文件化的 Executive 与 Repertoire 资产：prompt layer、bootstrap/resume context、活跃 skill、工具 schema、召回记忆，以及可热重载的 skills/prompts。
- 通过 canonical actor 与 channel alias 实现多接口身份连续性。
- 原生插件加载、插件 skills/prompts、SDK 类型和带运行时上下文的工具执行。

## 认知科学表述的准确边界

这里的认知科学词汇是架构启发，不是形式化等价声明。例如：

- “Global workspace” 对应前台调度和 Journal 广播。
- “Drift diffusion” 对应有界固定增量的置信度累积。
- “Complementary learning systems” 对应 captured/materialized/stabilized 记忆生命周期和 consolidation 启发式。
- “Reward prediction error” 对应 EWMA 工具效用和 UCB1 风格探索。

这种框架有助于工程一致性，但不应被理解为已经验证过的认知架构。

## 当前信任边界

原生插件是可信代码。它们通过 `dlopen` 和 FFI 入口点运行在 daemon 进程内，返回的 trait object 依赖运行时持有共享库句柄保持有效。这是实用的 Rust 扩展机制，不是沙箱，也不是长期稳定二进制 ABI。只应安装可信来源的插件。

工具风险门是 gate，不是 containment。内置工具有明确基础分数。未知工具，包括没有专门 profile 的插件和 MCP 工具，现在默认按保守风险评分处理，并需要确认。生产部署仍应定义显式 allowlist、deny rule 和按工具划分的策略。

Guardrails 是基础检测。它结合字面标记和 regex 检测 prompt injection 与 system prompt 泄露，能减少常见误用和朴素攻击，但不是完整对抗安全边界。

Replay 只有在副作用被记录时才是确定性的。重放投影会用 provider 提供的值替换 `SideEffectRecorded` 事件中的记录值，从而覆盖已记录的 LLM 响应、墙钟时间、随机值和外部 I/O 输出。会修改外部系统的工具仍需要 Journal 之外的幂等和审计设计。

## 生产加固 Backlog

- 为插件和 MCP 工具增加显式策略 profile，而不是只依赖通用风险评分。
- 为不可信原生插件考虑进程或容器隔离。
- 将 prompt injection 处理扩展到关键词/regex 之外，尤其覆盖 web、文件和跨 channel 输入。
- 增加插件 SDK 版本兼容和 manifest 协商测试。
- 增加长期 daemon soak test、replay 确定性测试，以及 provider、channel、database 故障注入测试。
- 分别记录个人本地使用、团队使用、多租户部署的运行威胁模型。

