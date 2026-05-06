# Changelog

## Unreleased

## 1.6.7 - 2026-05-06

### Memory Extraction Completeness

- Bias default memory extraction toward complete durable recall so stable corrections, preferences, negative constraints, ordered release workflows, exact paths/versions/config keys, and credential-boundary rules are not collapsed into terse summaries.
- Keep secret handling explicit: store secret names/handles and credential boundaries when useful, but never store secret values.
- Accept wrapped memory extraction arrays such as `{"memories":[...]}` and `{"memory_candidates":[...]}` so providers that return named JSON objects do not drop valid candidates.

### Release Surface

- Added prompt-manager coverage for the completeness rules and post-turn parser coverage for wrapped memory candidate arrays.
- Retargeted release audit, roadmap, behavior report, bounded soak/fault, plugin docs, and contract tests to 1.6.7 while keeping `cortex-sdk` at 1.6.4.

## 1.6.6 - 2026-05-06

### Default Hidden Thinking

- Clean hidden `<think>...</think>` content before assistant history, journal events, and final response storage so default-hidden output does not depend only on provider request flags or a later result pass.
- Treat empty `CORTEX_SHOW_THINKING` and `CORTEX_STRIP_THINK_TAGS` values as unset, matching deployment scripts that may leave a knob blank.
- Verified the live default instance returns `VISIBLE_OK` with matching response parts and no thinking wrapper after rebuilding and restarting the local service.

### Prompt Defaults

- Made built-in `bootstrap.md`, `bootstrap-init.md`, and default `user.md` domain-neutral instead of assuming a coding or repository environment.
- Added prompt-manager coverage that rejects coder/repository-biased wording in those default prompts.

## 1.6.5 - 2026-05-06

### Thinking Request Controls

- Registered `/think` in Telegram bot commands and added Telegram/QQ card controls for showing, hiding, and checking thinking state.
- Mapped `/think show|hide` to the main provider request path so OpenAI-compatible providers can receive a configured thinking toggle instead of only post-processing visible output.
- Added vLLM reasoning support through `chat_template_kwargs.thinking` by default for vLLM providers, with optional `chat-template-enable-thinking` and top-level `thinking` mappings for other deployments.
- Normalized vLLM `reasoning` response fields into Cortex `<think>...</think>` handling so the existing default-hidden output path works for both streamed and non-streamed replies.

### Plugin SDK Cadence

- Kept `cortex-sdk` independent from workspace runtime releases; plugins can rely on their declared minimum supported Cortex runtime instead of forcing SDK updates for every Cortex patch.
- Updated scaffold/docs/package-surface checks to describe `cortex_version` as the oldest supported runtime version and keep SDK release cadence explicit.

## 1.6.4 - 2026-05-06

### Thinking Output Control

- Hide provider `<think>...</think>` output by default, including streamed responses, while preserving an explicit opt-in path for raw provider thinking.
- Added `/think show|hide|status`, `/config set turn.show_thinking ...`, and `cortex config set turn.show_thinking ...` controls, with live config reload for user daemons.
- Documented direct `config.toml` and install-time environment controls through `CORTEX_SHOW_THINKING` and `CORTEX_STRIP_THINK_TAGS`.

### Embedding Credentials

- Added `CORTEX_EMBEDDING_API_KEY` as a first-class install/config environment variable.
- Persist embedding provider API keys into `[embedding].api_key` and show only set/not-set state in config summaries.
- Removed the need for deployment scripts to patch `config.toml` manually after install.

### Release Surface

- Added bilingual `1.6.4` release audits and retargeted roadmap, release behavior, bounded soak/fault, package, SDK, plugin, scaffold, and contract-test surfaces to the `1.6.4` target.
- Verified a local destructive reinstall of the default instance with the configured vLLM endpoint, embedding API-key persistence, default hidden thinking, and installed-daemon `/think` mutation through HTTP JSON-RPC.
- Kept long 24h/72h/7d daemon soak as an explicit limitation unless a candidate actually runs and records it.

## 1.6.3 - 2026-05-04

### Plugin Compatibility

- Treat plugin manifest `cortex_version` as the minimum supported Cortex runtime version instead of requiring an exact match with the running release.
- Keep concrete semver validation strict: empty, invalid, range, and future Cortex requirements are rejected before plugin load or native library probing.
- Verified the official `by-scott/cortex-plugin-dev` v1.6.1 package installs, tests, reviews, and enables on the updated 1.6.3 runtime while preserving local signature/trust governance.

