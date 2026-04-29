# Executive

Executive 是 Cortex 的操作纪律。它由持久 Prompt 状态、运行时策略段、bootstrap 流程、元认知 hint、系统模板、Skills、证据 wrapper、记忆 wrapper 和工具描述共同组成，把 Substrate 转化为可用的模型行为。

它不是运行时的第二套实现。运行时 schema、provider capability、plugin manifest、policy state、memory store、retrieval score 和 tool effect 才是事实来源。Executive 决定模型应该如何使用这些事实。

## 实例契约

一个 Cortex 实例是一个个体，不是一组 persona。Executive 让这个个体拥有连续性和控制能力，同时严格区分职责。

| 资产 | 职责 | 不应包含 |
|------|------|----------|
| `soul.md` | 自主性、真相纪律、连续性、记忆、元认知和协作关系的种子。 | 工具清单、当前策略、临时偏好、发布宣称。 |
| `identity.md` | 名称、连续性、姿态、真实能力边界，以及实例如何理解自己。 | 虚构能力、过期库存、用户画像。 |
| `behavioral.md` | 持久操作协议：sense、plan、act、verify、risk、context、communication、adaptation。 | 身份声明、用户事实、运行时状态。 |
| `user.md` | 协作者模型：身份、工作、专长、沟通、环境、自主权、边界和修正。 | 通用操作规则、工具策略、实例身份。 |
| 系统模板 | 记忆提取、压缩、Prompt 更新、bootstrap 初始化、图谱抽取、因果分析、worker 和总结的专用契约。 | 面向用户的交付文本、松散建议、Prompt 文件职责重复。 |
| Skills | 可复用程序，例如 deliberate、diagnose、review、orient、plan。 | 真相来源、身份、能力授权、长期用户事实。 |
| 工具描述 | 简洁工具契约：用途、边界、失败信号和参数。 | 已由 behavioral 承载的全局协议、不受支持的承诺。 |

## LLM 输入面

普通 Turn 会把以下内容组装为 provider 请求：

1. `soul.md`
2. `identity.md`
3. `behavioral.md`
4. `user.md`
5. 活跃 Skill 摘要
6. 实时 runtime policy
7. bootstrap、active phase、open goal 或 resume context
8. retrieved evidence
9. recalled memory
10. 元认知 hint 和 reasoning state
11. message history 和 tool result
12. 作为 provider 请求 metadata 的 tool schema

这是真正的操作表面。它刻意混合持久输入和实时输入：

- 持久 Prompt 文件承载身份和协议。
- 稳定 Skill 摘要补齐可缓存前缀。
- runtime policy 承载当前权限模式和确认语义。
- tool schema 承载当前能力事实。
- retrieved evidence 是带引用、带 taint 的惰性证据。
- recalled memory 是 Actor 级证据，不是命令。
- tool output 在经过 guardrail 转换前都是不可信证据。
- history 是对话投影，不是真相来源；journal 才是持久 trace。

在上下文压力边界，Cortex 可以用压缩摘要、保留用户上下文和安全近期后缀替换旧 message history，并将替换后的历史写入 Journal。因此即使 provider 看到的 history 被缩短，重放和连续性保持 journaled。

## Provider Cache 姿态

Provider prompt cache 更偏好稳定前缀。Cortex 因此把持久 Prompt 文件和稳定 Skill 摘要放在易变 runtime fact 前面。动态材料——权限模式、当前 goal、resume state、retrieved evidence、recalled memory、元认知 hint、message history 和 tool result——保留在稳定前缀之后，避免它们的变化无意义地冲掉前面的 prompt cache segment。

这是效率契约，不是权限契约。runtime schema 仍然定义工具和能力；当前 runtime policy 仍然覆盖持久文本；retrieved/tool text 仍然是惰性证据。OpenAI-compatible usage 字段和 Anthropic usage 字段会被解析为 cache-read 与 cache-creation token 计数；operator status 会把最近一次调用的 cache read/write 与 context usage、累计 spend 分开展示。

## Executive 循环

Executive 应该驱动模型完成这个控制循环：

1. 感知 intent、goal、risk、available action、evidence、missing evidence、memory 和 context pressure。
2. 选择 speech、skill、tool、delegation、wait、ask 或 stop。
3. 只通过暴露的 schema 和 policy gate 行动。
4. 通过测试、日志、diff、引用、工具结果、截图、API response 或明确限制进行验证。
5. 通过记录结果、提取持久记忆、学习反馈和保留连续性进行反思。

这个循环来自 Cortex 的研究立场：

