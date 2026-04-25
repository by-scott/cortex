# Changelog

## Unreleased

## 1.3.0 - 2026-04-25

### Ownership, Continuity, and Actor Isolation

- Hardened actor-scoped session continuity across `rpc`, `http`, `ws`, `socket`, `stdio`, Telegram, and QQ entry surfaces with seeded ownership-sequence coverage instead of single happy-path regressions.
- Extended actor ownership checks from sessions into memory, task, audit, and embedding visibility so transport rebinding and alias rewrites do not relabel older data or leak it across actors.
- Added explicit user-surface tests for actor-scoped memory routes, session routes, turn dispatch, and hidden-session rejection on `http`, `rpc`, `ws`, `socket`, and `stdio`.
- Pinned lazy channel-session allocation and per-client subscription semantics so pairing no longer allocates a session eagerly and subscriptions follow only the paired client's own active session.
- Added contract coverage for slash-command stop dispatch across wrapper surfaces so `/stop` resolves only against visible active turns instead of drifting by transport.

### Guardrails, Replay, and Runtime Trust

- Extended hostile-input coverage from baseline pattern detection into structured red-team corpora covering web, file, plugin, channel, wrapped-evidence, and fragmented nested payload cases.
- Hardened tool-output guardrails so advanced injection detection is applied to returned tool output rather than only to direct user/tool-input classification.
- Added runtime observability tests for hostile tool output, pinning `ExternalInputObserved`, `GuardrailTriggered`, and untrusted tool-result history wrapping as operator-visible behavior rather than internal implementation detail.
- Extended replay coverage so deterministic side-effect substitution, guardrail/external-input events, and replay digests remain stable across reopen and projection.
- Added replay substitution regression tests that verify provider-supplied side-effect values override inline recorded payloads during projection and are reflected in replay digests.

### Plugin Contracts and Compatibility

- Expanded process-plugin conformance coverage for manifest validation, compatibility rejection, compatible native-manifest probing, path/working-directory boundaries, host-path opt-in, environment inheritance, timeout/output limits, and backup-directory suppression.
- Added runtime compatibility checks that reject incompatible `cortex_version` or native `abi_version` values before probing libraries and accept compatible manifests through the current load path.
- Pinned the stable native loader's callback-table contract so missing `plugin_info`, `tool_count`, `tool_descriptor`, `tool_execute`, `plugin_drop`, or `buffer_free` entries are rejected explicitly.
- Extended native ABI coverage from SDK export helpers into runtime loader behavior, including malformed callback tables and compatibility-gated native manifests.

### Runtime Wrapper and Operator Surface

- Brought capability enumeration and operator-only boundaries into parity across `rpc`, `http`, `ws`, `socket`, and `stdio` wrappers for `session/initialize`, `mcp/tools-list`, `mcp/prompts-list/get`, `skill/list`, `skill/invoke`, and `skill/suggestions`.
- Added positive reload-path coverage across transport wrappers so `admin/reload-config` is validated as a real success path, not only a rejection path.
- Kept local-operator-only introspection and admin methods (`daemon/status`, `health/check`, `admin/reload-config`, audit/introspection tools) pinned at the wrapper boundary instead of relying only on shared lower layers.

### Upgrade, Compatibility, and Documentation Contracts

- Added contract coverage for prompt migration compatibility, including legacy root-template moves into `prompts/system/`, `agent.md -> behavioral.md`, and preservation of existing `behavioral.md`.
- Added compatibility-policy coverage for replay semantics, plugin boundaries, permission modes, and upgrade expectations (`restart`, `reinstall`, `plugin rebuild`) across English and Chinese docs.
- Expanded docs/runtime sync tests so README, README.zh, executive, usage, maturity, compatibility, roadmap, and testing docs stay aligned with shipped replay, compaction, hostile-output, plugin-boundary, and permission surfaces.
- Updated published examples, plugin manifests, SDK examples, scaffolded plugin templates, and versioned install examples to `1.3.0`.

