# Maturity and Production Notes

Cortex is best understood as an early local language-model harness with serious systems work already in place. It is not an autonomous work-execution product and it is not a mature multi-tenant platform. The architecture is ambitious and much of it is implemented, but the project still needs time under real workloads, hostile inputs, and third-party extensions before it should be treated as hardened infrastructure.

## What Is Implemented

- Event-sourced journal on SQLite WAL, including large-payload externalization, checkpoints, replay helpers, and context-compaction boundaries.
- Explicit turn state machine with constrained transitions for processing, tool waits, permission waits, human input, compaction, consolidation, completion, interruption, and suspension.
- Layered memory model with lifecycle state, evidence-backed claims, contradiction links, validity windows, usage outcomes, decay, reconsolidation, graph relations, hybrid recall, and consolidation paths.
- Runtime metacognition: attention channels, confidence tracking, doom-loop/fatigue/frame checks, outcome-calibrated adaptive thresholds, and tool utility tracking.
- Executive and Repertoire assets as files: prompt state, bootstrap/resume context, active skills, retrieved evidence, tool schemas, recalled memory, and hot-reloaded skills/prompts.
- Workspace admission with typed lanes, utility/risk/volatility scoring, taint barriers, budget-aware marginal utility, and eviction records.
- Skill manifests and bounded execution traces with effects, risk, success criteria, fallback, observability, trigger, duration, and status.
- Model capability routing with group profiles for coding, long context, vision, tool calling, JSON reliability, latency, cost, safety, and reasoning depth, including fallback and risk/confidence escalation explanations.
- Multi-interface identity continuity through canonical actors and channel-specific aliases.
- Process-isolated plugin proxies, trusted native ABI loading, plugin skills/prompts, and runtime-aware tool execution.
- Actor-scoped session and long-term memory visibility for channel and transport identities.
- Structured guardrail assessment for external content, including taint disposition, safe transformations, journaled guardrail events, and hostile-source memory candidates.
- RAG evidence roles, deterministic answer-claim support reports, and negative-evidence handling for contradicted or stale retrieved facts.
- Declarative tool effects for core tools and plugins, with effect-based risk floors plus preview, verification, and commit events around tool execution.
- Replay side-effect substitution, projection versions, replay diffs, causal audit graph edges, current replay fixtures, and deterministic replay digest comparison.
- Policy-as-code lint and simulation through `cortex policy lint`, `cortex policy simulate`, and daemon startup findings.

## Accuracy of Cognitive Claims

The cognitive-science vocabulary is intentionally architectural, not a claim of formal equivalence. For example:

- "Global workspace" maps to foreground scheduling and journal broadcast.
- "Drift diffusion" maps to bounded fixed-delta confidence accumulation.
- "Complementary learning systems" maps to captured/materialized/stabilized memory lifecycle, evidence quality, contradiction handling, and consolidation heuristics.
- "Reward prediction error" maps to EWMA tool utility plus UCB1-style exploration.

This framing is useful for engineering consistency, but it should not be read as a validated cognitive architecture.

## Current Trust Boundaries

Cortex has two plugin boundaries. Process JSON is the default external boundary: manifest-declared proxy tools run as child processes over a JSON stdin/stdout protocol with controlled cwd, environment, timeout, output limits, host-path opt-in, and Unix CPU/memory rlimits. Trusted native ABI plugins are shared libraries loaded in-process through `cortex_plugin_init`; they are a strong-trust extension boundary, not a sandbox.

Plugin packages now carry a governance contract: trust tier, requested file/network/process/secret/background capabilities, sandbox profile, package metadata, signed-package fields, SBOM/risk-profile references, conformance certificate, and tool effects. The runtime validates impossible or unsafe combinations before loading, rejects sandbox-enforcement claims it cannot actually provide, process tools expose manifest-declared effects plus a forced subprocess effect to risk scoring, `cortex plugin review` shows the install surface, and `cortex plugin test` runs the local conformance kit. These controls improve operator visibility and deny obviously unsafe combinations; they are still not equivalent to kernel/container isolation.

Tool risk is a gate, not a containment system. Built-in tools receive explicit baseline scores and now declare effect surfaces such as file read/write, process execution, network request, memory persistence, channel send, scheduling, media generation, and delegation. Unknown tools, including plugin and MCP tools without a specific profile, are treated conservatively and require confirmation by default. Production deployments should still define explicit allowlists, deny rules, and per-tool policies.

The runtime home is treated as a protected root for ordinary tool execution. File/edit/write tools are blocked from accessing the instance directory, symlinked paths are resolved before the check, and process/script tools are blocked while the protected root is active. Plugin tools that present prompt, config, session, journal, memory, or runtime-state mutation are blocked from direct mutation; self-evolution plugins should return structured proposals for the checked PromptManager/runtime-command path. This is a governance boundary for prompt/config/state mutation; it is not a replacement for OS-level plugin sandboxing.

