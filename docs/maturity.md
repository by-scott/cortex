# Maturity

Cortex 1.5 is a rewrite line, not a mature platform claim. It intentionally
removes the old runtime path and rebuilds only mechanisms that can be tested
under the strict gate.

## Implemented

- Multi-user ownership and visibility contracts.
- Workspace admission and competition.
- Memory capture/consolidation records with interference reporting.
- RAG authorization, ACLs, BM25 scoring, taint blocking, and placement.
- Turn planning and model-provider usage preservation.
- SQLite persistence for sessions, memory, permissions, deliveries, and token
  usage.
- Authenticated ingress with SHA-256 bearer-token digests.
- SDK plugin manifest, ABI, capability, host-path, and output-limit checks.
- Deployment plan ordering, evidence, artifact manifests, rollback actions, and
  rollback completion state.
- Reproducible release packaging script and strict Docker gate.

## Not Yet

- Live daemon lifecycle.
- HTTP/WebSocket APIs.
- Telegram/QQ live clients.
- Browser integration.
- Tool execution engine.
- Native plugin loading.
- Hostile multi-tenant OS isolation.

The release is moving toward production readiness by reducing undocumented
surface, not by preserving old code paths.
