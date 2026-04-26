# Changelog

## Unreleased

## 1.5.0 - 2026-04-26

### Full Rewrite Scope

- Removed the old 1.4 runtime implementation from the active source path and
  rebuilt the release workspace around a smaller set of directly testable
  production mechanisms.
- Kept Git history as the archive of the previous implementation while making
  the 1.5 source tree delete-first: stale daemon, channel, tool-execution,
  plugin-loader, media, prompt, memory, and orchestration paths are no longer
  presented as shipped surfaces.
- Rebuilt the workspace as seven crates: `cortex-types`, `cortex-kernel`,
  `cortex-retrieval`, `cortex-turn`, `cortex-runtime`, `cortex-sdk`, and
  `cortex-app`.
- Bumped the workspace and `cortex-sdk` to `1.5.0`.
- Switched the release Docker base to `rust:latest` while keeping the
  repository toolchain declaration on stable with `rustfmt` and `clippy`.

### Multi-User Ownership and Actor Isolation

- Added typed tenant, actor, client, session, turn, event, delivery,
  permission, and corpus identifiers as first-class runtime contracts.
- Added `OwnedScope`, `AuthContext`, and `Visibility` so every release-path
  object can be checked before private state is loaded, replayed, retrieved,
  delivered, or mutated.
- Added deny-by-default cross-tenant, cross-actor, and private-client checks.
- Added runtime tenant registration and client binding that rejects unknown
  tenants before follow-on state is created.
- Added authenticated ingress binding through a registry that stores only
  SHA-256 bearer-token digests and rejects empty, unknown, or incorrect
  credentials before a client is bound.
- Preserved the channel pairing invariant from the 1.2/1.3 line: pairing does
  not allocate a session by itself. The first real turn creates or reuses an
  actor-visible session.
- Added first-turn actor session reuse across clients for the same tenant and
  actor.
- Added per-client active session selection and delivery gates so a client only
  receives messages for its active session, not every session owned by the same
  canonical actor.
- Added journal recovery for tenant, client, session, and active-session
  bindings.

### Durable Substrate and Migration

- Replaced the old persistence surface with a compact `cortex-kernel`
  substrate containing a file-backed JSONL journal and a SQLite state store.
- Added visibility-filtered journal replay so private events are replayed only
  for matching owner scope.
- Added SQLite migrations for multi-user core state, memory state, permission
  state, delivery outbox state, and token-usage ledger state.
- Added schema migration ledger checks so applied migrations are durable and
  queryable.
- Added owner-filtered session queries and active-session persistence.
- Added owner-filtered fast-capture and semantic-memory persistence.
- Added owner-bound permission request and resolution persistence with request
  id, owner, and private-client matching.
- Added per-recipient delivery outbox persistence with delivery status,
  attempt count, and last-error fields.
- Added provider-reported token usage persistence and owner-filtered token
  usage totals.
- Added fixture-backed 1.4 session metadata import. Imported sessions remain
  private instead of being widened during migration.

### Workspace, Memory, and Control

- Added bounded workspace frames with item-count and token-budget admission.
- Added workspace items with kind, salience, urgency, taint, evidence id,
  source, and owner metadata.
- Added broadcast subscribers and explicit drop reasons so admission pressure
  is observable instead of silent.
- Added fast-capture and semantic-memory contracts with lifecycle status,
  provenance, confidence, and owner scope.
- Added consolidation jobs and interference reports so memory consolidation can
  detect actor-scoped conflicts rather than blindly promoting captures.
- Added drift-style accumulator contracts and expected-control-value
  decisioning for continue, wait, delegate, interrupt, and complete choices.
- Added tests for private visibility, actor-shared scope, workspace admission,
  memory interference, RAG taint blocking, outbound planning, permission
  resolution, and policy behavior in open mode.

### RAG and Retrieval

- Split retrieval evidence from durable memory. Retrieved material is evidence
  for a turn, not implicit long-term memory.
- Added query-scope authorization before corpus loading so forged query scopes
  are rejected early.
- Added corpus access classes and ACL checks so private corpus evidence cannot
  be loaded by another actor.
- Added deterministic BM25 lexical scoring with document-frequency and length
  normalization.
- Added deterministic rerank, citation scoring, support scoring, placement
  strategy, active-retrieval decisioning, and blocked-result reporting.
- Added evidence taint classification and blocking for instructional or unsafe
  retrieved material.
- Added tests for BM25 ranking, private-corpus isolation, and forged
  query-scope rejection.

### Turn Execution and Token Accounting

