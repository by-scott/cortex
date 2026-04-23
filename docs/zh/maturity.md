# 成熟度与生产说明

Cortex 更适合被理解为一个早期本地 Agent runtime：它已经有不少严肃的系统工程实现，但还不是成熟的多租户平台。它的架构野心很高，许多关键机制也已经落到代码里；不过，在把它当成强安全、强兼容的基础设施之前，还需要真实负载、对抗输入和第三方扩展的长期验证。

## 已实现内容

- 基于 SQLite WAL 的事件溯源 Journal，包括大 payload 外部化、checkpoint、replay helper 和上下文压缩边界。
- 显式 Turn 状态机，约束 processing、tool wait、permission wait、human input、compaction、consolidation、completion、interruption、suspension 等迁移。
- 分层记忆模型，包含生命周期状态、衰减、再巩固、图关系、混合召回和 consolidation 路径。
- 运行时元认知：注意力通道、置信度追踪、doom loop/fatigue/frame 检测、自适应阈值和工具效用追踪。
- 文件化的 Executive 与 Repertoire 资产：prompt layer、bootstrap/resume context、活跃 skill、工具 schema、召回记忆，以及可热重载的 skills/prompts。
- 通过 canonical actor 与 channel alias 实现多接口身份连续性。
- 原生插件加载、可热重载的进程隔离插件工具代理、插件 skills/prompts、SDK/ABI 检查和带运行时上下文的工具执行。
- 面向 channel/transport 身份的 actor 级 session 与长期记忆可见性。
- Replay 副作用替换和确定性 replay digest 对比。

## 认知科学表述的准确边界

这里的认知科学词汇是架构启发，不是形式化等价声明。例如：

- “Global workspace” 对应前台调度和 Journal 广播。
- “Drift diffusion” 对应有界固定增量的置信度累积。
- “Complementary learning systems” 对应 captured/materialized/stabilized 记忆生命周期和 consolidation 启发式。
- “Reward prediction error” 对应 EWMA 工具效用和 UCB1 风格探索。

这种框架有助于工程一致性，但不应被理解为已经验证过的认知架构。

## 当前信任边界

原生插件现在有两类边界。`trusted_in_process` 插件通过 `dlopen` 和 FFI 入口点运行在 daemon 进程内，返回的 trait object 依赖运行时持有共享库句柄保持有效。`process` 插件则通过 manifest 声明代理工具，并用 JSON stdin/stdout 协议作为子进程执行，可控制 cwd、环境变量、timeout、输出上限、宿主路径 opt-in，以及 Unix CPU/内存 rlimit。进程内插件是实用 Rust 扩展机制，不是长期稳定二进制 ABI；manifest 会声明 SDK 版本和 ABI revision，加载前会拒绝不兼容值。

工具风险门是 gate，不是 containment。内置工具有明确基础分数。未知工具，包括没有专门 profile 的插件和 MCP 工具，现在默认按保守风险评分处理，并需要确认。生产部署仍应定义显式 allowlist、deny rule 和按工具划分的策略。

可以通过 `[risk.tools.<name>]` 为单个工具声明策略，覆盖风险轴、强制确认或直接阻断。对已审查过的插件和 MCP 工具使用它：安全工具可以减少无谓确认，强能力工具可以始终保持显式确认。

外部工具输出会以 untrusted provenance 记录，并在进入 LLM history 前包裹成“不可信证据”，避免 web/file/plugin 返回内容被当作指令执行。Guardrails 提供 prompt injection、system prompt 泄露、role override 和 exfiltration 的基础检测；可疑工具输入会让会修改状态的工具强制进入确认，可疑工具输出会写入 Journal 供审计。

Replay 在副作用被记录时是确定性的。重放投影会用 provider 提供的值替换 `SideEffectRecorded` 事件中的记录值，从而覆盖已记录的 LLM 响应、墙钟时间、随机值和外部 I/O 输出。`replay_determinism_digest` 可在排除 event id 和 timestamp 后对比等价投影。会修改外部系统的工具仍需要 Journal 之外的幂等和审计设计。

## 尚未具备

- 没有针对已编译插件共享库的长期稳定二进制 ABI。
- 没有针对进程隔离插件命令的 container/seccomp 级沙箱；当前进程控制包括路径、环境变量、timeout、输出上限和 Unix rlimit。
- 没有进程内共享库插件热替换；进程隔离 manifest/tool-set 变更可热重载，但进程内库更新仍需要重启 daemon。
- 不宣称已经完成跨 OS 用户或不可信插件的敌对多租户加固。
- 没有超出 provenance 包裹、结构化 guardrails 和审计事件之外的完整对抗型 prompt injection 防线。
- 对会修改外部系统的工具没有完整 containment。

## 威胁模型说明

个人本地使用假设用户、机器账户和插件来源可信。主要风险是意外破坏性工具调用、本地密钥泄露、过期记忆和外部服务副作用。

团队或共享工作站使用会增加 channel 身份、操作员批准和插件来源风险。应使用显式 actor 映射，启用认证，并为会发布、部署、删除、花钱或访问凭据的工具配置 `[risk.tools.<name>]` 策略。

多租户现在具备 actor 级 session 可见性，以及 memory/session/task/audit store API 强制过滤。Embedding vector 通过 memory id 继承归属，不单独维护 actor 元数据。它仍不是敌对租户场景下的已加固部署目标；那还需要进程/容器隔离、每租户独立存储根、超出子进程控制的插件沙箱、更强策略执行、配额隔离，以及超出当前 baseline 的对抗输入测试。

## 生产加固 Backlog

- 为不可信进程插件增加 container/seccomp 隔离选项。
- 将 prompt injection 处理扩展到当前 provenance 包裹和 regex/literal 检测之外，尤其覆盖 web、文件和跨 channel 输入。
- 将 soak/fault harness 扩展为长期 daemon 测试，覆盖 provider、channel、database 故障。
- 分别记录个人本地使用、团队使用、多租户部署的运行威胁模型。