### Release Surface

- Added bilingual `1.6.3` release audits and retargeted roadmap, release behavior, bounded soak/fault, package, SDK, plugin, scaffold, and contract-test surfaces to the `1.6.3` target.
- Kept long 24h/72h/7d daemon soak as an explicit limitation unless a candidate actually runs and records it.

## 1.6.2 - 2026-05-04

### ModelOps Token Limits

- Replaced the provider-missing/offline 200k input / 300k output fallback with explicit config, provider metadata/cache, and conservative provider/model-family inference.
- Normalized legacy cached 200k/300k model-info entries by model so stale local caches do not preserve the old global fallback.
- Updated model routing, runtime fallback, generated config comments, and English/Chinese config docs to describe inferred limits as fallbacks, not provider proof.

### Release Evidence Surface

- Added bilingual `1.6.2` release audits and moved roadmap, release behavior, bounded soak/fault, package, SDK, plugin, scaffold, and contract-test surfaces to the `1.6.2` target.
- Carried forward P1 evidence surfaces for first-run diagnostics, policy profiles, plugin conformance, prompt-injection, actor-leakage, and replay-migration review without claiming sandbox containment or complete defense.
- Kept 24h/72h/7d daemon soak as an explicit limitation unless a candidate actually runs and records it.

## 1.6.1 - 2026-04-30

### Paired Channel Permission Model

- Restored paired Telegram/QQ channels as first-class trusted operating surfaces: normal read/write/build/test/script tools now flow through the regular permission gate instead of being blanket-denied because the instance has protected runtime roots.
- Kept Cortex instance state protected. Direct model-triggered access to prompts, config, sessions, journal, memory, channel state, and runtime files is still blocked through resolved protected-root checks.
- Returned concrete permission-denial reasons to tool results and journal events, replacing the opaque `permission denied` result.

### Plugin Effect Accuracy

- Stopped copying trusted native package-level manifest capabilities onto every native tool descriptor.
- Kept package capabilities as install/review metadata while LLM permission checks use each native tool's declared effects.
- Preserved the process-plugin guardrail that forces process-isolated plugin tools to expose `RunProcess:plugin subprocess`.

### Documentation and Release Surface

- Updated README, configuration, plugin, maturity, testing, roadmap, release-audit, usage, SDK, scaffold, and contract-test surfaces for the `1.6.1` release target.
- Added the bilingual `1.6.1` release audit and pointed release evidence scripts at the new target.
- Raised the repository Docker Compose `dev` service `nofile` limit so the release gate can run the highly parallel runtime SQLite/file-backed suites without descriptor-starvation flake.
- Bumped the workspace, SDK, plugin examples, tests, replay fixtures, scripts, and package documentation to `1.6.1`.

## 1.6.0 - 2026-04-30

### Executive Cache Boundary

- Moved volatile turn context out of the provider system prompt into a request-local runtime frame.
- Kept durable prompt files, stable skill summaries, and runtime permission context in the provider system prompt so provider caches can reuse the stable prefix more reliably.
- Kept bootstrap/resume context, active goals, retrieved evidence, recalled memory, reasoning state, and metacognitive hints outside the system prompt while preserving runtime-schema and policy authority.
- Added unit coverage for request-local runtime frames so dynamic context is appended for the current LLM call without mutating durable conversation history.

### Documentation and Release Surface

