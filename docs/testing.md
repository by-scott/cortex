# Testing

Cortex uses integration-style contract tests instead of scattered inline unit tests. The test suite is organized by crate boundary:

- `crates/cortex-types/tests/contracts.rs` checks shared data contracts, serialization, turn transitions, memory ownership, feedback attribution and replay checks, workspace admission and contamination barriers, media attachment governance defaults, plugin manifest compatibility, sandbox-enforcement claim rejection, and docs/runtime surface sync for published bilingual README and docs surfaces: event counts, turn-state counts, attention/metacognition/memory-recall wording, plugin-boundary wording, risk-surface guidance, and replay/compaction terminology.
- `crates/cortex-types/tests/model_routing.rs` checks the model capability routing contract, including low-cost JSON-capable extraction routing, high-risk low-confidence escalation to safer reasoning models, provider-failure fallback, schema-invalid fallback, rejected targets, and route explanations.
- `crates/cortex-retrieval/tests/rag_pipeline.rs` checks the independent RAG evidence pipeline, including deterministic indexing, exact lexical retrieval, dense paraphrase retrieval, late-interaction reranking, learned-sparse expansion, HyDE-style query transformations that cannot become evidence, actor-private document isolation, tainted retrieved instructions, citation keys, license propagation, evidence-role propagation, answer-claim support verification, negative evidence overriding stale support, retrieval evaluation metrics, active retrieval control for absent or low-support evidence, and workspace promotion under actor/budget guards.
- `crates/cortex-kernel/tests/persistence_replay.rs` checks SQLite-backed persistence, actor-scoped memory/task/audit visibility, embedding visibility inherited through memory ids, replay side-effect substitution, legacy empty-`execution_version` replay compatibility, externalized `ContextCompactBoundary` replay compatibility, replay projection versions, replay diffs, causal audit graph edges, a replay migration fixture corpus under `crates/cortex-kernel/tests/fixtures/replay/`, and replay determinism.
- `crates/cortex-kernel/src/policy.rs` unit tests check policy-as-code lint and simulation contracts, including open permission mode with unreviewed plugins, native/process plugins without explicit risk profiles, effect-floor simulation, and background execution denial.
- `crates/cortex-kernel/tests/prompt_manager.rs` checks prompt migration compatibility and prompt linting, including legacy root-template moves into `prompts/system/`, `agent.md -> behavioral.md` migration, non-overwrite behavior when a current `behavioral.md` already exists, checked-update rejection for runtime policy overrides, absent capability references, and unapproved self-edit diffs.
- `crates/cortex-kernel/tests/config_loader.rs` checks config migration compatibility, including cleanup of legacy `data/defaults.toml`, regeneration of the current `config.defaults.toml` reference during config load, legacy `actors.toml` files that omit either the `aliases` or `transports` section, and invalid legacy `client_sessions.json` / `actor_sessions.json` files defaulting to empty maps.
- `crates/cortex-kernel/tests/memory_store_compat.rs` checks memory-file migration compatibility, including loading legacy UUID-named memory files and removing those legacy filenames after the memory is re-saved under the current naming scheme.
- `crates/cortex-kernel/tests/session_store_compat.rs` checks session-store compatibility, including invalid legacy session metadata defaulting to `None` and invalid legacy MsgPack session history defaulting to an empty message list.
- `crates/cortex-kernel/tests/task_audit_compat.rs` checks task/audit store compatibility, including legacy `shared_tasks` / `audit_entries` schemas that omit `owner_actor` defaulting reopened rows to `local:default` without leaking across actor-scoped queries.
- `crates/cortex-runtime/tests/process_plugin.rs` checks process-isolated plugin registration, manifest and native-ABI compatibility rejection, compatible native-manifest library probing, execution, stderr/non-zero-exit propagation, invalid JSON output rejection, command/working-dir path-boundary validation, host-path opt-in, environment inheritance, timeout/output-limit behavior, backup-directory suppression, governance rejection for unsafe secret access, and manifest-declared capability/effect propagation through a shared conformance helper surface.
- `crates/cortex-runtime/src/plugin_loader.rs` unit tests check the stable native loader's callback-table validation, including rejection of missing `plugin_info`, `tool_count`, `tool_descriptor`, `tool_execute`, `plugin_drop`, and `buffer_free` entries before a native handle is accepted.
- `crates/cortex-runtime/src/tests/channel_store.rs` checks legacy channel-store compatibility, including paired users without a `subscribe` field, legacy `policy.json` files that omit optional lists and limits, and empty `update_offset.json` state defaulting to zero.
- `crates/cortex-runtime/src/tests/control.rs` checks runtime turn-control contracts, including actor-scoped `session/cancel`, hidden-session rejection on cancel, admin fallback to the global active turn, denial/removal of pending permissions when the target session is cancelled, and decision-trace rendering in pending permission prompts.
- `crates/cortex-runtime/src/tests/daemon_sessions.rs` checks actor-scoped session visibility, canonical-actor reuse, lazy channel session allocation, per-client active-session separation, runtime memory/task ownership under transport bindings, `ws`/`sock`/`stdio` transport continuity, transport-rebind memory/task/audit ownership semantics, seeded ownership/pairing/subscription/store sequence harnesses, and multi-seed end-to-end ownership sequences.
- `crates/cortex-runtime/src/tests/http_memory.rs` checks HTTP memory routes at the user-visible API surface, including transport-actor ownership on `POST /api/memory` and actor-scoped filtering on `GET /api/memory`.
- `crates/cortex-runtime/src/tests/http_audit.rs` checks HTTP audit routes at the operator surface, including local-operator access to `/api/audit/*` and rejection for non-local transport actors.
- `crates/cortex-runtime/src/introspect_tools.rs` unit tests check the self-introspection tool surface (`audit`, `prompt_inspect`, `memory_graph`), including hiding these tools from non-local actor tool schemas and enforcing local-operator-only access under runtime invocation context.
- `crates/cortex-runtime/src/tests/http_operator.rs` checks HTTP operator routes at the process-level surface, including local-operator access to `/api/daemon/status`, `/api/operator/dashboard`, `/api/health`, and `/api/metrics/structured`, normalized dashboard timeline content, and rejection for non-local transport actors.
- `crates/cortex-runtime/src/tests/http_meta.rs` checks HTTP meta routes at the user-visible API surface, including hidden-session rejection on `GET /api/meta/alerts`.
- `crates/cortex-runtime/src/tests/http_rpc.rs` checks the HTTP JSON-RPC wrapper at the user-visible API surface, including transport-actor ownership on `session/new`, actor-scoped `session/list` / `session/get` / `session/end` visibility, visible-session reuse plus hidden-session rejection on `session/prompt`, live `session/cancel` on a visible active turn, hidden-session rejection on `session/cancel`, live `/stop` through `command/dispatch`, transport-actor ownership on `memory/save`, actor-scoped `memory/list` visibility, actor-scoped `memory/get` / `memory/delete` visibility, actor-scoped `memory/search` visibility, `session/initialize` tool visibility, `mcp/tools-list` tool visibility, `skill/list`/`skill/invoke`/`skill/suggestions` visibility, `mcp/prompts-list`/`mcp/prompts-get` user-invocable visibility, visible-session success plus hidden-session rejection on `meta/alerts` and `command/dispatch`, local-operator enforcement on `daemon/status`, `operator/dashboard`, `admin/reload-config`, and `health/check`, mixed-result batch handling, notification suppression, empty-batch rejection, and unsupported content-type rejection through `POST /api/rpc`.
- `crates/cortex-runtime/src/tests/http_sessions.rs` checks HTTP session routes at the user-visible API surface, including transport-actor ownership on `POST /api/session`, actor-scoped filtering on `GET /api/sessions`, hidden-session rejection on `GET /api/session/{id}`, and `/api/turn` plus `/api/turn/stream` session resolution for accessible versus inaccessible HTTP sessions.
- `crates/cortex-runtime/src/tests/line_protocol.rs` checks the shared socket/stdin line-protocol surface, including transport-scoped sync RPC visibility, transport-actor ownership on `session/new` for both `socket` and `stdio`, actor-scoped `session/list` / `session/get` / `session/end` visibility for both `socket` and `stdio`, live `session/cancel` on visible active turns for both `socket` and `stdio`, live `/stop` through `command/dispatch` for both `socket` and `stdio`, hidden-session rejection on `session/cancel`, transport-actor ownership on `memory/save` for both `socket` and `stdio`, actor-scoped `memory/get` / `memory/delete` visibility for both `socket` and `stdio`, actor-scoped `memory/search` visibility for both `socket` and `stdio`, `session/initialize` tool visibility, `mcp/tools-list` tool visibility, `skill/list`/`skill/invoke`/`skill/suggestions` visibility, `mcp/prompts-list`/`mcp/prompts-get` user-invocable visibility, local-operator enforcement on `daemon/status`, `admin/reload-config`, and `health/check`, batch handling, visible-session success plus hidden-session rejection on `session/prompt`, `meta/alerts`, and `command/dispatch`, and prompt execution reuse of the active `socket` or `stdio` actor session when no explicit `session_id` is provided.
- `crates/cortex-runtime/src/tests/ws_rpc.rs` checks the WebSocket JSON-RPC surface, including transport-actor ownership on `session/new`, actor-scoped `session/list` / `session/get` / `session/end` visibility, live `session/cancel` on a visible active turn, hidden-session rejection on `session/cancel`, live `/stop` through `command/dispatch`, transport-actor ownership on `memory/save`, actor-scoped `memory/list` visibility, actor-scoped `memory/get` / `memory/delete` visibility, actor-scoped `memory/search` visibility, `session/initialize` tool visibility, `mcp/tools-list` tool visibility, `skill/list`/`skill/invoke`/`skill/suggestions` visibility, `mcp/prompts-list`/`mcp/prompts-get` user-invocable visibility, local-operator enforcement on `daemon/status`, `admin/reload-config`, and `health/check`, visible-session success plus hidden-session rejection on `session/prompt`, `meta/alerts`, and `command/dispatch`, and prompt execution reuse of the active `ws` actor session when no explicit `session_id` is provided.
- `crates/cortex-runtime/src/tests/rpc_batch.rs` checks the shared batch JSON-RPC contract used by HTTP, socket, and stdio transports, including empty-batch rejection and notification-only batch suppression.
- `crates/cortex-runtime/src/tests/rpc_memory.rs` checks RPC memory routes at the user-visible API surface, including transport-actor ownership on `memory/save`, actor-scoped filtering on `memory/list` and `memory/search`, and hidden-memory rejection on `memory/get` and `memory/delete`.
- `crates/cortex-runtime/src/tests/rpc_sessions.rs` checks RPC session routes at the user-visible API surface, including transport-actor ownership on `session/new`, actor-scoped filtering on `session/list`, `session/get`, `session/end`, live `session/cancel` on a visible active turn, hidden-session rejection on `session/cancel`, live `/stop` through `command/dispatch`, local-operator enforcement on `operator/dashboard`, `session/initialize` tool visibility, `mcp/tools-list` tool visibility, and `skill/list`/`skill/invoke`/`skill/suggestions`/`mcp/prompts-list`/`mcp/prompts-get` user-invocable visibility, visible-session success plus hidden-session rejection on `session/prompt`, `meta/alerts`, and `command/dispatch`, and session reuse on prompt execution without an explicit `session_id`.
- `crates/cortex-runtime/src/tests/retrieval_context.rs` checks the runtime-facing RAG fixture: generated retrieval evidence is formatted into the dedicated evidence context plane after situational context and before recalled memory, preserving citation and license metadata.
- `crates/cortex-turn/tests/memory_tools.rs` checks actor-scoped memory tool behavior at the user-visible tool surface, including `memory_search` visibility with and without a runtime actor, `memory_save` owner assignment from the runtime actor, the `local:default` fallback owner when no actor is present, and an end-to-end actor-isolated `memory_save -> memory_search` flow.
- `crates/cortex-turn/src/meta/adaptive.rs` checks metacognitive outcome calibration, including rich alert feedback, intervention success accounting, confidence-delta tracking, and adaptive threshold tightening/relaxing.
- `crates/cortex-turn/src/attention/mod.rs` unit tests check scheduler resource governance, including actor budgets, skipped-task explanations, emergency debounce, and maintenance-debt carryover.
- `crates/cortex-turn/src/agent_pool/delegation.rs` unit tests check delegation contracts, including non-local introspection filtering, explicit allowed-tool filtering, forbidden-action blocking, token/iteration budgets, allowed-evidence declarations, and rejection of broad parent-authority inheritance.
- `crates/cortex-turn/src/orchestrator/tpn.rs` unit tests check tool-permission control decisions, including selected action, candidate actions, rejected alternatives, risk boundary, blocking uncertainty, and the difference between auto-approved and confirmation-required tool calls.
- `crates/cortex-turn/src/skills/mod.rs` checks Repertoire contracts, including generated skill manifests with effects/risk/observability and bounded execution-trace capture.
- `crates/cortex-turn/src/context/builder.rs` and `crates/cortex-turn/src/context/evidence.rs` unit tests check context layer ordering and evidence formatting, including citation keys, taint labeling, and the rule that retrieved text is not executable prompt content.
- `crates/cortex-turn/tests/safety_contracts.rs` checks guardrail classification, risk-policy behavior, secret-handle dataflow policy, and a structured red-team corpus across web, file, plugin, and channel-shaped payloads, including advanced prompt-injection patterns, exfiltration markers, hostile structured tool-input/output cases, wrapped hostile evidence, channel callback/plugin stderr wrappers, safe corpus checks, and policy-precedence behavior.
- `crates/cortex-turn/src/tests/orchestrator_guardrails.rs` checks the runtime observability path for hostile tool output, including `ExternalInputObserved`, `GuardrailTriggered`, and untrusted tool-result history wrapping for tool output that must stay operator-visible and auditable.
- `crates/cortex-sdk/tests/native_abi.rs` and `crates/cortex-sdk/tests/tool_result.rs` check the stable native ABI export surface, init/null/ABI mismatch behavior, tool execution failure reporting, descriptor bounds, invalid invocation buffers, and SDK result/media DTOs through reusable ABI callback helpers.
- `crates/cortex-app/tests/cli_scaffold.rs`, `crates/cortex-app/tests/plugin_manager.rs`, and `crates/cortex-app/src/tests/deploy.rs` check the plugin scaffold CLI, local install filtering, `.cpx`/directory install behavior, signed package install, unsigned package rejection, unknown-publisher rejection, tampered payload rejection, `cortex plugin review`, `cortex plugin test` conformance failures, `cortex policy lint` failures, and `cortex policy simulate` argument handling.

