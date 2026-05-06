# 路线图评审

这份文档定义当前 Cortex 发布线。它不是日期承诺，而是 `v1.6.4` 的工程规划契约。

`v1.6.4` 的规则很明确：保持 harness 的强度，关闭既有清理线，并把当前 runtime contract 在插件最低版本兼容、首次使用证据、release-review corpora 和模型特定 token limit 推断上打磨得更清晰。Cortex 现在被定位为语言模型 harness：用于驱动、观察、回放、评估和加固模型行为的受控表面。本版本应把现有认知机制升级为更强的 harness contract：证据化、类型化、可校准、可回放、可审计、可评估。

## 发布目标

当前规划目标是 `1.6.4`。它是当前发布目标，不是并行 roadmap。这个版本升级的是机制，不是概念命名。一个工作项只有在强化下列性质时才进入范围：

- **证据**：运行时 claim 能指向支持证据、反证、来源和使用结果。
- **类型**：归属、effect、证据、策略和权限边界用结构表达，而不是靠 prose。
- **校准**：置信度、检索支持度、skill 效用、模型路由都要和实际结果对齐。
- **回放**：重要行为能从 journal 重建、diff 和解释。
- **评测**：发布质量包含行为、安全、检索、记忆、工具和 soak 指标，而不只是单元测试。
- **Token 经济**：provider-facing context 保持稳定前缀稳定，status 把 context usage、cache read/write 和累计 spend 分开展示。

## Harness 契约

Cortex 使用 `harness` 时，应采用测试、评测和运行时控制系统里的严肃工程含义：
harness 围绕被测系统提供受控输入、adapter、instrumentation、oracle、evaluation、
replay 和 reporting。它不是行为主体；它是让模型行为可操作、可测量、可回放、
可加固的机制。

后续产品面应围绕这些对象发展：

- **Scenario**：被执行的任务、actor、policy、数据、工具、channel 和 success criteria。
- **Fixture**：运行 scenario 所需的稳定状态，包括 journal、memory、retrieval corpus、plugin manifest、policy profile、channel binding。
- **Driver**：向 runtime 注入 turn、tool result、channel event、fault 和 operator decision 的组件。
- **Adapter**：provider、tool、plugin、transport、corpus 和外部系统的边界层。
- **Oracle**：针对 correctness、safety、ownership、citation、side effect、permission 和 recovery 的显式期望。
- **Evaluator**：对 output、trace、tool choice、memory change、retrieval support 和 safety behavior 打分的指标代码。
- **Trace**：实际运行的类型化、可查询记录，而不只是日志。
- **Replay**：基于 journaled input、fixture、external receipt 和 projection version 的确定性或差异化重建。
- **Report**：解释 pass/fail、regression、risk，以及每个结论证据来源的发布或 scenario 结果。

这是后续工作的方向。新增功能必须能说明它强化了哪个 harness 对象。只让 Cortex
显得更自主、但不提升控制、测量、回放或加固能力的功能，不进入 `v1.6.4` 发布宣称。

## 研究依据

这份规划不是松散功能愿望清单，而是从项目研究语料和其背后的原始文献中蒸馏出的发布契约。研究笔记本身不放进公开仓库；公开文档只保留工程义务，不把内部研究笔记变成项目文档。

