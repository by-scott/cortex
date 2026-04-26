# Cortex

**Cortex 1.5 is a full rewrite toward a production-ready multi-user agent runtime.**

This branch deliberately removes the old 1.4 runtime path. Git history remains
the archive; the release path is now a slim workspace built around mechanisms
that can be tested directly:

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

1.5 is not release-complete yet. The old implementation has been removed and
the new core is intentionally small so the production mechanisms can be built
back under strict tests instead of hidden inside legacy modules.

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

## Release Bar

Cortex 1.5 cannot ship until final public docs, SDK publication, binary
artifact upload, tag, and GitHub release artifacts are complete.
