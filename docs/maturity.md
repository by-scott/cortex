# Maturity

Cortex 1.5 is a rewrite line, not a mature platform claim. It intentionally
removes the old runtime path and rebuilds only mechanisms that can be tested
under the strict gate.

## Implemented

- Multi-user ownership and visibility contracts.
- Workspace admission and competition.
- Hierarchical control goals with active conflict detection and top-down bias.
- Cognitive-load profiles for intrinsic, extraneous, germane, and temporal
  pressure.
- Metacognitive monitoring for goal conflict, load pressure, feedback conflict,
  frame anchoring, and calibration drift.
- Memory capture/consolidation records with interference reporting.
- RAG authorization, ACLs, BM25 scoring, taint blocking, and placement.
- Turn planning and model-provider usage preservation.
- SQLite persistence for sessions, memory, permissions, deliveries,
  side-effect ledgers, cognitive control records, and token usage.
- Daemon lifecycle with Unix socket RPC, bootstrap, status, send, tenant
  registration, client binding, shutdown, journal recovery, and SQLite state
  recovery.
- Authenticated ingress with SHA-256 bearer-token digests.
- SDK plugin manifest, ABI, capability, host-path, and output-limit checks.
- Runtime tool execution with SDK validation, host-granted capabilities,
  output limits, and side-effect intent/result records.
- Deployment plan ordering, evidence, artifact manifests, rollback actions, and
  rollback completion state.
- Reproducible release packaging script and strict Docker gate.

## Not Yet

- Systemd service management and installer-managed daemon lifecycle.
- HTTP/WebSocket APIs.
- Telegram/QQ live clients.
- Browser integration.
- Media tools.
- Process plugin spawning.
- Native plugin loading.
- Hostile multi-tenant OS isolation.

The release is moving toward production readiness by reducing undocumented
surface, not by preserving old code paths.