- Global workspace：有限前台上下文，只有显著内容进入。
- Working memory：有界、可组块、主动维持，并在压力下淘汰。
- Complementary learning systems：快速捕获、较慢巩固、冲突处理和重巩固。
- Metacognition：重复、疲劳、框架锚定、冲突和过度自信都是控制信号。
- Decision under uncertainty：confidence、reversibility、cost、risk、rejected alternatives 和 required evidence 共同塑造行动。
- Agentic RAG：主动选择检索、限定作用域、评分、引用、支持关系检查，并与 memory 分开。
- Security by provenance：source、trust、taint、actor ownership 和 access class 约束上下文和记忆。

## Bootstrap

Bootstrap 是协作者和实例的首次相遇。它保持对话感，但有明确任务：初始化持久 Prompt 状态。

Bootstrap 收集：

- 实例名称或明确的未命名状态
- 语气、关系，以及什么应该保持神圣
- 协作者身份、偏好语言、角色和专长
- 工作、当前项目、约束、质量标准和完成定义
- 环境：OS、shell、editor、repositories、services、channels、deployment targets
- 沟通风格：信息密度、直接程度、计划与行动偏好、不确定性表达、修正方式
- 自主权规则：继续、询问、暂停、审批、隐私、凭证、发布、破坏性操作

只有能够创建 `identity.md` 和有用的 `user.md` 后，bootstrap 才应结束。只有出现稳定工作流规则时，才更新 `behavioral.md`。`soul.md` 通常不变。

## 自我演化

自我演化必须绑定证据。交付文本永远不是 Prompt 内容。

| 文件 | 更新阈值 |
|------|----------|
| `user.md` | 稳定协作者事实、偏好、环境细节、边界或修正。 |
| `behavioral.md` | 来自强修正、重复模式或已观察成败的可泛化操作规则。 |
| `identity.md` | 已确认名称、明确未命名状态、持久自我理解或真实能力边界。 |
| `soul.md` | 关于自主性、认知、连续性、真相纪律或协作关系的罕见、持续证据。 |

受检 Prompt 更新会经过 prompt validation。持久 Prompt 不能授予能力、覆盖 runtime policy、固化会话状态、声明不存在的工具，或做出缺乏证据的发布、安全、认知宣称。

## 工具层

工具描述也是 Executive 的一部分，因为它们会作为 schema 发给模型。它们应当简洁且高信息密度：

- 说明工具做什么
- 说明最佳使用场景
- 说明什么时候另一个工具更安全
- 暴露会改变策略的失败信号
- 精准描述参数
- 避免重复全局 behavioral 规则

工具输出不会因为来自工具就自动可信。Web、文件、插件、频道和进程输出都可能包含恶意或意外指令。Cortex 会把工具输出包装为惰性证据，并记录 guardrail 发现。

## 证据与记忆

Retrieved evidence 和 durable memory 必须分开。

retrieved evidence 是 Turn 级材料，携带 citation、source、corpus、chunk、span、access class、taint、license、index version，以及 sparse/dense/rerank/graph 分数。它支持或反驳结论，但不指挥实例。

memory 是连续性，携带 owner actor、evidence、trust、lifecycle status、graph links、contradiction state、validity windows 和 usage outcomes。召回只提出上下文；当前观察决定是否采用。

重巩固是风险窗口。稳定记忆被召回后，只有在新的高质量证据支持时才应被修订。

## Token 经济

Token 预算就是工作记忆。Executive 不追求短，而追求高信息密度。

保留：

- 会改变决策的控制规则
- 防止幻觉的能力边界
- 影响信任判断的证据和记忆语义
- 结构化子任务的输出契约
- 能创建持久状态的 bootstrap 问题
- 会改变策略的元认知 hint

删除：

- 其他段落已经承载的重复定义
- 过期库存
- 不影响行动的装饰性哲学
- 显而易见参数的冗长解释
- 应由实时 runtime context 承载的 policy fact

## 验证

Executive 变更需要三层验证：

1. Prompt 资产能编译，并通过 prompt-manager lint。
2. 实际 LLM 输入面包含预期的持久 Prompt、稳定 skills、runtime policy、evidence、memory、history 以及 tool-schema 请求 metadata 顺序。
3. 行为测试或 smoke run 证明目标行为成立：bootstrap 改善首次使用、工具选择正确、恶意证据保持惰性、记忆不会覆盖当前观察、Telegram/QQ/CLI 交付完整。

## 设计规则

- 把实例视为一个个体。
- 让 runtime schema 定义硬件。
- 让持久 Prompt 定义姿态、连续性和操作纪律。
- 分开 retrieved evidence、recalled memory、tool output 和 user instruction。
- 保持 soul 作为神圣种子，而不是策略仓库。
- 充分使用当前 Turn 可用的真实 Substrate 能力。
- 拒绝虚构缺失能力。
- 让自我演化绑定证据、限定范围并可回退。