- Rewrote the English and Chinese READMEs into a tighter formal project narrative: Cortex as a cognitive harness substrate, not a generic agent framework.
- Updated Executive, roadmap, testing, quickstart, plugin, release-audit, and SDK documentation for the `1.6.0` release target.
- Documented the official [`cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev) development plugin as the reference package for coding and project-maintenance workflows.
- Added the bilingual `1.6.0` release audit as the current release truth table.

### Version Alignment

- Bumped the workspace, SDK, plugin examples, tests, scripts, replay fixtures, and package documentation to `1.6.0`.

## 1.5.11 - 2026-04-29

### Executive Cache Posture

- Reordered system prompt assembly so durable prompt files and stable skill summaries form the provider-cache-friendly prefix.
- Kept live runtime policy, active goals, resume/bootstrap context, retrieved evidence, recalled memory, metacognitive hints, history, and tool results in the dynamic suffix.
- Documented that cache-oriented ordering is an efficiency contract only: runtime schemas, current runtime policy, evidence trust, and tool boundaries remain authoritative.

### Provider Token Accounting

- Parsed OpenAI-compatible cached prompt usage from `prompt_tokens_details.cached_tokens`, `input_tokens_details.cached_tokens`, and top-level `cached_tokens`.
- Parsed Anthropic `cache_read_input_tokens` and `cache_creation_input_tokens`.
- Added cache-read and cache-creation input token fields to LLM completion events, turn aggregation, runtime metrics, HTTP status, operator timeline payloads, and `/status`.
- Split visible status into independent context, cache, and cumulative token lines.

### Persistence and Gate Hardening

- Hardened journal startup by creating the database parent directory explicitly and retrying transient SQLite open failures.
- Made kernel atomic writes and channel pairing state writes use unique temporary files before rename, closing same-process write collisions in release gate and runtime state persistence paths.

### Documentation and Validation

- Updated README, README.zh, Executive, operations, testing, roadmap, SDK, plugin, usage, release-audit, and release-report surfaces for the `1.5.11` release target.
- Added contract coverage so published docs continue to describe the cache-friendly Executive order and provider cache metrics.

## 1.5.10 - 2026-04-29

### Runtime State Surfaces

- Connected task persistence to the daemon JSON-RPC state surface. `task/create`, `task/list`, `task/get`, `task/delete`, `task/claim`, and `task/update` are actor-scoped; claims are atomic; updates enforce the task state machine.
- Replaced the unused JSON goal store with a SQLite-backed runtime goal store. Goals now carry owner actor, level, source, status, priority, success criteria, linked task, evidence refs, memory refs, deadlines, and completion time.
- Added actor-scoped `goal/create`, `goal/list`, `goal/get`, `goal/delete`, and `goal/update` JSON-RPC methods. Goal updates enforce checked transitions such as `Active -> Blocked -> Completed`, and open goals are injected into active turn context.
- Kept goal lifecycle events journaled through `GoalSet`, `GoalShifted`, and `GoalCompleted`, so replay and operator timelines retain the explicit goal signal.

### ACP Client Integration

- Added configured ACP client support through `[acp].clients`, including stdio JSON-RPC process launch, initialize/session/prompt flow, timeout handling, response-id validation, and streamed-text collection.
- Added the `acp_agent` tool so Cortex turns can delegate to configured ACP-compatible external agents while keeping the external process behind a bounded lifecycle.
- Journaled ACP invocation and response events, and added tests for configured-agent registration, invocation, id validation, and streaming output collection.

### Plugin Tool Guardrails

- Forced every process-isolated plugin tool to expose a `RunProcess:plugin subprocess` effect at runtime, even when the plugin manifest omits the `process` capability.
- Closed the plugin-tool effect underreporting path so protected runtime-home policy does not depend on plugin self-reporting for subprocess execution.
- Added regression coverage proving a process plugin tool without a declared process capability still reaches risk and protected-root policy as `RunProcess`.

### Runtime Home Protection

- Clarified that LLM-triggered plugin tool calls use the same tool registry, effect preview, permission gate, and approval path as built-in tools.
- Documented the trust boundary precisely: process plugins are governed subprocess tools; trusted native plugins are in-process trusted code and are not an OS sandbox.
- Kept direct prompt/config/session/journal/memory/runtime-state mutation out of model-callable plugin shortcuts under protected roots; self-evolution plugins can propose changes, but applying them remains on checked runtime paths.

### Project Cleanup

- Removed compatibility-era tests and docs that no longer described the current runtime contract.
- Removed dead or unconnected agent-pool orchestration/planner code and the old app plugin-loader shim instead of preserving compatibility paths.
- Reduced old fake delegation surfaces to explicit contract validation and kept live delegation on the connected sub-turn path.
- Kept the SDK independent from Cortex internal crates and aligned the SDK release line with the workspace release line.

### Documentation and Validation

- Updated README, README.zh, usage, config, ops, maturity, testing, plugin, roadmap, and release-audit documentation for the `1.5.10` release target.
- Added the bilingual `1.5.10` release audit as the working truth table for the current release claim, including explicit limitations for partial surfaces.
- Added task, goal, ACP, plugin-effect, actor-ownership, protected-root, and release-surface tests.
- Kept the release authority on the repository Docker gate: no warning suppressions, `cargo fmt --check`, docs/package/secret checks, strict clippy with `-D warnings -W clippy::pedantic -W clippy::nursery`, full workspace tests, and doctests.

## 1.5.9 - 2026-04-28

### Runtime Home Protection

- Treat the Cortex instance home as a protected runtime root for foreground and sub-turn tool evaluation.
- Block ordinary file/edit/write tool access into the protected runtime home, including symlinked paths that resolve under the instance directory.
- Block process/script execution while protected runtime roots are active, closing bash and script-based bypass paths around prompt/config/runtime-state protection.
- Keep prompt evolution on the checked PromptManager path instead of allowing direct model-driven edits to durable prompt Markdown files.

### Status and Token Accounting

- Reworked `/status` and daemon status text into separate token lines: last-call context usage and cumulative token spend.
- Persist per-session input/output token totals in session metadata so status can report cumulative spend as total/session instead of only daemon-wide totals.
- Kept raw daemon metrics available programmatically while removing low-value turn/daemon token clutter from the user-visible status card.

### QQ Pairing and Command Routing

- Changed QQ inbound routing so pairing is checked before slash-command dispatch.
- Prevented an unpaired first `/status` message from producing QQ command cards; pairing prompts are plain text and do not attach command options.
- Added QQ route tests covering pairing-before-command behavior.

### Documentation and Validation

- Updated status, operations, roadmap, release-audit, plugin, usage, and SDK-facing documentation for the `1.5.9` release target.
- Added targeted tests for protected runtime roots and QQ command routing, and kept the release surface bound to the repository Docker gate.

## 1.5.8 - 2026-04-28

### Executive Refactor

- Rebuilt the default Executive prompt set around one Cortex instance: `soul.md`, `identity.md`, `behavioral.md`, and `user.md` now have clearer non-overlapping responsibilities and stronger grounding in attention, memory, metacognition, evidence, autonomy, and collaborator correction.
- Reworked system templates for memory extraction, context compression, prompt self-update, entity extraction, memory consolidation, bootstrap, bootstrap initialization, worker turns, batch analysis, causal analysis, summarization, and metacognitive hints.
- Refactored default system skills (`deliberate`, `diagnose`, `review`, `orient`, `plan`) into higher-density reusable control procedures with explicit activation purpose, evidence rules, failure modes, and verification expectations.
- Rewrote core tool descriptions and tool-result wrappers so the model receives compact, high-signal contracts for capability use, failure signals, trust boundaries, and parameter intent.
- Tightened actual LLM input sections for runtime policy, active skills, retrieved evidence, recalled memory, and untrusted tool output so live state, evidence, memory, and capabilities stay separate.

### Bootstrap and Self-Evolution

- Updated bootstrap behavior to initialize not just an instance name, but also collaborator profile, work context, communication style, environment, autonomy rules, privacy boundaries, and approval expectations.
- Strengthened prompt self-evolution rules so durable updates remain evidence-bound, scoped to the correct prompt file, and prevented from fossilizing runtime state, tool inventories, permission modes, or transient plans.
- Preserved the soul as the sacred seed and carrier of the instance while keeping runtime schemas authoritative for real capabilities.

### Telegram Delivery Integrity

- Reworked Telegram text delivery around one Markdown-to-HTML rendering path for streaming drafts, final answers, callbacks, and inline-keyboard messages.
- Split long Telegram responses by rendered size with independently renderable Markdown chunks, so completed answers do not depend on the number of draft bubbles produced while streaming.
- Prevented silent truncation during message edits: single-message edits now reject multi-chunk text and fall back to complete replacement sends.
- Delayed Telegram streaming draft updates while Markdown strong/code/fenced-code markers are still open, preventing partially rendered text from exposing visible Markdown markers.
- Converted Markdown to plain text before Telegram fallback sends/edits, so API fallback paths do not leak formatting syntax.

### Documentation

- Rewrote README, README.zh, and Executive documentation around Cortex as an ambitious cognitive harness substrate and one daemon-backed working individual, not a thin agent wrapper.
- Updated roadmap, release-audit, plugin, usage, quickstart, and SDK-facing examples to the `1.5.8` release target.
- Added `1.5.8` release-audit documents in English and Chinese with explicit Executive and Telegram patch evidence.

### Validation

- Added and updated tests for Telegram long-message delivery, Markdown chunking, strong-marker streaming drafts, Markdown-free fallback paths, evidence rendering, and untrusted tool-output wrapping.
- Kept the release target bound to the repository Docker gate and the strict zero-warning policy.

## 1.5.7 - 2026-04-28

### Telegram Delivery Integrity

- Reworked Telegram text delivery around one Markdown-to-HTML rendering path for streaming drafts, final answers, callbacks, and inline-keyboard messages.
- Split long Telegram responses by rendered size, with independently renderable Markdown chunks, so a final answer no longer depends on how many draft bubbles existed while the model was streaming.
- Prevented silent truncation during message edits: single-message edits now reject multi-chunk text and fall back to sending a complete replacement instead of keeping only the first chunk.
- Cleaned up stale draft bubbles after final delivery when the completed answer uses fewer chunks than the streamed draft.
- Simplified code-block HTML to Telegram-supported `<pre><code>` markup, avoiding language-class attributes that Telegram does not consistently accept.
- Delayed Telegram streaming draft updates while Markdown strong/code/fenced-code markers are still open, preventing partially rendered bold text from exposing visible `**` markers.
- Deleted stale Telegram draft messages after replacement sends so failed edits do not leave older half-rendered bubbles in the chat.
- Converted Markdown to plain text before Telegram fallback sends/edits, so API fallback paths do not leak Markdown syntax.

### Project Narrative

- Rewrote the README and Chinese README around the mature harness ecosystem rather than isolated-prompt comparisons.
- Repositioned Cortex as a cognitive harness substrate for durable, inspectable, governable, recoverable model behavior across real tools and interfaces.
- Expanded the cognitive-runtime narrative around attention, working memory, durable memory, feedback, metacognition, value/risk evaluation, and replay, while keeping biological-consciousness and biological-wisdom claims explicitly out of scope.

### Validation

- Added Telegram unit coverage for long Markdown responses, fenced code block splitting, response-tail preservation, per-chunk size limits, closed Markdown state, and supported code-block HTML.
- Added Telegram unit coverage for strong-marker streaming drafts, strong-marker chunk splitting, and Markdown-free plain-text fallback.

### SDK and Plugin Publishing Documentation

- Published `cortex-sdk` as `1.5.7` so native plugin authors can depend on the current release line directly.
- Expanded SDK and plugin documentation into a full scaffold-to-release path covering process plugins, trusted native plugins, signing keys, conformance checks, package signing, `.cpx` packaging, GitHub Release upload, and user install.

## 1.5.6 - 2026-04-28

### Plugin Signing and Publisher Trust

- Added Ed25519 signing for governed plugin packages through `cortex plugin keygen` and `cortex plugin sign`.
- Added signed `package.toml` metadata fields for signature algorithm and public key, with signature payload coverage for manifest hashes, native artifact hashes, SBOM/risk/conformance references, and supported packaged files.
- Enforced signature verification for packaged installs from `.cpx`, URL, and GitHub release names.
- Added local publisher trust-on-first-use at `$CORTEX_HOME/plugin-trust.toml`; interactive installs can confirm a new verified publisher key, while non-interactive installs can use `--yes` after operator review.
- Fixed top-level CLI option validation so install and plugin release commands accept `--permission-level`, `--key`, `--publisher`, and `--yes`.
- Fixed plugin signing so a source tree that relies on packer auto-resolution from `target/release` signs the same native library payload that will be installed from the `.cpx` archive.
- Rejected unsigned packaged installs under release policy, invalid signatures, manifest/native hash mismatches, tampered signed files, and unknown publishers under reject/non-interactive policy.
- Hardened plugin archive extraction and directory copying to ignore symlinks and unsupported archive entry types.

### Documentation and Validation

- Updated plugin, usage, quickstart, README, roadmap, release-audit, and SDK-facing documentation for signed publishing and local publisher trust.
- Updated the repository Docker base image back to `rust:latest` for the authoritative Compose gate.
- Added app-level integration coverage for signed package install, unsigned package rejection, unknown-publisher rejection, tampered payload rejection, and verified signature listing.

## 1.5.5 - 2026-04-27

### Runtime Contracts

- Added richer control-decision records for tool permission paths, including candidate actions, rejected alternatives, reversibility, required evidence, blocking uncertainty, risk boundary, fallback plan, and permission prompt explanations.
- Upgraded the attention scheduler from fixed channel selection to resource-governed scheduling with actor budgets, maintenance debt, emergency debounce, deadlines, cost, risk, priority inheritance, and schedule explanations.
- Added prompt linting for checked prompt updates, rejecting durable prompt writes that try to fossilize runtime policy, reference absent capabilities, make unsupported release/security/cognition claims, or include unapproved self-edit diffs.
- Added media evidence governance to core attachments and channel download paths: media id, hash, source actor, source URI, license, taint, vision confidence, external-recipient policy, durable-memory policy, and publish policy. Defaults block silent memory, publishing, and cross-actor external delivery.
- Added controlled delegation contracts for worker tasks, covering scope, allowed tools, forbidden actions, token and iteration budgets, evidence budget, allowed evidence, expected artifact, merge verifier, review requirement, and parent-authority inheritance.
- Added secret dataflow handles and sink policy. Model-visible secret references expose only handle/source/purpose metadata; the default policy allows runtime-broker use and blocks provider, web, plugin, channel, memory, and log sinks unless explicitly allowed.
- Added typed feedback attribution and replay checks so user corrections can be attributed to style, fact, tool choice, memory, evidence, permission judgment, prompt, skill, policy, or unknown, then checked against future similar tasks.

### Plugin, Sandbox, and Evaluation

- Rejected plugin sandbox-enforcement claims the runtime cannot actually provide yet, including uid-drop, no-network, system sandbox, container/VM, remote-worker, and seccomp claims.
- Added release behavior and bounded soak/fault harness scripts to produce review evidence for memory, retrieval/RAG, tools, safety, operator timeline, long-task recovery, replay, provider/channel/SQLite/plugin/disk/config/rate-limit faults, and reconnect behavior.
- Updated release audit documentation so every `1.5.5` review area has explicit code evidence, test evidence, remaining limits, and disposition.

### Validation

- Verified the touched release surface in the repository Docker Compose environment with suppression checks, `cargo fmt --all --check`, docs drift checks, secret scans, package tests for `cortex-types`, `cortex-turn`, `cortex-retrieval`, `cortex-kernel`, runtime test compilation, and strict clippy using `-D warnings -W clippy::pedantic -W clippy::nursery`.

## cortex-sdk 1.5.4 - 2026-04-27

- Published an SDK-only patch release because the previous `1.5.x` SDK uploads on crates.io are yanked and cannot be overwritten.
- Kept `cortex-sdk` independent from Cortex workspace internals; it depends only on public serialization crates and owns the stable plugin DTOs exposed to native plugins.
- Updated the README positioning language to reflect the current harness ecosystem instead of framing Cortex against one-shot prompting.

## 1.5.0 - 2026-04-27

### Runtime Harness Direction

- Reframed Cortex as a cognitive harness for language models: a controlled runtime for exercising, observing, replaying, and hardening model behavior rather than a generic agent wrapper.
- Preserved the mature 1.4 daemon, transport, session, plugin, retrieval, and replay surfaces while strengthening the runtime contracts around them instead of replacing them with a thinner rewrite.
- Kept Docker Compose as the authoritative repository gate, including warning-suppression scanning, docs drift checks, package-surface checks, secret/path scanning, strict clippy, the full workspace test suite, and doctests.

### Tool Effects and Runtime Policy

- Added typed tool-effect contracts for file access, process execution, network access, memory persistence, channel send, scheduling, media generation, delegation, and related side-effect surfaces.
- Added transactional side-effect journal events for preview, verification, and commit so mutating tools can be inspected and replayed as explicit runtime activity.
- Added policy-as-code checks for configuration and plugin manifests, including dangerous plugin combinations, undeclared capabilities, unsafe environment inheritance, sandbox claims, and tool/effect decisions.
- Added policy simulation so an operator can inspect a single tool/effect decision before execution.
- Exposed declared tool effects to the risk assessor so tool risk is tied to actual capability surfaces rather than only tool names.

### Plugin Governance and SDK Surface

- Expanded plugin manifests with trust tiers, requested file/network/process/secret/background capabilities, sandbox profile, package metadata, signed-package fields, SBOM and risk-profile references, conformance certificate fields, and declared tool effects.
- Hardened process-plugin validation for command paths, working directories, host-path opt-in, inherited environment variables, timeout behavior, output limits, invalid JSON output, and unsafe secret access.
- Added local conformance coverage for process and native plugin boundaries, including manifest compatibility, native ABI callback-table validation, and manifest-declared effect propagation.
- Made `cortex-sdk` independent of Cortex workspace internals: it no longer depends on `cortex-types`, owns its stable plugin DTOs, and converts to runtime types only at the daemon boundary.
- Updated plugin scaffolding, public plugin docs, SDK README examples, and install examples to the 1.5 release line.

### Replay, Audit, and Compatibility

- Added replay-facing causal audit graph coverage that links permissions, tool effects, memory changes, and runtime decisions.
- Added replay diff support and fixture-backed replay compatibility for current event surfaces, including externalized payloads, tool-effect transactions, compaction boundaries, side-effect substitution, and legacy execution-version handling.
- Added policy and compatibility fixtures so schema evolution remains testable against historical journal, memory, plugin, actor, retrieval, and daemon-state surfaces.
- Extended package and documentation contract tests so README positioning, roadmap commitments, plugin surfaces, risk surfaces, event counts, and compatibility language stay aligned with shipped code.

### Retrieval, Evidence, and Workspace Control

- Strengthened RAG support verification so answer claims can be reported as supported, contradicted, unsupported, or insufficiently supported, with negative evidence overriding stale support.
- Preserved the separate evidence plane for retrieved material and ensured evidence promotion remains actor-scoped, taint-aware, budget-aware, and citation-preserving.
- Expanded workspace admission with lane, utility, risk, volatility, taint, marginal utility, admission outcome, contamination barrier, and eviction records.
- Added prompt-context coverage that keeps retrieved evidence before recalled memory and prevents retrieved instructions from becoming runtime instructions.

### Guardrails and Safety Harnesses

- Expanded red-team coverage across web, file, plugin, channel, wrapped-evidence, fragmented payload, and hostile tool-output paths.
- Kept hostile external content as tainted evidence or metadata-only summaries, journaled guardrail hits, and prevented raw hostile text from re-entering instruction-bearing history.
- Hardened risk assessment for unknown, plugin, MCP, mutating, and effect-declaring tools under policy overrides, blocklists, allowlists, and nested hostile payloads.

### Metacognition, Skills, and Model Routing

- Added richer adaptive-threshold feedback with outcome, intervention, confidence delta, intervention success rate, precision, and threshold snapshots.
- Added skill manifests with preconditions, inputs, outputs, effects, required tools, risk, expected duration, success criteria, fallback behavior, and observability metadata.
- Added bounded skill execution traces so skill behavior becomes inspectable without turning traces into unbounded logs.
- Added capability-based model routing from `[llm_groups.*]` and provider metadata, covering coding, long context, vision, tool calling, JSON reliability, latency, cost, safety, and reasoning depth.
- Added route explanations for selected group, fallback reasons, rejected failed targets, schema-invalid fallback, and low-confidence or high-risk escalation.

### Operator Dashboard and Observability

- Added `GET /api/operator/dashboard` for local operators, returning daemon state, metrics, active and persisted sessions, shared bindings, pending permissions, backlog, risk mode, provider model profiles, and bounded timeline data.
- Added JSON-RPC `operator/dashboard` across HTTP, socket, WebSocket, and stdio transports with the same local-operator identity enforcement as other operator-only methods.
- Normalized recent journal events into lifecycle, message, LLM, tool, permission, workspace, retrieval, memory, control, guardrail, scheduler, and other runtime timeline categories.
- Added HTTP, JSON-RPC, direct RPC, and documentation contract coverage for the dashboard and timeline surface.

### Documentation and Release Surface

- Rewrote README and README.zh as formal project documentation with precise harness positioning, daemon entrypoints, architecture, runtime contracts, plugin boundaries, and repository Docker gate requirements.
- Updated usage, operations, maturity, retrieval, plugin, compatibility, config, testing, and roadmap docs to describe the 1.5 runtime contracts as shipped.
- Moved model routing and operator dashboard work from active P1 planning into implemented release-line evidence.
- Bumped workspace, SDK, scaffolded plugin templates, plugin examples, install examples, and public release-surface docs to `1.5.0`.

### Validation

- Verified the release tree with:
  - `./scripts/gate.sh --docker`

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