- Rebuilt `cortex-turn` around explicit `TurnPlanner`, `TurnExecutor`,
  `ModelProvider`, `ModelRequest`, `ModelReply`, and `TurnOutput` contracts.
- Added prompt assembly that includes retrieved evidence as untrusted evidence
  instead of flattening it into ordinary user or memory context.
- Preserved model-provider token usage from replies and made token accounting a
  shared typed contract.
- Added executor coverage for evidence wrapping and provider token usage
  preservation.

### Outbound Delivery and Transport Rendering

- Added structured `OutboundMessage`, `OutboundBlock`, `DeliveryPlan`,
  `DeliveryItem`, and `TransportCapabilities` contracts.
- Added Unicode-boundary-safe outbound planning with final length preservation.
- Added delivery records with planned, sent, failed, and acknowledged states.
- Added Telegram, QQ, and CLI transport adapters that render the same
  `DeliveryPlan` according to each transport's Markdown, plain-text, and media
  capabilities.
- Added tests pinning Markdown-preserving Telegram output and plain QQ output.

### Permission and Policy Contracts

- Added typed policy modes, action risk, permission requests, permission
  decisions, and permission resolutions.
- Added resolution validation that rejects mismatched request ids, wrong owner
  scope, and wrong private client.
- Preserved the security invariant that `open` mode cannot override ownership
  boundaries.

### Plugin SDK Boundary

- Rebuilt `cortex-sdk` as a small, capability-first SDK surface for 1.5.
- Added ABI version `2`, `PluginContext`, `ResourceLimits`, `ToolRequest`,
  `ToolResponse`, `PluginManifest`, and `PluginBoundary`.
- Added ABI validation, declared-capability validation, host-path denial by
  default, and output-limit enforcement.
- Added SDK conformance tests for ABI mismatch rejection, undeclared
  capability rejection, host-path denial, and output-size limits.
- Updated the SDK README to describe the 1.5 manifest, ABI, capability, and
  host-boundary expectations.

### Deployment and Release Mechanics

- Added deployment records with backup, migration, install, smoke, package, and
  publish steps.
- Added deployment evidence, artifact manifest records, rollback actions, and
  rollback completion state.
- Added release-readiness checks that require every release step in order and
  block progress after failed steps until rollback is recorded.
- Added `scripts/package-release.sh` to build the release binary and produce
  `dist/cortex-v1.5.0-linux-amd64.tar.gz` plus a matching sha256 file.
- Added package-surface checks that keep installer asset naming aligned with
  release packaging.
- Ignored generated `dist/` artifacts in Git.

### CLI and Public Surface

- Replaced the old broad CLI surface with a deliberately small operator
  surface for the current rewrite: `version`, `status`, `release-plan`, and
  `help`.
- Unknown commands now exit with status `2` instead of falling through.
- The current app reports the runtime/gate surface honestly rather than
  exposing old daemon, systemd, channel, browser, live tool-execution, or native
  plugin-loader commands that were removed from the active path.

### Documentation

- Rewrote README and README.zh around the actual 1.5 mechanisms and release
  constraints.
- Rewrote English and Chinese docs for compatibility, configuration,
  executive/context assembly, maturity, operations, plugins, quickstart,
  retrieval, roadmap, testing, and usage.
- Updated documentation to state that live daemon operation, HTTP/WebSocket
  wrappers, Telegram/QQ live clients, browser integration, live tool execution,
  and native plugin loading are not restored in the current 1.5 source path.
- Added docs drift checks for the 1.5 rewrite surface, RAG evidence semantics,
  executive control contracts, and multi-user test coverage.
- Updated testing documentation to make the zero-warning, zero-error policy and
  warning-suppression ban explicit.

### Validation Status

- Passed the cached Docker strict gate for the current tree, including
  suppression scan, `cargo fmt --all --check`, docs/package/secret checks,
  strict clippy with `-D warnings -W clippy::pedantic -W clippy::nursery`,
  full workspace tests, and doctests.
- The official release-authority command remains `./scripts/gate.sh --docker`
  and must pass before tag or release publication. The current environment is
  blocked before Rust/Cargo starts because external registry DNS requests for
  `rust:latest` time out or fail through the host DNS.

## 1.4.0 - 2026-04-26

### Production-Readiness Gate