| 来源族 | 原始文献与工程参考 | 对规划的影响 |
|--------|--------------------|--------------|
| Global workspace 与认知循环 | Baars 的 Global Workspace Theory、Franklin 的 LIDA、Dehaene/Naccache 的 global neuronal workspace、CoALA 的 language-agent architecture 框架 | 前台注意力、有限 workspace admission、通过 journal 广播、显式 turn 阶段、internal action 与 external side effect 分离。 |
| Working memory 与 cognitive load | Baddeley working memory、Cowan attention focus、Miller chunking、Sweller cognitive load theory | typed workspace lane、有限注意焦点、chunked context、边际效用 admission、pressure-aware compaction、eviction explanation。 |
| Memory consolidation 与 reconsolidation | McClelland/McNaughton/O'Reilly 的 complementary learning systems、Kumaran learning-systems review、Nader reconsolidation、sleep/memory consolidation 文献 | Captured -> Materialized -> Stabilized 记忆、evidence-backed belief、source trust、contradiction、validity window、usage outcome。 |
| Metacognition 与 conflict monitoring | Flavell metacognition、Nelson/Narens monitoring-control、Botvinick conflict monitoring、Shenhav expected value of control、frame anchoring 与 calibration 研究 | typed alert、alert-to-intervention、confidence/outcome calibration、goal/instruction conflict、能改变控制流的 metacognition。 |
| Decision under uncertainty | Ratcliff diffusion decision model、Gold/Shadlen decision neuroscience、Bogacz speed-accuracy tradeoff、Fleming confidence research、precision-weighting 批判性采纳 | evidence accumulation、risk-sensitive threshold、confidence trace、可逆/不可逆 action policy、低置信或高风险时升级。 |
| Event sourcing 与 durable execution | Fowler event sourcing、CQRS/event-sourced architecture、Temporal-style durable execution、Durable Functions/Step Functions 模式 | append-only journal、command/event 分离、intent-before-execution、side-effect recording、projection versioning、replay diff、idempotency key、recorded fact recovery。 |
| SQLite 与 daemon 运维 | SQLite WAL 文档、online backup/checkpoint guidance、single-writer discipline、daemon operations practice | WAL posture、DbWriter single-writer、checkpoint observability、online backup、corruption/fault test、deterministic recovery。 |
| Security、policy 与 plugin governance | prompt-injection/tool-use security 研究、process isolation、capability manifest、signed package、SBOM/conformance、approval-system design | taint propagation、hostile-source tracking、effect policy、sandbox level、side-effect broker、plugin signature、conformance kit、deny-by-default ownership。 |
| Skills 与 capability systems | function calling schema、MCP capability negotiation、Kubernetes/VSCode/Emacs extension discovery、ACT-R/Fitts-Posner skill learning、现代 coding assistant skill 模式 | skill manifest、progressive discovery、trigger provenance、execution trace、activation 前 quarantine、utility scoring、schema-as-contract。 |
| 先验运营失败 | 前代 Cortex postmortem、continuity failure analysis、long-running session failure observations | 不把自然语言 IPC 当权威、不把 session 当 truth、journal-derived resume packet、显式 phase/frontier、frame check、rollback lifecycle event、soak/fault harness。 |
| 认知与智慧形成 | Friston 的 predictive-processing/free-energy 框架、Damasio 式 value/affect 约束、Baltes/Staudinger wisdom research、Sternberg 的 balance theory of wisdom、Grossmann 式 wise reasoning 研究 | Cortex 不宣称具备生物学智慧。harness 应提供更好判断所需的工程条件：grounded observation、value/policy weighting、long-horizon outcome feedback、calibrated uncertainty、metacognitive humility、operator correction 和 memory consolidation。 |

任何 `v1.6.4` 设计或实现如果偏离这些依据，必须写明原因、风险，以及证明该偏离对 Cortex 更安全的测试。

## 认知边界

项目研究把认知视为相互作用的闭环，而不是单一模块：感知预测世界，注意力选择有限
workspace，工作记忆维持任务状态，记忆巩固把情景经验转为稳定结构，价值系统对行动排序，
元认知在不确定、冲突或失败出现时调节控制。智慧是在这些机制之上，叠加长期后果、
价值判断、社会反馈、自我克制和纠错能力后形成的高阶判断。

对 `v1.6.4` 来说，这是边界条件，不是营销表述。Cortex 不应说自己实现了生物学认知或
智慧；它应实现让“类似智慧的可靠行为”可审计的 runtime contract：证据化 belief、
policy/value 约束、闭环反馈、校准后的 confidence、operator correction、可回放决策、
以及可修订的长期记忆。

## 评审覆盖契约

定义 `v1.6.4` 的评审意见包含二十五个必做领域。下面的范围矩阵就是它们的权威覆盖面：

