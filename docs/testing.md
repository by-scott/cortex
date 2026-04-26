# Testing

The Cortex 1.5 release gate is Docker-authoritative:

```bash
./scripts/gate.sh --docker
```

The gate runs:

- suppression scan: no `#[allow(...)]`, `#![allow(...)]`, `#[expect(...)]`, or
  `cfg_attr(..., allow/expect)` warning suppression attributes;
- `cargo fmt --all --check`;
- docs/package/secret checks;
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery`;
- `cargo test --workspace --all-features`.

The same gate can be run inside an already built gate image when Docker Hub is
temporarily unreachable:

```bash
docker run --rm -e CORTEX_GATE_IN_DOCKER=1 \
  -v cortex-gate-cargo:/home/dev/.cargo \
  -v "$PWD":/workspace -w /workspace \
  cortex-gate:latest ./scripts/gate.sh --host
```

Release assets are created by:

```bash
./scripts/package-release.sh
```

The script builds the `cortex` release binary, writes
`dist/cortex-v${VERSION}-${PLATFORM}.tar.gz`, and writes the matching
`.sha256` checksum. `scripts/check-package-surface.sh` keeps this asset name in
sync with the installer.

Current mechanism tests:

- `crates/cortex-types/tests/mechanisms.rs`
- `crates/cortex-types/tests/deployment.rs`
- `crates/cortex-retrieval/tests/rag_pipeline.rs`
- `crates/cortex-kernel/tests/journal.rs`
- `crates/cortex-kernel/tests/sqlite_store.rs`
- `crates/cortex-runtime/tests/ingress.rs`
- `crates/cortex-runtime/tests/multi_user.rs`
- `crates/cortex-runtime/tests/transport.rs`
- `crates/cortex-turn/tests/executor.rs`
- `crates/cortex-sdk/tests/plugin_contract.rs`

Any future subsystem that touches ownership, memory, retrieval, tools,
permissions, delivery, replay, migration, authenticated ingress, or persistence
must add tests that prove cross-tenant and cross-actor access is denied before
private state is loaded or mutated.

`sqlite_store.rs` is the persistence contract for tenant/client/session state,
legacy session import, memory persistence, active-session recovery, permission
request/resolution ownership, per-recipient delivery outbox records, and
owner-filtered token usage accounting. Migration samples are generated inside
the test so repository root fixtures do not become part of the public surface.

`deployment.rs` is the release contract for ordered steps, evidence records,
artifact manifests, rollback actions after a failed step, and rollback
completion state.
