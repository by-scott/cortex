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

Cortex 1.5.0 is the daemon-first production-core rebuild line for that
direction. It is a deliberate rewrite of the active source tree, not an
incremental cleanup of the 1.4 daemon. The old implementation remains available
through Git history; the current tree keeps only mechanisms that are explicit,
tested, and small enough to harden directly.

## Status

1.5.0 is not a full replacement for every 1.4 user-facing feature yet. It ships
the core contracts and release discipline needed for a production-grade multi-user
runtime, and the active line is restoring live surfaces only when they sit on
those contracts.

Implemented now:

- typed tenant, actor, client, session, turn, event, delivery, permission, and
  corpus identifiers;
- deny-by-default ownership and visibility checks;
- SQLite persistence for migrations, sessions, active sessions, memory,
  permissions, delivery outbox records, side-effect records, and token usage;
- file-backed event journaling with visibility-filtered replay;
- a daemon-first Unix socket runtime with bootstrap, status, send, tenant
  registration, client binding, shutdown, journal recovery, and SQLite state
  recovery;
- RAG evidence retrieval with query-scope authorization, corpus ACLs, BM25
  lexical scoring, placement, taint blocking, and support decisions;
- turn execution that wraps retrieved material as untrusted evidence and
  preserves provider-reported token usage;
- structured outbound delivery planning for Telegram, QQ, and CLI rendering
  contracts;
- authenticated client ingress using SHA-256 bearer-token digests;
- capability-first SDK plugin contracts with ABI, declared-capability,
  host-path, and output-limit validation;
- runtime tool execution through the SDK contract, with host-granted
  capabilities, host-path denial, output-limit enforcement, and durable
  side-effect intent/result records;
- deployment planning with ordered release steps, evidence, artifacts, and
  rollback state;
- release binary installation and update through `scripts/cortex.sh`, with
  checksum verification.

Not restored in the 1.5.0 active path:

- systemd service setup and installer-managed daemon lifecycle;
- HTTP, WebSocket, JSON-RPC, MCP, ACP, Telegram, QQ, and browser live clients;
- media tools, native plugin loading, process plugin spawning, and the old
  skills registry;
- the old 1.4 prompt, memory, task, audit, channel, and orchestration modules.

Those features should return only when they use the new ownership, retrieval,
persistence, delivery, and strict-gate contracts.

## Workspace

| Crate | Role |
| --- | --- |
| `cortex-types` | Runtime contracts: ownership, workspace, memory, retrieval, control, policy, outbound delivery, events. |
| `cortex-kernel` | Durable substrate primitives: file journal, SQLite state, migrations, permissions, delivery, and usage. |
| `cortex-retrieval` | Ownership-filtered evidence retrieval and placement. |
| `cortex-turn` | Workspace/control/retrieval turn planning. |
| `cortex-runtime` | Multi-user daemon boundary, tenant/session gate, delivery, ingress, and tool execution. |
| `cortex-sdk` | Capability-first plugin context surface. |
| `cortex-app` | CLI binary entrypoint. |

## Design Rule

The goal is not to look like the literature. Cortex keeps a cognitive or RAG
term only when the corresponding mechanism exists in code and tests.

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

## Quality Gate

Release gate command:

```bash
./scripts/gate.sh --docker
```

The gate uses `rust:latest`, the repository stable toolchain, zero warning
suppressions, `cargo fmt --all --check`, strict clippy
`-D warnings -W clippy::pedantic -W clippy::nursery`, and full workspace tests.

## Release

Cortex 1.5.0 should be released only from a clean tree after the strict Docker
gate passes. The release must include the SDK crate, tag, GitHub release, Linux
binary artifact, checksum, and gate evidence. Subsequent 1.5.x work should
restore user-facing runtime features only when they sit on the new ownership,
retrieval, persistence, delivery, tool-execution, and gate contracts.
