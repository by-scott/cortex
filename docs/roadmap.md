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
- Delivery planning, transport rendering, and per-recipient outbox records.
- Deployment evidence, artifacts, rollback actions, and rollback completion.
- Release packaging script.

## Remaining Before Release

- Final public docs review.
- Official Docker gate once Docker Hub metadata is reachable.
- SDK publish.
- Git tag.
- GitHub release with binary and checksum assets.

## After 1.5

Rebuild daemon lifecycle, HTTP APIs, live channels, tool execution, and native
plugin loading only after each boundary has tests that enforce ownership,
authorization, recovery, and failure behavior.