- Made Docker the authoritative release gate through a pinned Rust 1.95.0 image, so local host toolchain drift no longer defines whether a release is valid.
- Added a single release command that runs warning-suppression scanning, formatting, docs drift checks, package-surface checks, secret/path scanning, strict clippy, the full workspace test suite, and doctests.
- Enforced the zero-warning policy as a release contract: `cargo fmt` must have no diff, clippy runs with `-D warnings -W clippy::pedantic -W clippy::nursery`, and Rust warning suppression attributes are rejected instead of tolerated.
- Added release-clean verification support so a tagged release can be validated from a committed tree instead of relying on a dirty workspace.
- Updated operator and testing documentation so the strict Docker gate is the documented release authority, while host and Docker Compose commands remain developer shortcuts.

### Retrieval and RAG Evidence

- Introduced a dedicated retrieval evidence plane separate from durable memory. Documents are chunked, indexed, retrieved, reranked, compressed, cited, and promoted as evidence rather than silently becoming recalled memory.
- Added hybrid sparse+dense retrieval with deterministic BM25-style lexical scoring, pluggable dense encoders, configurable paraphrase handling, score normalization, and configurable reranking limits.
- Added extension hooks for learned sparse expansion and late-interaction reranking, preserving scope checks and baseline sparse retrieval even when those hooks are enabled.
- Added actor and access-class filtering to retrieval, so private evidence stays bound to the requesting actor and public evidence is the only cross-actor default.
- Added taint-aware evidence modeling. Retrieved text is treated as untrusted or tainted evidence when appropriate, including retrieved instructions that look like prompt-injection attempts.
- Added citation, source-title, corpus, chunk, span, license, index-version, and score metadata to evidence so the runtime can explain where retrieved material came from.
- Added query transforms, including hypothetical-document style expansion, as query aids only. Transforms are explicitly not evidence and cannot be promoted as source material.
- Added retrieval-quality evaluation metrics and support decisions so low-support results trigger rerank/seek-more behavior instead of being treated as sufficient grounding.
- Added journal event payloads for retrieval decisions, retrieved evidence, and promoted evidence so RAG behavior becomes observable and replayable at the runtime surface.
- Added bilingual retrieval documentation describing the RAG pipeline, evidence safety model, implemented surface, and current limits.

### LLM Context Assembly

- Added a dedicated retrieved-evidence prompt layer between situational context and recalled memory.
- Rendered evidence with citation, source, corpus/chunk, span, access class, taint, license, index version, and score metadata instead of flattening it into ordinary conversation history.
- Marked retrieved evidence as inert context for the model: it may support answers, but it is not an instruction source and cannot override system, developer, user, or runtime policy.
- Preserved existing memory semantics by passing retrieval evidence independently from recalled memory.
- Added runtime-facing coverage that verifies retrieved evidence enters the assembled prompt before memory and retains citation/license metadata.

### Workspace and Control Contracts

- Added typed workspace frames and items with actor ownership, taint, kind, salience, token budget, and item-count budget validation.
- Added control-decision contracts for continue/wait/delegate/interrupt/complete behavior, including expected value, signal aggregation, conflicts, impasses, and subgoals.
- Added journal event payloads for workspace frame assembly, workspace item promotion, control decisions, and impasse recording.
- Extended public type exports so runtime, turn orchestration, and future plugins can share the same workspace/control/retrieval contract shapes.
- Updated replay and journal type mapping so the new 1.4 runtime events remain visible in payload-type projections.

### Documentation and Release Surface

- Bumped the workspace, SDK, plugin compatibility examples, scaffolded plugin templates, installer examples, and public docs to the `1.4.0` release line.
- Updated the README and README.zh architecture descriptions to include the retrieval evidence pipeline, separate evidence context, and expanded event surface.
- Updated executive docs so LLM request assembly explicitly includes retrieved evidence before recalled memory.
- Updated maturity notes to distinguish retrieved evidence from memory and keep the project framed as a local-first runtime with explicit hardening limits.
- Reworked the roadmap into one `1.4.0` production-readiness line, avoiding parallel future-version tracks and keeping remaining work scoped as internal workstreams.
- Added docs/runtime contract checks so retrieval docs, roadmap wording, strict-gate documentation, event counts, plugin version examples, and package metadata stay aligned with the shipped runtime.

### Runtime Quality

- Cleaned the persistence command shape to satisfy strict clippy and reduce oversized enum payloads without adding warning suppressions.
- Kept existing actor/session/channel ownership behavior intact while adding retrieval and context-assembly surfaces.
- Preserved installer asset naming and latest-release resolution so `install` and `update` continue to fetch the newest published binary by default.

### Validation

- Verified the release tree with:
  - `./scripts/gate.sh --docker`

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