Per-tool policies can be declared in `[risk.tools.<name>]` to override risk axes, force confirmation, or block a tool. Use this for reviewed plugin and MCP tools so safe tools can be less noisy and powerful tools can be held behind explicit confirmation.

External tool output is recorded with provenance and assessed before entering LLM history. Benign external content is quoted as evidence; hostile content is reduced to summary or metadata-only evidence, raw hostile text is not reintroduced into history, and the source is journaled for audit. Guardrails add baseline detection for prompt injection, system-prompt leakage, role override, and exfiltration patterns; suspicious tool inputs force confirmation for mutating tools, suspicious tool outputs are journaled for audit as guardrail events, and post-turn processing can create hostile-source memory candidates for future turns.

Replay is deterministic where side effects are recorded. The replay projector substitutes provider-supplied values for `SideEffectRecorded` events, which closes the projection loop for recorded LLM responses, wall-clock values, random values, and external I/O outputs. Tool execution also records preview, verification, and commit events for declared effects. `replay_determinism_digest` compares equivalent projections while excluding event ids and timestamps. Tools that mutate external systems still need idempotency and deeper rollback design outside the journal.

Policy-as-code is a preflight gate, not a sandbox. `cortex policy lint` reports dangerous combinations in config and enabled plugin manifests, and `cortex policy simulate` explains a single tool/effect decision before the tool runs. Daemon startup logs the same findings so high-risk posture is visible before the first tool call. These checks improve operator review but do not replace OS isolation, credential brokering, or runtime authorization.

Model routing is a deterministic capability decision surface, not a live provider benchmark. Profiles can be declared in `[llm_groups.*]` or inferred conservatively from group name, provider protocol, model name, and score hints. The resolver can explain selected group, fallback reason, escalation, and cost/latency/safety tradeoff; it still depends on accurate operator-supplied provider/model metadata and future provider-health observations.

The operator dashboard is a structured runtime inspection surface, not a general observability stack. It exposes daemon state, token and tool metrics, active/persisted sessions, shared actor bindings, pending work, model profiles, and a bounded journal timeline normalized into lifecycle, message, LLM, tool, permission, workspace, retrieval, memory, control, guardrail, scheduler, and other event categories. It improves operator triage without replacing tracing, long-term metrics storage, or audit review.

## Not Yet

- No sandbox for trusted native shared-library plugins.
- Sandbox profiles are declared and validated, but there is no container/seccomp-style enforcement for process-isolated plugin commands yet. Current process controls are path, environment, timeout, output, and Unix rlimit constraints; manifests that claim `uid_no_network`, `system_sandbox`, `container_vm`, `remote_worker`, `sandbox.network = "none"`, `sandbox.uid_drop = true`, or non-empty `sandbox.seccomp` are rejected until enforcement exists.
- Trusted native shared-library changes still require a daemon restart to take effect.
- No claim of hostile multi-tenant hardening across OS users or untrusted plugins.
- No complete adversarial prompt-injection defense beyond provenance wrapping, typed taint disposition, safe transformations, structured guardrails, hostile-source memory candidates, and audit events.
- No full containment for tools that mutate external systems.
- No automated provider benchmarking or SLA-grade live model health scoring yet; model routing uses declared or inferred profiles plus explicit failure signals.

## Threat Model Notes

Personal local use assumes a trusted user, trusted machine account, and trusted plugin sources. Main risks are accidental destructive tool calls, leaked local secrets, stale memory, and external service side effects.

Team or shared workstation use adds channel identity, operator approval, and plugin provenance risks. Require explicit actor mappings, enable auth, and use `[risk.tools.<name>]` policies for tools that publish, deploy, delete, spend money, or access credentials.

Multi-tenant use has actor-scoped session visibility plus actor-enforced memory, session, task, goal, and audit store APIs, but it is not a hardened deployment target across hostile tenants. Embedding vectors inherit ownership through memory ids rather than carrying separate actor metadata. Hostile tenancy would still require process/container isolation, per-tenant storage roots, plugin sandboxing beyond child-process controls, stronger policy enforcement, quota isolation, and adversarial input testing beyond the current baseline.

## Production Hardening Backlog

- Add container/seccomp isolation options for untrusted process plugins.
- Expand prompt-injection handling beyond current provenance wrapping, typed taint disposition, safe transformations, and regex/literal checks, especially for web, file, and cross-channel inputs.
- Extend the current soak/fault harness into continuously running daemon tests for provider, channel, and database failures.
- Feed provider failures, invalid schemas, and latency/cost observations back into the model capability registry for longer-running calibration.
- Document operational threat models for personal local use, team use, and multi-tenant deployment separately.

For the staged follow-up priorities, see the [Roadmap Review](roadmap.md).
