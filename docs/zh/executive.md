# Executive

Cortex 1.5 不再把 Executive 当成 prompt 文本。Executive 是 runtime control
surface：负责 workspace item 准入、evidence 评估、control action 选择和
ownership enforcement。

当前已经实现的机制：

- `BroadcastFrame` 执行有界 workspace admission 和 salience / urgency
  competition。
- `GoalGraph` 实现 strategic、episodic、contextual、sensorimotor 四层
  hierarchical control goal。parent-child link 必须从更抽象控制层指向更
  具体控制层；active goals 可以显式互相 inhibit。
- `GoalGraph::top_down_bias` 把 Miller/Cohen 风格的 biasing 实现为 active
  goals 对 action candidate 的类型化 tag overlap。
- `LoadProfile` 测量 intrinsic、extraneous、germane context load，并把
  pressure 映射到 `PressureAction`。
- `MonitoringReport` 把 goal conflict、load pressure、feedback conflict、
  frame anchoring、calibration drift 评估成具体 control signal。
- `Accumulator` 实现 drift 风格 evidence accumulation。
- `ExpectedControlValue` 计算 intervention benefit、cost、risk。
- `ControlDecision` 把 accumulated evidence 和 EVC 映射为具体动作。
- `TurnExecutor` 装配 runtime context，把 retrieved material 包裹为
  untrusted evidence，调用 model provider，并保留 provider token usage。
- `PolicyMode` 评估 tool / action risk，并且即使在 open mode 也拒绝
  cross-owner action。
- `OutboundMessage` 按 transport capability 规划 delivery。
- `CortexRuntime` 作为 daemon-first boundary 运行，绑定 client，在第一轮
  turn 创建或复用 actor-visible session，按 client 激活 session，通过 SDK
  contract 执行 tool，并且只把 outbound plan 投递给 active session 与
  delivery session 匹配的 subscriber。
- `TransportAdapter` 按 Telegram、QQ、CLI 各自的 Markdown / plain / media
  capability 渲染 delivery plan。

Executive 还没有完成。HTTP / live transport clients、media tools、native /
process plugin loaders 仍需在同一套 ownership、side-effect 和 gate contract
之上重建。
