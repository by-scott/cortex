# Maturity and Production Notes

Cortex is best understood as an early local agent runtime with serious systems work already in place. It is not a mature multi-tenant platform. The architecture is ambitious and much of it is implemented, but the project still needs time under real workloads, hostile inputs, and third-party extensions before it should be treated as hardened infrastructure.

## What Is Implemented

- Event-sourced journal on SQLite WAL, including large-payload externalization, checkpoints, replay helpers, and context-compaction boundaries.
- Explicit turn state machine with constrained transitions for processing, tool waits, permission waits, human input, compaction, consolidation, completion, interruption, and suspension.
- Layered memory model with lifecycle state, decay, reconsolidation, graph relations, hybrid recall, and consolidation paths.
- Runtime metacognition: attention channels, confidence tracking, doom-loop/fatigue/frame checks, adaptive thresholds, and tool utility tracking.
- Executive and Repertoire assets as files: prompt layers, bootstrap/resume context, active skills, retrieved evidence, tool schemas, recalled memory, and hot-reloaded skills/prompts.
- Multi-interface identity continuity through canonical actors and channel-specific aliases.
- Process-isolated plugin proxies, trusted native ABI loading, plugin skills/prompts, and runtime-aware tool execution.
- Actor-scoped session and long-term memory visibility for channel and transport identities.
- Replay side-effect substitution plus deterministic replay digest comparison.

## Accuracy of Cognitive Claims

The cognitive-science vocabulary is intentionally architectural, not a claim of formal equivalence. For example:

- "Global workspace" maps to foreground scheduling and journal broadcast.
- "Drift diffusion" maps to bounded fixed-delta confidence accumulation.
- "Complementary learning systems" maps to captured/materialized/stabilized memory lifecycle and consolidation heuristics.
- "Reward prediction error" maps to EWMA tool utility plus UCB1-style exploration.

This framing is useful for engineering consistency, but it should not be read as a validated cognitive architecture.

## Current Trust Boundaries

Cortex has two plugin boundaries. Process JSON is the default external boundary: manifest-declared proxy tools run as child processes over a JSON stdin/stdout protocol with controlled cwd, environment, timeout, output limits, host-path opt-in, and Unix CPU/memory rlimits. Trusted native ABI plugins are shared libraries loaded in-process through `cortex_plugin_init`; they are a strong-trust extension boundary, not a sandbox.

Tool risk is a gate, not a containment system. Built-in tools receive explicit baseline scores. Unknown tools, including plugin and MCP tools without a specific profile, are treated conservatively and require confirmation by default. Production deployments should still define explicit allowlists, deny rules, and per-tool policies.

Per-tool policies can be declared in `[risk.tools.<name>]` to override risk axes, force confirmation, or block a tool. Use this for reviewed plugin and MCP tools so safe tools can be less noisy and powerful tools can be held behind explicit confirmation.

External tool output is recorded as untrusted provenance and wrapped before entering LLM history so returned web/file/plugin content is presented as evidence rather than instructions. Guardrails add baseline detection for prompt injection, system-prompt leakage, role override, and exfiltration patterns; suspicious tool inputs force confirmation for mutating tools, and suspicious tool outputs are journaled for audit.

Replay is deterministic where side effects are recorded. The replay projector substitutes provider-supplied values for `SideEffectRecorded` events, which closes the projection loop for recorded LLM responses, wall-clock values, random values, and external I/O outputs. `replay_determinism_digest` compares equivalent projections while excluding event ids and timestamps. Tools that mutate external systems still need idempotency and audit design outside the journal.

## Not Yet

- No sandbox for trusted native shared-library plugins.
- No container/seccomp-style sandbox for process-isolated plugin commands; current process controls are path, environment, timeout, output, and Unix rlimit constraints.
- Trusted native shared-library changes still require a daemon restart to take effect.
- No claim of hostile multi-tenant hardening across OS users or untrusted plugins.
- No complete adversarial prompt-injection defense beyond provenance wrapping, structured guardrails, and audit events.
- No full containment for tools that mutate external systems.

## Threat Model Notes

Personal local use assumes a trusted user, trusted machine account, and trusted plugin sources. Main risks are accidental destructive tool calls, leaked local secrets, stale memory, and external service side effects.

Team or shared workstation use adds channel identity, operator approval, and plugin provenance risks. Require explicit actor mappings, enable auth, and use `[risk.tools.<name>]` policies for tools that publish, deploy, delete, spend money, or access credentials.

Multi-tenant use has actor-scoped session visibility plus actor-enforced memory, session, task, and audit store APIs, but it is not a hardened deployment target across hostile tenants. Embedding vectors inherit ownership through memory ids rather than carrying separate actor metadata. Hostile tenancy would still require process/container isolation, per-tenant storage roots, plugin sandboxing beyond child-process controls, stronger policy enforcement, quota isolation, and adversarial input testing beyond the current baseline.

## Production Hardening Backlog

- Add container/seccomp isolation options for untrusted process plugins.
- Expand prompt-injection handling beyond current provenance wrapping and regex/literal checks, especially for web, file, and cross-channel inputs.
- Extend the current soak/fault harness into continuously running daemon tests for provider, channel, and database failures.
- Document operational threat models for personal local use, team use, and multi-tenant deployment separately.

For the current contract boundaries, see the [Compatibility Policy](compatibility.md). For the staged follow-up priorities, see the [Roadmap Review](roadmap.md).
