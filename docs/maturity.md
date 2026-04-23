# Maturity and Production Notes

Cortex is best understood as an early local agent runtime with serious systems work already in place. It is not a mature multi-tenant platform. The architecture is ambitious and much of it is implemented, but the project still needs time under real workloads, hostile inputs, and third-party extensions before it should be treated as hardened infrastructure.

## What Is Implemented

- Event-sourced journal on SQLite WAL, including large-payload externalization, checkpoints, replay helpers, and context-compaction boundaries.
- Explicit turn state machine with constrained transitions for processing, tool waits, permission waits, human input, compaction, consolidation, completion, interruption, and suspension.
- Layered memory model with lifecycle state, decay, reconsolidation, graph relations, hybrid recall, and consolidation paths.
- Runtime metacognition: attention channels, confidence tracking, doom-loop/fatigue/frame checks, adaptive thresholds, and tool utility tracking.
- Executive and Repertoire assets as files: prompt layers, bootstrap/resume context, active skills, tool schemas, recalled memory, and hot-reloaded skills/prompts.
- Multi-interface identity continuity through canonical actors and channel-specific aliases.
- Native plugin loading, plugin skills/prompts, SDK types, and runtime-aware tool execution.

## Accuracy of Cognitive Claims

The cognitive-science vocabulary is intentionally architectural, not a claim of formal equivalence. For example:

- "Global workspace" maps to foreground scheduling and journal broadcast.
- "Drift diffusion" maps to bounded fixed-delta confidence accumulation.
- "Complementary learning systems" maps to captured/materialized/stabilized memory lifecycle and consolidation heuristics.
- "Reward prediction error" maps to EWMA tool utility plus UCB1-style exploration.

This framing is useful for engineering consistency, but it should not be read as a validated cognitive architecture.

## Current Trust Boundaries

Native plugins are trusted code. They run in the daemon process through `dlopen` and FFI entry points, and returned trait objects are kept alive by retaining the shared library handle. This is a practical Rust extension mechanism, not a sandboxed or long-term stable binary ABI. Install plugins only from sources you trust.

Tool risk is a gate, not a containment system. Built-in tools receive explicit baseline scores. Unknown tools, including plugin and MCP tools without a specific profile, are now treated conservatively and require confirmation by default. Production deployments should still define explicit allowlists, deny rules, and per-tool policies.

Per-tool policies can be declared in `[risk.tools.<name>]` to override risk axes, force confirmation, or block a tool. Use this for reviewed plugin and MCP tools so safe tools can be less noisy and powerful tools can be held behind explicit confirmation.

Guardrails are baseline detection. They combine literal markers and regex detection for prompt injection and system-prompt leakage. They reduce common accidents and naive attacks, but they are not an adversarial security boundary.

Replay is deterministic only where side effects are recorded. The replay projector substitutes provider-supplied values for `SideEffectRecorded` events, which closes the projection loop for recorded LLM responses, wall-clock values, random values, and external I/O outputs. Tools that mutate external systems still need idempotency and audit design outside the journal.

## Not Yet

- No sandbox for native plugins.
- No stable long-term binary ABI for compiled plugin shared libraries.
- No native plugin hot-swap; daemon restart is required for new or updated shared libraries.
- No claim of multi-tenant hardening.
- No complete adversarial prompt-injection defense.
- No full containment for tools that mutate external systems.

## Threat Model Notes

Personal local use assumes a trusted user, trusted machine account, and trusted plugin sources. Main risks are accidental destructive tool calls, leaked local secrets, stale memory, and external service side effects.

Team or shared workstation use adds channel identity, operator approval, and plugin provenance risks. Require explicit actor mappings, enable auth, and use `[risk.tools.<name>]` policies for tools that publish, deploy, delete, spend money, or access credentials.

Multi-tenant use is not currently in scope as a hardened deployment target. It would require process/container isolation, tenant-scoped storage, plugin sandboxing, stronger policy enforcement, quota isolation, and adversarial input testing beyond the current baseline.

## Production Hardening Backlog

- Consider process or container isolation for untrusted native plugins.
- Expand prompt-injection handling beyond keyword/regex checks, especially for web, file, and cross-channel inputs.
- Add compatibility tests for plugin SDK versions and manifest negotiation.
- Add long-running daemon soak tests, replay determinism tests, and failure-injection tests for provider, channel, and database failures.
- Document operational threat models for personal local use, team use, and multi-tenant deployment separately.
