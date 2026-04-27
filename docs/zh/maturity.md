# 成熟度与生产说明

Cortex 更适合被理解为一个早期本地语言模型 harness：它已经有不少严肃的系统工程实现，但它不是自主执行任务的产品，也还不是成熟的多租户平台。它的架构野心很高，许多关键机制也已经落到代码里；不过，在把它当成强安全、强兼容的基础设施之前，还需要真实负载、对抗输入和第三方扩展的长期验证。

## 已实现内容

- 基于 SQLite WAL 的事件溯源 Journal，包括大 payload 外部化、checkpoint、replay helper 和上下文压缩边界。
- 显式 Turn 状态机，约束 processing、tool wait、permission wait、human input、compaction、consolidation、completion、interruption、suspension 等迁移。
- 分层记忆模型，包含生命周期状态、证据化 claim、冲突链接、有效期窗口、使用结果、衰减、再巩固、图关系、混合召回和 consolidation 路径。
- 运行时元认知：注意力通道、置信度追踪、doom loop/fatigue/frame 检测、基于 outcome 校准的自适应阈值和工具效用追踪。
- 文件化的 Executive 与 Repertoire 资产：prompt layer、bootstrap/resume context、活跃 skill、retrieved evidence、工具 schema、召回记忆，以及可热重载的 skills/prompts。
- Workspace admission 包含 typed lane、utility/risk/volatility 评分、taint barrier、预算感知 marginal utility 和 eviction record。
- Skill manifest 与有界 execution trace，覆盖 effect、risk、success criteria、fallback、observability、trigger、duration 和 status。
- 模型能力路由已使用 group profile 覆盖 coding、long context、vision、tool calling、JSON reliability、latency、cost、safety 和 reasoning depth，并能解释 fallback 以及 risk/confidence escalation。
- 通过 canonical actor 与 channel alias 实现多接口身份连续性。
- 进程隔离插件代理、强信任 native ABI 加载、插件 skills/prompts 和带运行时上下文的工具执行。
- 面向 channel/transport 身份的 actor 级 session 与长期记忆可见性。
- 面向外部内容的结构化 guardrail assessment，包括 taint disposition、safe transformation、Journal guardrail event 和 hostile-source memory candidate。
- RAG evidence role、确定性回答 claim support report，以及针对被反驳或过期 retrieved fact 的 negative evidence 处理。
- 面向核心工具和插件的声明式 tool effects，包含 effect-based risk floor，以及工具执行前后的 preview、verification、commit event。
- Replay 副作用替换、projection version、replay diff、causal audit graph edge、migration fixture 和确定性 replay digest 对比。
- 通过 `cortex policy lint`、`cortex policy simulate` 以及 daemon 启动 finding 提供 policy-as-code lint 和 simulation。

## 认知科学表述的准确边界

这里的认知科学词汇是架构启发，不是形式化等价声明。例如：

- “Global workspace” 对应前台调度和 Journal 广播。
- “Drift diffusion” 对应有界固定增量的置信度累积。
- “Complementary learning systems” 对应 captured/materialized/stabilized 记忆生命周期、证据质量、冲突处理和 consolidation 启发式。
- “Reward prediction error” 对应 EWMA 工具效用和 UCB1 风格探索。

这种框架有助于工程一致性，但不应被理解为已经验证过的认知架构。

## 当前信任边界

Cortex 现在有两条插件边界。进程 JSON 是默认外部边界：插件通过 manifest 声明代理工具，并用 JSON stdin/stdout 协议作为子进程执行，可控制 cwd、环境变量、timeout、输出上限、宿主路径 opt-in，以及 Unix CPU/内存 rlimit。强信任 native ABI 插件则通过 `cortex_plugin_init` 作为共享库加载到进程内；它是强信任扩展边界，不是沙箱。

Plugin package 现在携带治理 contract：trust tier、请求的 file/network/process/secret/background capability、sandbox profile、package metadata、signed-package 字段、SBOM/risk-profile 引用、conformance certificate 和 tool effects。Runtime 会在加载前校验不可能或不安全的组合，会拒绝当前无法真正提供的 sandbox enforcement 声明，process tool 会把 manifest 声明的 effects 暴露给风险评分，`cortex plugin review` 会展示安装面，`cortex plugin test` 会运行本地 conformance kit。这些控制提高 operator 可见性，并拒绝明显不安全组合；它们仍不等价于 kernel/container 隔离。

工具风险门是 gate，不是 containment。内置工具有明确基础分数，并会声明 file read/write、process execution、network request、memory persistence、channel send、scheduling、media generation、delegation 等 effect surface。未知工具，包括没有专门 profile 的插件和 MCP 工具，现在默认按保守风险评分处理，并需要确认。生产部署仍应定义显式 allowlist、deny rule 和按工具划分的策略。

可以通过 `[risk.tools.<name>]` 为单个工具声明策略，覆盖风险轴、强制确认或直接阻断。对已审查过的插件和 MCP 工具使用它：安全工具可以减少无谓确认，强能力工具可以始终保持显式确认。

