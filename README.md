<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center"><strong>Cognitive Runtime for Language Models</strong></p>
  <p align="center">
    <a href="https://github.com/by-scott/cortex/releases"><img src="https://img.shields.io/github/v/release/by-scott/cortex?display_name=tag" alt="Release"></a>
    <a href="https://crates.io/crates/cortex-sdk"><img src="https://img.shields.io/crates/v/cortex-sdk" alt="Crates.io"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
  <p align="center">
    <a href="docs/quickstart.md">Quick Start</a> ·
    <a href="docs/usage.md">Usage</a> ·
    <a href="docs/config.md">Configuration</a> ·
    <a href="docs/plugins.md">Plugins</a> ·
    <a href="docs/compatibility.md">Compatibility</a> ·
    <a href="docs/roadmap.md">Roadmap</a> ·
    <a href="README.zh.md">中文</a>
  </p>
</p>

---

Modern agent frameworks have brought language models remarkably far —
persistent memory, tool orchestration, multi-step planning, and context
management are increasingly mature capabilities across the ecosystem. Cortex
takes a complementary approach: rather than assembling these capabilities ad
hoc, it organizes them around cognitive-science-inspired runtime constraints.

Global Workspace Theory shapes the concurrency model. Complementary Learning
Systems inform memory consolidation. Metacognitive conflict monitoring becomes
a first-class subsystem with self-tuning thresholds, not a logging layer.
Drift-diffusion evidence accumulation is approximated as a bounded confidence
tracker. Cognitive load theory drives graduated context pressure response.
These are engineering implementations inspired by the theories, not formal
cognitive-science models.

The result is a runtime intended to help a language model sustain coherent,
self-correcting, goal-directed behavior across time, across interfaces, and
under pressure, while keeping the major runtime mechanisms explicit and
inspectable.

## What Cortex Is

The shortest accurate description is:

> Cortex is a long-running local agent runtime, closer to an agent OS substrate
> than a prompt loop framework.

Cortex 1.5 is a full rewrite toward a production-ready multi-user agent runtime:
the old 1.4 runtime path has been removed from active source, Git history
remains the archive, and the release path is now a slim workspace built around
mechanisms that can be tested directly:

- typed tenant, actor, client, session, turn, event, delivery, permission, and
  corpus identifiers;
- deny-by-default ownership and visibility checks;
- bounded workspace admission, salience/urgency competition, broadcast
  subscribers, and dropped-item reasons;
- fast capture and slow semantic memory consolidation with interference
  reports;
- drift-style evidence accumulation plus expected-control-value decisioning;
- turn executor that assembles workspace/retrieval/control context, wraps
  retrieved material as untrusted evidence, calls a provider, and preserves
  provider token usage;
- RAG query-scope authorization, corpus ACLs, BM25 lexical scoring, support
  scoring, placement, taint blocking, and active-retrieval decisions;
- structured outbound delivery planning with transport capabilities and a
  per-recipient delivery outbox;
- file-backed event journal replay filtered by visibility;
- SQLite state store with schema migration ledger, owner-filtered session
  queries, active-session persistence, owner-filtered memory persistence,
  permission request/resolution persistence, owner-filtered delivery outbox
  records, owner-filtered token usage ledger, and fixture-backed 1.4 session
  metadata import;
- multi-user runtime client binding, first-turn actor session reuse,
  per-client active sessions, active-session delivery gates, and journal
  recovery of those bindings;
- authenticated ingress registry that stores SHA-256 bearer-token digests and
  rejects unauthenticated client binding before runtime state changes;
- permission resolution bound to request id, owner, and private client;
- SDK plugin authorization for declared capabilities, host-path denial, and
  output limits, plus manifest ABI validation and declared-capability
  conformance;
- deployment/release state machine requiring backup, migration, install,
  smoke, package, and publish before release readiness, with evidence,
  artifact manifest, rollback actions, and rollback completion state;
- Telegram/QQ/CLI transport adapters that render `DeliveryPlan` according to
  each transport's Markdown/plain/media capability.

The goal is not to look like the literature. Cortex keeps a cognitive or RAG
term only when the corresponding mechanism exists in code and tests.

## Workspace

| Crate | Role |
| --- | --- |
| `cortex-types` | Runtime contracts: ownership, workspace, memory, retrieval, control, policy, outbound delivery, events. |
| `cortex-kernel` | Durable substrate primitives. The current slice is a file-backed journal with visibility-filtered replay. |
| `cortex-retrieval` | Ownership-filtered evidence retrieval and placement. |
| `cortex-turn` | Workspace/control/retrieval turn planning. |
| `cortex-runtime` | Multi-user runtime boundary and tenant/session gate. |
| `cortex-sdk` | Capability-first plugin context surface. |
| `cortex-app` | CLI binary entrypoint. |

## Current Status

1.5 is a released production-core baseline, not a full replacement for every
1.4 user-facing feature. The old implementation has been removed and the new
core is intentionally small so production mechanisms can be built back under
strict tests instead of hidden inside legacy modules.

Release gate command:

```bash
./scripts/gate.sh --docker
```

The gate uses `rust:latest`, the repository stable toolchain, zero warning
suppressions, `cargo fmt --all --check`, strict clippy
`-D warnings -W clippy::pedantic -W clippy::nursery`, and full workspace tests.

## Multi-User Rule

Every release-path object must carry ownership. Cross-tenant or cross-actor
access must be rejected before private state is loaded, replayed, retrieved,
delivered, or mutated.

This is tested today by:

- `crates/cortex-types/tests/mechanisms.rs`
- `crates/cortex-retrieval/tests/rag_pipeline.rs`
- `crates/cortex-kernel/tests/journal.rs`
- `crates/cortex-kernel/tests/sqlite_store.rs`
- `crates/cortex-runtime/tests/multi_user.rs`
- `crates/cortex-runtime/tests/ingress.rs`
- `crates/cortex-runtime/tests/transport.rs`
- `crates/cortex-turn/tests/executor.rs`
- `crates/cortex-sdk/tests/plugin_contract.rs`
- `crates/cortex-types/tests/deployment.rs`

## Release

Cortex 1.5.0 has been published with the SDK crate, tag, GitHub release, Linux
binary artifact, checksum, and strict Docker gate evidence. Subsequent 1.5.x
work should restore user-facing runtime features only when they sit on the new
ownership, retrieval, persistence, delivery, and gate contracts.
