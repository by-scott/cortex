# Testing

Cortex uses integration-style contract tests instead of scattered inline unit tests. The test suite is organized by crate boundary:

- `crates/cortex-types/tests/contracts.rs` checks shared data contracts, serialization, turn transitions, memory ownership, and plugin manifest compatibility.
- `crates/cortex-kernel/tests/persistence_replay.rs` checks SQLite-backed persistence, actor-scoped memory visibility, and replay determinism.
- `crates/cortex-runtime/tests/process_plugin.rs` checks process-isolated plugin registration, execution, and backup-directory suppression.
- `crates/cortex-turn/tests/safety_contracts.rs` checks guardrail classification and risk-policy behavior.
- `crates/cortex-sdk/tests/native_abi.rs` and `crates/cortex-sdk/tests/tool_result.rs` check the stable native ABI export surface and SDK result/media DTOs.
- `crates/cortex-app/tests/cli_scaffold.rs` and `crates/cortex-app/tests/plugin_manager.rs` check the plugin scaffold CLI, local install filtering, and `.cpx`/directory install behavior.

Required local gate:

```bash
cargo fmt --check
docker compose run --rm dev cargo test --workspace
docker compose run --rm dev cargo clippy --workspace --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery
```

Warnings are build failures. Warning suppression attributes are not used in the codebase.