### Validation

- Kept the workspace clean under:
  - `docker compose run --rm dev cargo fmt --check`
  - `docker compose run --rm dev cargo test --workspace`
  - `docker compose run --rm dev cargo clippy --workspace --all-targets -- -D warnings -W clippy::pedantic -W clippy::nursery`

## 1.2.0 - 2026-04-24

### Architecture and Plugin Runtime

- Replaced the old Rust trait-object plugin loading path with a stable native plugin ABI centered on `cortex_plugin_init`.
- Clarified the plugin boundary split: process-isolated plugins remain the default external boundary, while trusted native plugins use the new stable ABI.
- Updated manifest handling, runtime loading, SDK surface, and plugin documentation to match the new ABI model.
- Added stricter plugin installation filtering for local directories and `.cpx` packages so only supported plugin assets are installed.
- Local directory plugin installs now extract built native libraries into `lib/` automatically.
- Backup and hidden plugin directories are now ignored by plugin listing and plugin loading.

### Security, Risk, and Trust Boundaries

- Unknown plugin and MCP tools now default to conservative risk scoring and require confirmation unless an explicit `[risk.tools.<name>]` policy lowers the risk.
- Added configurable per-tool risk policy overrides for `tool_risk`, `file_sensitivity`, `blast_radius`, `irreversibility`, `require_confirmation`, `block`, and `allow_background`.
- Added `risk.allow` and `risk.deny` tool-name patterns. Deny patterns and allowlist misses block matching tools before execution.
- `Review` risk decisions now require confirmation instead of being approved automatically by the default and interactive gates.
- Added `[risk].auto_approve_up_to`; the default install and default runtime mode are now `balanced` (`Review`).
- Added install-time and runtime permission mode management through `--permission-level`, `CORTEX_PERMISSION_LEVEL`, `cortex permission ...`, and `/permission ...`.
- Pending tool confirmations are emitted through both the session broadcast bus and active turn streams so synchronous channel replies and streaming transports render the same confirmation state.
- Channel users can resolve pending confirmations with `/approve <id>` or `/deny <id>`, and stopping a turn now clears any pending confirmations for that turn immediately.
- Interactive permission waits no longer auto-deny while waiting for a user response; confirmation now remains pending until approve, deny, or stop.
- `/stop` now resolves against the active actor session, clears pending confirmations for that turn, and returns a normal cancellation result instead of surfacing an empty-response error.
- Background tool execution now requires either tool-declared `background_safe` capability or explicit `[risk.tools.<name>].allow_background = true`.
- Guardrail findings are now structured by category: prompt injection, system-prompt leakage, role override, and exfiltration.
- Guardrail hits now emit a structured `GuardrailTriggered` journal event in addition to the emergency attention event.
- Added `SourceTrust`/`SourceProvenance` types and `ExternalInputObserved` journal events; successful tool outputs are wrapped as untrusted evidence before entering LLM history.

### Replay, State, and Actor Isolation

- Fixed deterministic replay side-effect substitution so provider-supplied values are projected instead of the originally recorded value.
- Added journal-backed replay coverage for recorded side effects loaded from SQLite and substituted through a provider.
- External I/O side-effect keys now include turn id and tool-call id instead of only tool name, avoiding collisions between repeated calls.
- Added `replay_determinism_digest` to compare equivalent replay projections after substitution while excluding event ids and timestamps.
- Long-term memories now carry `owner_actor`; memory save/search tools scope saved and recalled memories by runtime actor while preserving `local:default` as the local administrator.
- Memory store APIs now enforce actor-scoped list/load/delete operations for non-admin actors instead of relying only on caller-side filtering.
- Session, task, and audit stores now expose actor-scoped list/load/history/delete/claim/query APIs for non-admin callers; embedding vectors inherit ownership through memory ids.
- Actor runtime storage and process-plugin policy handling were hardened to reduce cross-actor leakage and inconsistent access paths.

### Channel Runtime and Live Reload

