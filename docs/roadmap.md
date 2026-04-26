# Roadmap

The 1.5 line is a full rewrite focused on production readiness, multi-user
ownership, and removal of historical drift.

## Completed In The Rewrite Core

- Delete-first workspace replacement.
- Ownership, visibility, and authenticated ingress.
- Journal replay and SQLite state recovery.
- RAG evidence as a separate runtime object from memory.
- Provider token usage contract and usage ledger.
- SDK manifest/ABI/capability conformance.
- Daemon lifecycle with Unix socket RPC, bootstrap, status, send, tenant
  registration, client binding, shutdown, journal recovery, and SQLite state
  recovery.
- Runtime tool execution with SDK validation, host-granted capabilities,
  output limits, and durable side-effect intent/result records.
- Delivery planning, transport rendering, and per-recipient outbox records.
- Deployment evidence, artifacts, rollback actions, and rollback completion.
- Release packaging script.

## Remaining Before Release

- Complete remaining production daemon operations that belong in 1.5, especially
  service installation evidence and release smoke tests.
- Final public docs review.
- SDK publish.
- Git tag.
- GitHub release with binary and checksum assets.

## After 1.5

Rebuild HTTP APIs, live channels, media tools, process plugin spawning, and
native plugin loading only after each boundary has tests that enforce
ownership, authorization, recovery, and failure behavior.