Required gate:

```bash
./scripts/gate.sh --docker
```

Use Docker Compose through the repository entrypoints. This command runs the
`dev` service from this repository's `docker-compose.yml`, built from the
repository `Dockerfile` on `rust:latest`; that repository Docker Compose environment is the
release authority. Host `cargo` commands are diagnostic shortcuts only and do
not replace the repository Docker Compose gate.

Release verification should add `--require-clean` after the release commit is
created:

```bash
./scripts/gate.sh --docker --require-clean
```

The gate runs suppression checks, formatting, docs/package drift, secret scans,
strict clippy, and full workspace tests. Warnings are build failures. Warning
suppression attributes and compiler warning-suppression flags are not used in
the codebase.

Release candidates also generate a behavior evidence report:

```bash
docker compose run --rm dev ./scripts/release-behavior-report.sh --run
docker compose run --rm dev ./scripts/soak-fault-harness.sh --run
```

The report records the memory, retrieval/RAG, tool, safety, operator timeline,
long-task recovery, replay, and soak evidence surfaces that support release
claims. The bounded soak/fault harness records provider, channel, SQLite,
plugin, disk/config, rate-limit/backpressure, replay-after-upgrade, and
reconnect evidence. Both reports are attached to the release review; neither
replaces the strict Docker gate.

Manual Docker Compose equivalents for debugging individual failures inside the
same repository `dev` service:

```bash
docker compose run --rm dev cargo fmt --all --check
docker compose run --rm dev cargo test --workspace --all-features
docker compose run --rm dev cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
```