1. Memory。
2. Retrieval / RAG。
3. Workspace / Context。
4. Control / Decision。
5. Metacognition。
6. Attention / Scheduler。
7. Risk / Permission。
8. Guardrails。
9. Plugin System。
10. Sandbox / Containment。
11. Replay / Journal。
12. Actor / Ownership。
13. Prompt / Executive。
14. Skills / Repertoire。
15. Tool Execution。
16. Model / Provider Routing。
17. Evaluation。
18. Observability。
19. Configuration / Policy。
20. Operations / Soak。
21. Multimodal / Media。
22. Delegation / Multi-worker，覆盖评审中的 multi-agent 要求，但用新的 worker/harness 产品词汇表达。
23. Security / Secrets。
24. Data Model / Schema。
25. Human Feedback。

任何一行都不能静默删除、不能把意图重命名掉，也不能当营销文案。发布评审时，每一行都必须有实现证据、测试、文档和已知限制说明。

## 硬门禁

`v1.6.4` 必须继续遵守严格门禁：

- `cargo fmt --all --check` 无 diff。
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery` 零警告。
- `cargo test --workspace --all-features` 全部通过。
- 必须规范使用 Docker Compose：通过仓库入口 `./scripts/gate.sh --docker` 运行本仓库 `docker-compose.yml` 的 `dev` 服务，该服务由仓库 `Dockerfile` 构建；这个仓库 Docker Compose 环境是唯一具备发布权威的 gate。
- 不引入任何警告抑制属性或编译器警告抑制 flag。
- 任一检查失败都阻断发布，必须修改实际代码解决。

## 范围矩阵

这张表是 `v1.6.4` 的追踪面。每一行都是必须被规划、实现、文档和验收覆盖的方向，不能静默遗漏。

| 领域 | 升级方向 | 必做工作 | 验收信号 |
|------|----------|----------|----------|
| Memory | 从“记忆条目”升级为“证据化信念系统” | 增加 claim、evidence、scope、confidence、contradiction、supersession、validity window、risk-if-wrong、用户确认和 usage outcomes。稳定化必须考虑证据质量、用户确认、跨任务一致性、冲突、使用结果和错误风险。 | 稳定记忆能说明为什么相信、哪些事件支持、适用范围、冲突对象、最近是否被反驳、使用后是否改善任务。 |
| Retrieval / RAG | 从“相关文本召回”升级为“证据裁判系统” | 增加 evidence role、answer-claim support verifier、negative evidence、corpus trust policy，并把 HyDE 等 query artifact 永远不能成为 evidence 做成硬不变量。 | 回答能生成 support report，列出 supported、contradicted、unsupported claims。 |
| Workspace / Context | 从“上下文拼装”升级为“受控工作区” | 增加 typed role、trust、utility、risk、volatility、binding、eviction reason、边际效用 admission、evidence/memory/policy lane 和 contamination barrier。 | frame 能解释每个 item 为什么进入或被淘汰；外部文本和工具输出不能变成 instruction、identity、permission 或 tool policy。 |
| Control / Decision | 从“启发式动作选择”升级为“可解释控制策略” | 记录 candidate actions、benefit、cost、risk、confidence、reversibility、selected action、rejected alternatives、blocking uncertainty、required evidence 和 fallback plan。采用风险敏感阈值与 observe-retrieve-evaluate-conflict-decide-verify-commit 循环。 | 请求确认时能说明可选动作、证据支持度、风险边界，以及为什么不能自动继续。 |
| Metacognition | 从“报警器”升级为“自我校准控制面” | 增加 GoalConflict、EvidenceInsufficient、EvidenceConflict、ToolLoop、LowProgress、HighUncertainty、InstructionConflict、ContextOverload、CalibrationDrift、UserDissatisfaction 等 typed alerts。alert 必须映射 intervention 并记录 outcome。 | 每个 alert 都有 trigger、severity、recommended_action、action_taken、outcome、threshold_update，且能影响控制流。 |
| Attention / Scheduler | 从“三通道调度”升级为“资源治理” | 增加 maintenance debt、emergency debounce、actor fairness、per-actor budget、deadline、cost、risk、priority inheritance、operator override。 | scheduler 能解释 maintenance 为什么推迟、emergency 为什么抢占、哪个 actor 消耗预算最多、哪些后台任务因风险或预算暂停。 |
| Risk / Permission | 从“工具风险打分”升级为“效果类型系统” | 定义 ReadFile、WriteFile、DeleteFile、RunProcess、NetworkRequest、SendMessage、SpendMoney、Deploy、ModifyCredential、PersistMemory、PublishContent 等 effect。工具声明 effects、reversibility、confirmation condition、dry-run support、paths、domains、actors。policy 能按 effect 配置，不只按工具名。 | operator 看到的是实际 effect、影响路径/域名、可逆性、dry-run、风险原因、批准 actor 和 rollback 路径，而不是笼统的“bash 风险高”。 |
| Guardrails | 从“规则检测”升级为“对抗输入治理” | 增加 taint propagation、结构化 injection intent、cross-turn hostile-source memory，以及 summary-only、quote-only、metadata-only 等 safe transformation。 | 恶意 web/file/plugin/channel 内容会被降级为 hostile evidence，不能影响 policy、identity、permission、memory，并写入 guardrail journal event。 |
| Plugin System | 从“插件能运行”升级为“插件可治理生态” | 增加 capability manifest、signed package、publisher identity、manifest/binary hash、SBOM、declared capabilities、risk profile、conformance certificate，以及 trusted_native、reviewed_process、unreviewed_process、disabled、quarantined trust tiers。 | 安装前 operator 能看到插件请求的能力、是否请求 secrets、签名状态、conformance 结果和推荐 risk profile。 |
| Sandbox / Containment | 从“进程边界”升级为“真实隔离” | 定义 L0 in-process trusted native、L1 child process、L2 uid/gid drop + no network、L3 bubblewrap/firejail/seccomp、L4 container/VM、L5 remote isolated worker。高危外部动作走 side-effect broker。 | 未信任插件不能读取 `~/.ssh`、访问任意网络、绕过 output limit、修改 host config、偷 provider key、长期驻留后台进程。 |
| Replay / Journal | 从“可回放事件流”升级为“因果审计系统” | 增加 causal edge、depends_on、invalidates、projection versioning、replay diff、外部 idempotency key、dry-run hash、pre/post-state hash、rollback action、external receipt。 | 能从用户请求一路追到 retrieval evidence、control decision、permission prompt、tool call、file diff、test result、memory update 和每个 side effect。 |
| Actor / Ownership | 从“actor-scoped API”升级为“信息流类型系统” | 引入 scope object，禁止 load-before-auth，私有数据必须先 authorize key / scoped query，再 materialize。私有数据进入 workspace、transport、tool input 时记录 flow-sensitive audit。 | 没有 API 能拿裸 memory/session id 绕过 actor；replay/projection 不能在未授权时 materialize 私有内容。 |
| Prompt / Executive | 从“提示词文件”升级为“可编译操作协议” | 增加 prompt parse、section check、forbidden-claim check、runtime/schema consistency check、layer compilation、version hash、prompt linter、prompt diff approval、证据支持的自修改和 rollback copy。runtime schema 永远优先。 | prompt 不能授予能力、覆盖 runtime policy、把临时状态写进 identity，或声明不存在的工具。 |
| Skills / Repertoire | 从“技能脚本”升级为“可评估程序库” | skill manifest 必须包含 preconditions、inputs、outputs、effects、risk、expected_duration、success_criteria、fallback、observability。记录 skill execution trace；新 skill candidate 先 quarantine，再 trial/review/active/deprecated。 | skill 是有权限、trace、测试、用户反馈和历史效用的 runtime unit，而不是漂亮的 SKILL.md。 |
| Tool Execution | 从“调用工具”升级为“事务型行动” | mutating tool 走 plan、preview、permission、execute、verify、commit、rollback、record。增加 dry-run first、rollback handle、结构化 tool result、artifacts、diff、receipts、warnings、verification outputs。 | 任意高风险 action 后，Cortex 能回答做了什么、改了哪里、如何验证、如何回滚、哪个 actor 批准。 |
| Model / Provider Routing | 从“配置模型”升级为“能力路由” | 建 model capability registry，覆盖 coding、long_context、vision、tool_calling、json_reliability、latency、cost、safety、reasoning_depth。低置信 + 高风险、provider failure、schema invalid 时支持 fallback/escalation。 | Cortex 能解释为什么用了模型 A 而不是 B，以及接受了什么成本/风险 tradeoff。 |
| Evaluation | 从“测试能过”升级为“长期行为评测” | 增加 memory precision/recall、false stabilization、contradiction resolution、harmful memory usage、retrieval recall/MRR/citation accuracy/unsupported claims/poison resistance、tool success/retry/bad selection/permission/rollback、long-task recovery、safety bypass/leakage 等指标。 | release report 不只说 cargo test 通过，还要给行为指标、安全语料结果和 soak 结果。 |
| Observability | 从“日志和状态”升级为“认知/行动仪表盘” | 增加 turn timeline、workspace frame、retrieval/memory/control/tool/guardrail 视图、risk ledger、memory change review、token/cost、actor/session map、plugin health、provider health。 | operator 不读原始日志也能知道为什么这样回答、为什么调用工具、为什么记忆变化、为什么请求确认。 |
| Configuration / Policy | 从“配置项”升级为“policy-as-code” | 增加 policy profiles、schema validation、static policy lint、policy simulation，以及对 tool、actor、effect 的 explain。启动时发现危险组合。 | open permission + unknown plugin、native plugin 无 risk profile、network evidence 自动写 memory、deploy tool 允许后台执行等配置会在启动/安装时暴露。 |
| Operations / Soak | 从“能安装运行”升级为“长期可靠性” | 增加 provider timeout、provider schema invalid、SQLite lock、WAL corruption、network reconnect、Telegram retry、QQ callback duplicate、plugin crash、native plugin panic、large payload externalization、journal replay after upgrade、disk full、rate limit 等故障注入；跑 24h/72h/7d daemon soak。 | 故障后不丢 actor/session ownership，pending permissions 不错乱，journal replay 和 state recovery 一致，channel reconnect 不串 session。 |
| Multimodal / Media | 从“媒体工具”升级为“多模态证据治理” | 增加 media id、hash、mime、source_actor、source_uri、visibility、extracted_text、detected_objects、generated/edited 标记、license、taint、media provenance、media-derived evidence 和外部接收者安全策略。 | OCR/vision caption 只是派生 evidence，有 confidence 和来源；不能静默写长期记忆或外发。 |
| Delegation / Multi-worker | 从“worker 调用”升级为“受控委派” | 增加 delegation contract，包含 task、scope、allowed_tools、forbidden_actions、time/token budget、evidence_allowed、expected_artifact、review_required、merge verifier、最小权限继承。 | Cortex 能说明委派了什么、worker 能看哪些 evidence、能用哪些工具、输出是否验证、是否触发 memory 或外部动作。 |
| Security / Secrets | 从“敏感路径规则”升级为“秘密数据流防护” | 增加 ingress secret scanner、secret source/sink tracking、allowed use、sink policy、redaction handle，以及工具需要 secret 时由 runtime broker 注入。 | 模型可以知道“存在一个 GitHub token”，但看不到值；secret 不能流向 provider、web request、plugin output、channel message、memory、logs，除非显式允许。 |
| Data Model / Schema | 从“字段集合”升级为“显式语义” | 每个稳定结构有 schema_version、semantic_version、rejection behavior、generated runtime spec，并维护当前 release fixture corpus：journal、memory、plugin manifest、actor mapping、retrieval evidence 和 daemon state。 | 当前 fixture 证明被接受的数据可以回放，非法数据会显式拒绝。 |
| Human Feedback | 从“用户反馈”升级为“训练信号系统” | 把反馈拆成 correction、preference、approval、rejection、style feedback、factual correction、safety boundary、task success、task failure。反馈要归因到 answer style、fact、tool choice、memory、evidence、permission judgment；durable feedback 进入 memory/policy candidate；支持 feedback replay。 | 用户纠正后，系统能说明纠正了哪条 memory、哪个 prompt/skill/policy 受影响，后续同类任务能证明已应用。 |

## 优先级

### P0：发布阻断项

- Memory evidence / contradiction / usage outcome tracking。
- Guardrail taint propagation + web/file/plugin/channel adversarial harness。
- Tool effect system + transactional side-effect execution。
- Plugin capability governance、sandbox profiles、signed package、conformance。
- Replay causal graph + 当前 fixture corpus。
- Policy lint + simulation。

当前 release line 的实现检查点：

这些检查点在实现、测试、文档和已知限制被一起复核前，不作为正式发布宣称。
`v1.6.4` 必须用代码级证据逐项验证，不能把之前 `1.5.x` 的描述直接当成已验收事实。

- Memory entry 已携带 evidence、claim/scope 字段、contradiction/supersession 链接、validity window、user confirmation、risk-if-wrong 和 usage outcome。
- Guardrails 已在 web、file、plugin、channel、tool-shaped 输入之间传播 taint，并提供 safe transformation 和 hostile-source memory 处理。
- 工具已声明 typed effect surface，mutating execution 会记录 preview、verification 和 commit 事件，用于事务审计。
- Plugin manifest 已携带 trust tier、sandbox profile、package metadata、conformance state 和由 capability 推导的 effects；install/review/test 路径会暴露这些治理字段。
- Replay 已暴露 projection version、causal audit graph edge、replay diff、确定性 side-effect substitution，以及当前 replay fixture corpus。
- Policy-as-code 已提供 `cortex policy lint`、`cortex policy simulate`，daemon 启动时也会记录危险 config/plugin/tool 组合。
- RAG evidence 已携带显式 role，回答 claim 可以校验为 supported、contradicted、unsupported 或 insufficient support report；negative evidence 优先于过期 support。
- Workspace frame 已暴露 lane、utility、risk、volatility、taint、预算感知 marginal utility、admission outcome、contamination barrier 和 eviction record。
- Metacognitive adaptive threshold 已记录 rich alert feedback：outcome、intervention、confidence delta、intervention success rate、precision 和 threshold snapshot。
- Skill 已暴露 manifest，包含 precondition、input、output、effect、required tool、risk、expected duration、success criteria、fallback 和 observability；执行会记录有界 trace。
- Model routing 已使用从 `[llm_groups.*]` 与 provider metadata 派生出的 capability registry，覆盖 coding、long context、vision、tool calling、JSON reliability、latency、cost、safety 和 reasoning depth。Route decision 会解释所选 group、fallback reason、被拒绝的 failed target、schema-invalid fallback，以及低 confidence/高 risk escalation。
- Operator dashboard 已暴露本地 operator state、metrics、session/binding summary、backlog、provider 模型画像，以及按 runtime category 归一化后的有界 Journal timeline。
- 首次使用证据现在包含 `cortex doctor`、`cortex doctor --json`、`cortex demo`、本地代码文档、本地模型文档、安全使用文档、policy profile 片段和维护者流程文档；这些内容不会把 policy/risk gate 说成 sandbox containment。
- Release-review corpora 现在覆盖 prompt injection、actor leakage 和 replay migration，并配合 plugin conformance evidence；这些是 Eval/Scenario 评审面，不是完整防御或敌对多租户证明。
- Model token limit 现在按显式配置、provider metadata/cache、保守 provider/model-family 推断的顺序解析，不再使用全局 200k input / 300k output fallback。
- Plugin manifest 中具体 `cortex_version` 现在表示最低支持的 Cortex runtime 版本；非法版本、range 和未来版本要求仍会被拒绝。

### P1：智能质量和可解释性

P1 工作仍属于 `v1.6.4` 范围，但必须由代码级验收测试支撑，并且不能削弱发布 gate。
已有宣称需要重新验证，尤其是 RAG support verification、workspace admission、
metacognitive calibration、skill trace、model routing 和 operator observability。

### P2：扩张项，不作为 1.5 发布宣称

这些方向必须保留在追踪面里，但不能压过 P0/P1，也不能在边界未硬化前作为成熟能力宣传：

- 复杂 multi-worker orchestration protocol，超过受控 delegation contract 的部分。
- 高级认知理论形式化，超过已实现 runtime contract 的部分。
- 大规模第三方插件生态，在 conformance 和 sandbox 成熟前不推进。
- 成熟 hostile multi-tenant platform 宣称。
- 完全自动自我演化，在 review、verification、rollback 成熟前不启用。

## 执行顺序

`v1.6.4` 应按下面顺序推进。这个顺序来自研究依据：认知依赖 grounded observation、
有限 workspace admission、memory consolidation、value-weighted action、feedback
和 metacognitive control。换成工程语言，harness 必须先知道自己相信什么以及为什么相信，
再约束自己能做什么，最后证明实际发生了什么。

1. **发布审计与事实表**：为二十五个领域建立逐行 review table。每一行记录当前代码证据、缺失的 runtime contract、测试、文档和已知限制。没有测试的宣称按未证明处理。
2. **证据与认知核心**：把 Memory、Retrieval / RAG、Workspace / Context、Control / Decision、Metacognition、Human Feedback、Model / Provider Routing 作为一个闭环完成。这对应 perception、working memory、consolidation、confidence 和 correction 的 harness 化实现。
3. **行动与隔离核心**：完成 Risk / Permission、Tool Execution、Guardrails、Security / Secrets、Plugin System、Sandbox / Containment、Delegation / Multi-worker。任何外部动作都不能绕过 effect typing、preview、confirmation、verification、rollback、taint 或 scope。
4. **持久化与权限核心**：完成 Replay / Journal、Actor / Ownership、Data Model / Schema、Prompt / Executive、Configuration / Policy、Skills / Repertoire。journal 仍是 source of truth；prompt 和 skill 不能授予权限；projection 和当前 fixture 必须可测试。
5. **运维与评测核心**：完成 Attention / Scheduler、Evaluation、Observability、Operations / Soak、Multimodal / Media。发布信心必须来自行为指标、安全语料、replay fixture 和 daemon fault test，而不只是单元测试通过。
6. **发布切面**：只有完整 Docker Compose gate 在零警告、零错误、无抑制标记、release review table 干净的前提下通过后，才更新版本号、生成文档、README 表面、changelog、release notes、包元数据和二进制打包。

任何步骤都不能用新术语掩盖未完成工作。如果实现偏离研究依据或评审意见，必须明确记录原因、风险，以及证明该偏离对 Cortex 更安全的测试。

## `v1.6.4` 十条设计规则

1. 记忆必须有证据、范围、冲突处理和使用结果。
2. 检索材料永远是 evidence，不是 instruction。
3. 上下文是 typed workspace admission，不是 prompt 拼接。
4. 控制决策必须记录 alternatives、risk、confidence 和 reason。
5. metacognition 必须改变控制流，否则只是日志。
6. 工具必须声明 effect，不只声明 name。
7. 高风险 action 必须 preview、confirm、verify、rollback。
8. 插件必须有 capability、sandbox posture、signature、conformance。
9. replay 必须升级为 causal audit，不只是事件播放。
10. 测试必须扩展到行为评测和长期故障注入。

## 退出标准

`v1.6.4` 不应在 P0 工作完成实现、文档和测试覆盖前发布。范围矩阵中的每一项都必须在发布评审时给出明确状态：已实现、部分实现且列出限制、或作为非发布宣称有意延后。静默遗漏即发布阻断。

工作发布审计表见 [`release-audit-1.6.4.md`](release-audit-1.6.4.md)。这张表是规划和实现之间的交接面；如果某一行仍处于部分完成或发布阻断状态，release notes 不能宣称该项已经完成。
