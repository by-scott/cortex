# Executive

Cortex 1.5 no longer treats the Executive as prompt text. The Executive is the
runtime control surface that admits workspace items, evaluates evidence,
chooses control actions, and enforces ownership.

Current implemented mechanisms:

- `BroadcastFrame` performs bounded workspace admission and salience/urgency
  competition.
- `Accumulator` implements drift-style evidence accumulation.
- `ExpectedControlValue` scores intervention benefit, cost, and risk.
- `ControlDecision` maps accumulated evidence and EVC into concrete actions.
- `TurnExecutor` assembles the runtime context, wraps retrieved material as
  untrusted evidence, calls a model provider, and preserves provider token
  usage.
- `PolicyMode` evaluates tool/action risk and denies cross-owner actions even
  in open mode.
- `OutboundMessage` plans delivery according to transport capabilities.
- `CortexRuntime` binds clients, creates or reuses an actor-visible session on
  first turn, activates sessions per client, and only delivers outbound plans
  to subscribers whose active session matches the delivery session.
- `TransportAdapter` renders delivery plans for Telegram, QQ, and CLI according
  to each transport's Markdown/plain/media capability.

The Executive is not complete until these mechanisms are wired into a full turn
runtime with tool execution, permission persistence, live transport clients,
and replay recovery.
