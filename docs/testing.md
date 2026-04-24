# Testing

Cortex uses integration-style contract tests instead of scattered inline unit tests. The test suite is organized by crate boundary:

- `crates/cortex-types/tests/contracts.rs` checks shared data contracts, serialization, turn transitions, memory ownership, plugin manifest compatibility, and docs/runtime surface sync for published bilingual README and docs surfaces: event counts, turn-state counts, attention/metacognition/memory-recall wording, plugin-boundary wording, risk-surface guidance, and replay/compaction terminology.
- `crates/cortex-kernel/tests/persistence_replay.rs` checks SQLite-backed persistence, actor-scoped memory/task/audit visibility, embedding visibility inherited through memory ids, and replay determinism.
- `crates/cortex-runtime/tests/process_plugin.rs` checks process-isolated plugin registration, execution, command/working-dir path-boundary validation, host-path opt-in, environment inheritance, timeout/output-limit behavior, and backup-directory suppression through a shared conformance helper surface.
- `crates/cortex-runtime/src/tests/daemon_sessions.rs` checks actor-scoped session visibility, canonical-actor reuse, lazy channel session allocation, per-client active-session separation, runtime memory/task ownership under transport bindings, transport-rebind memory/task/audit ownership semantics, and seeded ownership/pairing/subscription/store sequence harnesses.
- `crates/cortex-runtime/src/tests/http_memory.rs` checks HTTP memory routes at the user-visible API surface, including transport-actor ownership on `POST /api/memory` and actor-scoped filtering on `GET /api/memory`.
- `crates/cortex-runtime/src/tests/http_sessions.rs` checks HTTP session routes at the user-visible API surface, including transport-actor ownership on `POST /api/session`, actor-scoped filtering on `GET /api/sessions`, and hidden-session rejection on `GET /api/session/{id}`.
- `crates/cortex-turn/tests/memory_tools.rs` checks actor-scoped memory tool behavior at the user-visible tool surface, including `memory_search` visibility with and without a runtime actor, `memory_save` owner assignment from the runtime actor, the `local:default` fallback owner when no actor is present, and an end-to-end actor-isolated `memory_save -> memory_search` flow.
- `crates/cortex-turn/tests/safety_contracts.rs` checks guardrail classification, risk-policy behavior, and a structured red-team corpus across web, file, plugin, and channel-shaped payloads, including advanced prompt-injection patterns, exfiltration markers, hostile structured tool-input/output cases, wrapped hostile evidence, safe corpus checks, and policy-precedence behavior.
- `crates/cortex-sdk/tests/native_abi.rs` and `crates/cortex-sdk/tests/tool_result.rs` check the stable native ABI export surface, init/null/ABI mismatch behavior, tool execution failure reporting, descriptor bounds, invalid invocation buffers, and SDK result/media DTOs through reusable ABI callback helpers.
- `crates/cortex-app/tests/cli_scaffold.rs` and `crates/cortex-app/tests/plugin_manager.rs` check the plugin scaffold CLI, local install filtering, and `.cpx`/directory install behavior.

Required local gate:

```bash
docker compose run --rm dev cargo fmt --check
docker compose run --rm dev cargo test --workspace
docker compose run --rm dev cargo clippy --workspace --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery
```

Warnings are build failures. Warning suppression attributes are not used in the codebase.