外部工具输出会带 provenance 记录，并在进入 LLM history 前先经过 assessment。良性外部内容会作为引用证据进入 history；敌对内容会降级为 summary-only 或 metadata-only evidence，原始敌对文本不会重新放回 history，来源也会写入 Journal 供审计。Guardrails 提供 prompt injection、system prompt 泄露、role override 和 exfiltration 的基础检测；可疑工具输入会让会修改状态的工具强制进入确认，可疑工具输出会写入 Journal 供审计并作为 guardrail event，post-turn 处理可以为后续 turn 生成 hostile-source memory candidate。

Replay 在副作用被记录时是确定性的。重放投影会用 provider 提供的值替换 `SideEffectRecorded` 事件中的记录值，从而覆盖已记录的 LLM 响应、墙钟时间、随机值和外部 I/O 输出。工具执行也会围绕声明式 effects 记录 preview、verification 和 commit event。`replay_determinism_digest` 可在排除 event id 和 timestamp 后对比等价投影。会修改外部系统的工具仍需要 Journal 之外更深的幂等和 rollback 设计。

Policy-as-code 是预检 gate，不是沙箱。`cortex policy lint` 会报告配置和已启用插件 manifest 中的危险组合，`cortex policy simulate` 会在工具运行前解释单个 tool/effect 决策。daemon 启动时也会记录同一套 finding，让高风险姿态在第一次工具调用前可见。这些检查提升 operator review 质量，但不替代 OS 隔离、凭据 broker 或运行时授权。

模型路由是确定性的 capability decision surface，不是实时 provider benchmark。Profile 可以在 `[llm_groups.*]` 中显式声明，也可以从 group name、provider protocol、model name 和 score hint 保守推断。Resolver 能解释所选 group、fallback reason、escalation，以及 cost/latency/safety tradeoff；但它仍依赖准确的 operator provider/model metadata 和后续 provider-health 观测。

Operator dashboard 是结构化 runtime inspection surface，不是通用 observability stack。它暴露 daemon state、token/tool metrics、活跃与持久化会话、共享 actor binding、待处理 backlog、模型画像，以及按 lifecycle、message、LLM、tool、permission、workspace、retrieval、memory、control、guardrail、scheduler 和其它类别归一化后的有界 Journal timeline。它用于提升 operator triage 质量，但不替代 tracing、长期 metrics 存储或 audit review。

## 尚未具备

- 没有强信任 native 共享库插件沙箱。
- Sandbox profile 已经可声明并校验，但还没有针对进程隔离插件命令的 container/seccomp 级执行隔离。当前进程控制包括路径、环境变量、timeout、输出上限和 Unix rlimit；manifest 如果声明 `uid_no_network`、`system_sandbox`、`container_vm`、`remote_worker`、`sandbox.network = "none"`、`sandbox.uid_drop = true` 或非空 `sandbox.seccomp`，会在真正实现 enforcement 前被拒绝。
- 强信任 native 共享库代码变更仍需要重启 daemon 才会生效。
- 不宣称已经完成跨 OS 用户或不可信插件的敌对多租户加固。
- 没有超出 provenance 包裹、typed taint disposition、safe transformation、结构化 guardrails、hostile-source memory candidate 和审计事件之外的完整对抗型 prompt injection 防线。
- 对会修改外部系统的工具没有完整 containment。
- 还没有自动 provider benchmark 或 SLA 级实时模型健康评分；模型路由当前使用声明/推断 profile 和显式失败信号。

## 威胁模型说明

个人本地使用假设用户、机器账户和插件来源可信。主要风险是意外破坏性工具调用、本地密钥泄露、过期记忆和外部服务副作用。

团队或共享工作站使用会增加 channel 身份、操作员批准和插件来源风险。应使用显式 actor 映射，启用认证，并为会发布、部署、删除、花钱或访问凭据的工具配置 `[risk.tools.<name>]` 策略。

多租户现在具备 actor 级 session 可见性，以及 memory/session/task/audit store API 强制过滤。Embedding 向量通过 memory id 继承归属，而不是单独携带 actor 元数据。它仍不是敌对租户场景下的已加固部署目标；那还需要进程/容器隔离、每租户独立存储根、超出子进程控制的插件沙箱、更强策略执行、配额隔离，以及超出当前 baseline 的对抗输入测试。

## 生产加固 Backlog

- 为不可信进程插件增加 container/seccomp 隔离选项。
- 将 prompt injection 处理扩展到当前 provenance 包裹、typed taint disposition、safe transformation 和 regex/literal 检测之外，尤其覆盖 web、文件和跨 channel 输入。
- 将当前 soak/fault harness 继续扩展为持续运行的 daemon 测试，覆盖 provider、channel、database 故障。
- 将 provider failure、invalid schema、latency/cost 观测回灌到 model capability registry，用于长期校准。
- 分别记录个人本地使用、团队使用、多租户部署的运行威胁模型。

当前契约边界见[兼容性策略](compatibility.md)，分阶段的后续优先级见[路线图评审](roadmap.md)。
