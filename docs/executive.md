# Executive

Cortex 1.5 no longer treats the Executive as prompt text. The Executive is the
runtime control surface that admits workspace items, evaluates evidence,
chooses control actions, and enforces ownership.

Current implemented mechanisms:

- `BroadcastFrame` performs bounded workspace admission and salience/urgency
  competition.
- `GoalGraph` implements hierarchical control goals across strategic,
  episodic, contextual, and sensorimotor levels. Parent-child links must move
  from more abstract to more concrete control, and active goals can explicitly
  inhibit one another.
- `GoalGraph::top_down_bias` implements Miller/Cohen-style biasing as typed
  tag overlap from active goals into action candidates.
- `LoadProfile` measures intrinsic, extraneous, and germane context load and
  maps pressure to `PressureAction`.
- `MonitoringReport` evaluates goal conflict, load pressure, feedback conflict,
  frame anchoring, and calibration drift into concrete control signals.
- `Accumulator` implements drift-style evidence accumulation.
- `ExpectedControlValue` scores intervention benefit, cost, and risk.
- `ControlDecision` maps accumulated evidence and EVC into concrete actions.
- `TurnExecutor` assembles the runtime context, wraps retrieved material as
  untrusted evidence, calls a model provider, and preserves provider token
  usage.
- `PolicyMode` evaluates tool/action risk and denies cross-owner actions even
  in open mode.
- `OutboundMessage` plans delivery according to transport capabilities.
- `CortexRuntime` runs as a daemon-first boundary, binds clients, creates or
  reuses an actor-visible session on first turn, activates sessions per client,
  executes tools through the SDK contract, and only delivers outbound plans to
  subscribers whose active session matches the delivery session.
- `TransportAdapter` renders delivery plans for Telegram, QQ, and CLI according
  to each transport's Markdown/plain/media capability.

The Executive is not complete until HTTP/live transport clients, media tools,
and native/process plugin loaders are rebuilt on the same ownership,
side-effect, and gate contracts.