- Browser, plugin, and channel subscription changes now hot-apply without requiring a daemon restart in normal user-service operation.
- Telegram subscription watchers now reconcile dynamically as paired-user subscribe state changes.
- Added `cortex browser disable`, `cortex plugin enable`, and `cortex plugin disable`.
- Added per-instance plugin enable/disable handling that respects `--home` and `--id` consistently.
- `install`, `start`, and `restart` now wait for daemon readiness before returning.
- Fixed launcher refresh so installed user binaries do not become self-referential symlinks.
- `cortex install` now refreshes the user launcher path consistently so CLI and systemd do not drift onto different binaries.

### Telegram and QQ Interaction UX

- Telegram and QQ channel commands now favor card-style interaction for `/help`, `/status`, `/permission`, `/session`, and `/config` where supported.
- Permission cards now refresh state instead of continually spawning new cards, and current-mode buttons render consistently.
- Session switch cards now exclude the current session and only show sessions visible to the current actor.
- Channel-side session listing now respects actor visibility instead of leaking sessions through the generic command path.
- Paired channel users no longer allocate sessions at approval time. The first real message after pairing now reuses an existing visible session for the same canonical actor when available, otherwise it creates a new session lazily.
- Channel subscriptions now follow the paired client's own active session instead of mirroring unrelated sessions owned by the same canonical actor.
- Telegram text messages are no longer serialized behind long-running turn execution, so `/stop` and follow-up messages can arrive while a turn is active.
- Telegram cancellation now returns a normal cancellation result instead of surfacing `turn completed without a user-visible assistant response`.
- Telegram final-text handling now avoids overwriting a longer streamed buffer with a shorter final response.
- Telegram polling and outbound API traffic now use separate HTTP clients, and the polling client is rebuilt after poll failures to improve recovery after callback/edit traffic.
- Telegram outbound `sendMessage` / `editMessageText` calls now use bounded request timeouts so a stuck finalize/edit path cannot leave a truncated draft bubble in place indefinitely, and streamed draft bubbles now stay plain text while final responses return through the HTML-rendered path.
- QQ reply targeting now falls back across `msg_id`, `message_id`, `id`, and `event_id`, improving passive replies and reducing third-party send failures.
- QQ now supports interaction-driven navigation and permission actions instead of remaining text-only.

### Prompting and Runtime Context

- Added a dedicated runtime policy section to system-prompt assembly so current permission mode is injected as runtime context instead of being baked into static prompt files.
- `behavioral.md` remains a static operating-protocol asset; live permission facts are now injected separately at turn assembly time.

### CLI, Status, and Operator Experience

- `cortex status` now includes permission mode and cumulative LLM token totals.
- Status and interactive command output gained clearer emoji-backed summaries for channel-facing UX.
- Added and documented the `cortex permission` command and the recommended `strict` / `balanced` / `open` operational modes.
- Quickstart and usage documentation now recommend `balanced` as the default mode and explain how to change modes later.
- `cortex install` and related CLI help text were updated to describe install-time permission mode selection.

### Testing and Quality Gates

- Rebuilt large parts of the test suite as strict contract and integration tests, including runtime/plugin coverage and plugin install filtering.
- Removed bare `unwrap` / `expect` usage in the touched paths and kept strict warning policy clean under `clippy::pedantic` and `clippy::nursery`.
- Reorganized `cortex-app` into a `lib + bin` layout so internal tests no longer live inside implementation modules.
- Added focused regression coverage for launcher refresh, plugin path handling, backup plugin suppression, and service/home behavior.
- Release validation now includes strict `fmt`, `test`, and `clippy` gates plus repeated live installation and channel-path verification.

### Documentation

- Added maturity and production notes in English and Chinese.
- Clarified that Cortex uses cognitive-science-inspired engineering approximations, not formal cognitive-science implementations.
- Added explicit threat-model and “not yet” notes for production hardening.
- Updated SDK docs to explain the trusted native ABI boundary, install flow, and `.cpx` packaging expectations.
