# Compatibility Policy

Cortex 1.5 is a full rewrite. The compatibility surface is the code and tests
that exist in this release line, not the older 1.4 daemon surface.

## Stable For 1.5

- Typed tenant, actor, client, session, turn, event, delivery, permission, and
  corpus identifiers.
- `OwnedScope` visibility rules and deny-by-default cross-owner checks.
- File journal replay filtered by visibility.
- SQLite migrations for tenants, clients, sessions, memory, permission
  requests, delivery outbox records, and token usage records.
- RAG query-scope authorization, corpus ACLs, BM25 scoring, taint blocking, and
  placement.
- SDK plugin manifests with ABI version checks and declared-capability
  conformance.
- Release gate behavior in `scripts/gate.sh` and asset naming in
  `scripts/package-release.sh`.

## Versioned Surface

- `cortex-sdk` ABI version.
- SQLite schema migration numbers.
- Public DTOs in `cortex-types`.
- Release archive name: `cortex-v${VERSION}-${PLATFORM}.tar.gz`.

Changes to these surfaces need tests and release notes.

## Unstable Surface

Live daemon management, HTTP APIs, channel pairing, browser integration, and
native plugin loading are not part of the 1.5 rewrite surface yet. They should
not be documented as available until they are rebuilt under the strict gate.
