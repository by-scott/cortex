# Configuration

Cortex 1.5 currently has no daemon configuration format. Runtime state is
expressed by typed contracts and persisted through the SQLite store in
`cortex-kernel`.

## Build And Gate Variables

| Variable | Purpose |
| --- | --- |
| `CORTEX_GATE_IMAGE` | Docker image name used by `scripts/gate.sh --docker` |
| `CORTEX_GATE_CARGO_VOLUME` | Named Docker volume for Cargo cache |
| `CORTEX_GATE_IN_DOCKER` | Internal flag used by the gate container |
| `CORTEX_PACKAGE_PLATFORM` | Release archive platform suffix, default `linux-amd64` |
| `CORTEX_DIST_DIR` | Release output directory, default `dist` |
| `SOURCE_DATE_EPOCH` | Reproducible archive timestamp for release packaging |

## Persistent State

The current SQLite store covers:

- tenants, clients, and active sessions;
- actor-visible sessions and least-privilege 1.4 session import;
- fast captures and semantic memories;
- permission requests and resolutions;
- per-recipient delivery outbox records;
- owner-filtered token usage records.

Every persisted object carries ownership. Cross-tenant and cross-actor access
must be rejected before private state is read or mutated.
