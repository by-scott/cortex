# Configuration

Cortex 1.5 has a small daemon bootstrap format for initial tenants and clients.
Runtime state is expressed by typed contracts and persisted through the SQLite
store in `cortex-kernel`.

## Daemon Bootstrap

```json
{
  "tenants": [
    {"id": "default", "name": "Default"}
  ],
  "clients": [
    {
      "tenant_id": "default",
      "actor_id": "local",
      "client_id": "cli",
      "max_chars": 4096
    }
  ]
}
```

Start the daemon with:

```bash
cortex daemon --data-dir /var/lib/cortex --socket /run/cortex.sock --config bootstrap.json
```

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
- side-effect intent/result records;
- owner-filtered token usage records.

Every persisted object carries ownership. Cross-tenant and cross-actor access
must be rejected before private state is read or mutated.
