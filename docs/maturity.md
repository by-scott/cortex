# Maturity and Production Notes

Cortex is best understood as an early local agent runtime with serious systems work already in place. It is not a mature multi-tenant platform. The architecture is ambitious and much of it is implemented, but the project still needs time under real workloads, hostile inputs, and third-party extensions before it should be treated as hardened infrastructure.

## What Is Implemented

- Event-sourced journal on SQLite WAL, including large-payload externalization, checkpoints, replay helpers, and context-compaction boundaries.
- Explicit turn state machine with constrained transitions for processing, tool waits, permission waits, human input, compaction, consolidation, completion, interruption, and suspension.
- Layered memory model with lifecycle state, decay, reconsolidation, graph relations, hybrid recall, and consolidation paths.
- Runtime metacognition: attention channels, confidence tracking, doom-loop/fatigue/frame checks, adaptive thresholds, and tool utility tracking.
- Executive and Repertoire assets as files: prompt layers, bootstrap/resume context, active skills, tool schemas, recalled memory, and hot-reloaded skills/prompts.
- Multi-interface identity continuity through canonical actors and channel-specific aliases.
- Native plugin loading, process-isolated plugin tool proxies, plugin skills/prompts, SDK/ABI checks, and runtime-aware tool execution.
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

Native plugins have two boundaries. `trusted_in_process` plugins run in the daemon process through `dlopen` and FFI entry points, and returned trait objects are kept alive by retaining the shared library handle. `process` plugins register manifest-declared proxy tools and run as child processes over a JSON stdin/stdout protocol. In-process plugins are practical Rust extensions, not a long-term stable binary ABI; manifests declare SDK version and ABI revision and incompatible values are rejected before loading.

Tool risk is a gate, not a containment system. Built-in tools receive explicit baseline scores. Unknown tools, including plugin and MCP tools without a specific profile, are now treated conservatively and require confirmation by default. Production deployments should still define explicit allowlists, deny rules, and per-tool policies.

Per-tool policies can be declared in `[risk.tools.<name>]` to override risk axes, force confirmation, or block a tool. Use this for reviewed plugin and MCP tools so safe tools can be less noisy and powerful tools can be held behind explicit confirmation.

External tool output is recorded as untrusted provenance and wrapped before entering LLM history so returned web/file/plugin content is presented as evidence rather than instructions. Guardrails add baseline detection for prompt injection, system-prompt leakage, role override, and exfiltration patterns, and guardrail hits are journaled for audit.

Replay is deterministic where side effects are recorded. The replay projector substitutes provider-supplied values for `SideEffectRecorded` events, which closes the projection loop for recorded LLM responses, wall-clock values, random values, and external I/O outputs. `replay_determinism_digest` compares equivalent projections while excluding event ids and timestamps. Tools that mutate external systems still need idempotency and audit design outside the journal.

## Not Yet

- No stable long-term binary ABI for compiled plugin shared libraries.
- No container/seccomp-style sandbox for process-isolated plugin commands.
- No native manifest/tool-set hot-swap; process-isolated command implementation updates apply next invocation, but manifest changes and in-process library updates require daemon restart.
- No claim of hostile multi-tenant hardening across OS users or untrusted plugins.
- No complete adversarial prompt-injection defense beyond provenance wrapping, structured guardrails, and audit events.
- No full containment for tools that mutate external systems.

## Threat Model Notes

Personal local use assumes a trusted user, trusted machine account, and trusted plugin sources. Main risks are accidental destructive tool calls, leaked local secrets, stale memory, and external service side effects.

Team or shared workstation use adds channel identity, operator approval, and plugin provenance risks. Require explicit actor mappings, enable auth, and use `[risk.tools.<name>]` policies for tools that publish, deploy, delete, spend money, or access credentials.

Multi-tenant use has actor-scoped session and memory visibility, but it is not a hardened deployment target across hostile tenants. That would require process/container isolation, per-tenant storage roots, plugin sandboxing, stronger policy enforcement, quota isolation, and adversarial input testing beyond the current baseline.

## Production Hardening Backlog

- Add container/seccomp isolation options for untrusted process plugins.
- Expand prompt-injection handling beyond current provenance wrapping and regex/literal checks, especially for web, file, and cross-channel inputs.
- Add long-running daemon soak tests and failure-injection tests for provider, channel, and database failures.
- Document operational threat models for personal local use, team use, and multi-tenant deployment separately.
