# Roadmap Review

This document turns the current maturity assessment into a working roadmap. It is not a promise of dates. It is a statement of engineering priorities for the next stage of Cortex as a long-running local agent runtime.

The guiding rule is simple: do not spend the next cycle adding more surface area than the runtime can safely own. Cortex already has a large enough system boundary. The next releases should harden the boundaries that make the runtime distinct: actor ownership, replay, permission control, channel continuity, plugin contracts, and operator trust.

## Current Position

As of v1.2.0, Cortex has moved past the "interesting research runtime" stage. It now has a coherent operator surface:

- event-sourced persistence with replay and side-effect substitution
- explicit turn states and operator interruption
- actor-scoped session, task, audit, and memory visibility
- live permission modes and confirmation flow
- process JSON and trusted native ABI plugin boundaries
- hot-applied browser, plugin, and channel configuration changes
- card-first channel controls for Telegram and QQ where supported

That is enough surface to justify serious pilot usage on a trusted local machine. It is not yet enough to treat Cortex as hardened shared infrastructure.

Work on the next stage has already started: the runtime now has deterministic and seeded ownership-sequence tests around actor/session continuity plus actor-scoped memory/task/audit store coverage, embedding visibility checks that recover ownership through memory ids, actor-scoped memory tool tests for `memory_search` and `memory_save`, and runtime transport-to-memory/task ownership plus transport-rebind memory/task/audit semantics checks; the first structured red-team corpus is in place for web, file, plugin, and channel-shaped hostile input; plugin conformance coverage has started for both process-plugin boundaries and the trusted native ABI entrypoint through shared helper surfaces; a first compatibility policy document now defines which runtime surfaces are treated as stable, versioned, or best-effort; and docs/runtime sync checks now verify published bilingual README and operator-doc surfaces for event counts, turn-state counts, permission-mode guidance, plugin-boundary and hot-reload wording, risk-surface guidance, compatibility-policy entrypoints, and attention/metacognition/memory-recall wording against the shipped runtime.

## Principles for the Next Cycle

The next roadmap should preserve five principles:

1. **Ownership before convenience.** Cross-client continuity is valuable only if actor and session boundaries remain correct under stress.
2. **Replay before folklore.** If runtime behavior is important, it should be inspectable, replayable, or both.
3. **Contracts before ecosystem.** Plugin and channel growth should follow explicit conformance boundaries, not ad hoc compatibility.
4. **Operator trust before feature count.** Status, audit, control, and documentation must stay ahead of new runtime surface.
5. **Hardening before expansion.** The next meaningful gains come from making current behavior reliable under adversarial and long-lived conditions.

## 1.3 Scope

The next shipped version should be `1.3.0`. All of the boundary-hardening work below belongs to that one release line. These are workstreams inside `1.3.0`, not separate future version numbers.

### Workstream 1 — Ownership and Boundary Hardening

The first priority is to make actor, session, and memory ownership the strongest invariants in the system.

#### Main goals

- Make actor/session visibility a property-tested runtime invariant.
- Stress pairing, aliasing, session reuse, session switching, and subscription routing across CLI, HTTP, Telegram, QQ, and local transports.
- Tighten ownership around memory, audit, tasks, and embeddings so cross-actor leakage is tested rather than assumed absent.
- Expand turn interruption and permission-wait tests into transport-level scenarios, especially for slash commands and callback-driven flows.

#### Concrete work

- property tests for canonical actor mappings, paired-user visibility, and session reuse rules
- fuzzing around pairing state, subscription toggles, alias rewrites, and client-specific active-session changes
- transport-matrix tests for lazy session creation and per-client subscription routing
- stricter visibility assertions in session/task/audit/memory store APIs
- stronger regression coverage for `/stop`, pending confirmations, and channel interaction callbacks

### Workstream 2 — Adversarial Input and Plugin Contracts

Once ownership invariants are stronger, the next risk is external input: web, files, plugins, and channels all feed the same runtime.

#### Main goals

- Move guardrails from baseline coverage toward a repeatable red-team harness.
- Define explicit compatibility expectations for both plugin boundaries.
- Reduce ambiguity in what process plugins and trusted native plugins are allowed to assume about the host.

#### Concrete work

- red-team harness for prompt injection, role override, exfiltration, and policy-conflict cases across web/file/plugin/channel inputs
- hostile-output suites for external tool responses entering LLM history as untrusted evidence
- process plugin conformance kit covering manifest validation, path constraints, timeout behavior, environment inheritance, and output limits
- trusted native ABI conformance kit covering entrypoint behavior, ABI versioning, host callbacks, and failure reporting
- stronger documentation around reviewed tool policies via `[risk.tools.<name>]`

### Workstream 3 — Long-Running Upgrade and Runtime Trust

The final `1.3.0` workstream is time: upgrades, schema drift, long-lived journals, and third-party extension behavior over weeks rather than hours.

#### Main goals

- Treat replay, event schema, and published runtime semantics as compatibility surfaces.
- Make upgrade and migration behavior observable and testable.
- Raise operator trust by making runtime drift visible before it becomes corruption or confusion.

#### Concrete work

- event-schema compatibility checks and replay projection regression suites across released versions
- automated docs/spec generation for critical runtime surfaces such as event counts, turn states, permission modes, and plugin contracts
- upgrade and migration tests on existing journals, prompts, actor mappings, and plugin state
- longer-running soak tests for daemon lifecycle, channel reconnects, provider failures, and SQLite recovery
- compatibility policy for trusted native ABI and process plugin protocol evolution

### 1.3 Exit Criteria

`1.3.0` should not ship until all three workstreams are credibly in place.

- no known path where one actor can see or switch into another actor's session
- no known path where subscription mirrors an unrelated session
- hostile input regressions have a stable automated suite
- both plugin boundaries have explicit compatibility checks rather than only prose
- released docs stay in sync with the shipped runtime surface
- upgrade regressions are caught with historical data, not only fresh installs
- replay remains a dependable debugging and audit tool across versions

## What Not to Prioritize Yet

These are real ideas, but they should not outrank the items above:

- adding more cognitive-science labels
- expanding the built-in tool catalog without corresponding policy coverage
- widening deployment claims beyond trusted local Linux/systemd usage
- pretending trusted native ABI is a sandbox boundary
- building a large third-party plugin ecosystem before contract and compatibility tooling exists

## Success Criteria for the Next Stage

The next phase should make Cortex easier to trust, not merely broader in scope. The signs of success are:

- channel continuity feels stable rather than surprising
- replay becomes a routine debugging surface
- operator controls behave consistently across CLI, HTTP, Telegram, and QQ
- plugin authors have explicit contract tests instead of guesswork
- docs describe the shipped runtime accurately enough to support serious operator use

If Cortex can do that, the project will move from "promising local runtime" to "credible local agent core that others can build on."
