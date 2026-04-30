# 1.5.11 Release Audit

This is the working truth table for the `v1.5.11` release target. It is not a
release note and not a marketing claim. Each row maps to one required review
area from `docs/roadmap.md` and records the current evidence, the verification
surface, the remaining gap, and the `1.5.11` disposition.

Status meanings:

- **Surface present**: code and tests already expose a relevant contract, but
  the release review must still verify limitations.
- **Partial**: part of the contract exists and has acceptance evidence, but the
  remaining limitation must stay public and must not be promoted as a completed
  release claim.
- **Release blocker**: the area lacks a required runtime contract or acceptance
  proof and must be completed before `1.5.11` ships.

## Audit Table

| Area | Current code evidence | Current verification | `1.5.11` gap | Status |
|------|-----------------------|----------------------|-------------|--------|
| Memory | `crates/cortex-types/src/memory.rs`, `crates/cortex-kernel/src/memory_store.rs`, `crates/cortex-turn/src/memory/*`, `crates/cortex-turn/src/tools/memory_tools.rs` expose claim ids, evidence events, user confirmation, contradictions, supersession, validity windows, risk-if-wrong, usage outcomes, actor ownership, recall, consolidation, and memory tools. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-turn/tests/memory_tools.rs`, ``docs/testing.md`, `docs/roadmap.md`. | Verify an end-to-end stabilization path that explains why a belief stabilized, which evidence supports it, what conflict/refutation exists, and whether later task outcomes improved. | Partial |
| Retrieval / RAG | `crates/cortex-types/src/retrieval.rs` and `crates/cortex-retrieval/src/lib.rs` define evidence roles, taint, access, query transforms, hybrid retrieval, rerank hooks, citations, support verification, negative evidence, and workspace promotion. | `crates/cortex-retrieval/tests/rag_pipeline.rs`, `crates/cortex-runtime/src/tests/retrieval_context.rs`, `docs/retrieval.md`, `docs/testing.md`. | Connect answer support reports to runtime final-answer gating and release behavior metrics, not only independent retrieval tests. | Partial |
| Workspace / Context | `crates/cortex-types/src/workspace.rs` defines typed lanes, taint barriers, utility/risk/volatility, budgets, marginal-utility admission, eviction records, and actor/session checks. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-turn/src/context/*`, `crates/cortex-runtime/src/tests/retrieval_context.rs`, `docs/testing.md`. | Add operator-facing frame explanations and confirm runtime context assembly never lets retrieved/tool text become policy, identity, permission, or tool instruction. | Partial |
| Control / Decision | `crates/cortex-types/src/control.rs`, retrieval control helpers, TPN tool-risk control, runtime permission prompts, and operator timelines record control signals, candidate actions, rejected alternatives, required evidence, reversibility, fallback plan, confidence, benefit, cost, risk, impasses, conflicts, and waits. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-turn/src/orchestrator/tpn.rs`, `crates/cortex-runtime/src/tests/control.rs`, `crates/cortex-retrieval/tests/rag_pipeline.rs`. | Extend the same trace coverage to every non-tool control transition and include decision-trace summaries in the final release behavior report. | Partial |
| Metacognition | `crates/cortex-turn/src/meta/*`, `crates/cortex-turn/src/orchestrator/tpn.rs`, and runtime status surfaces expose doom loop, fatigue, frame anchoring, health, adaptive thresholds, alert feedback, and calibration snapshots. | `crates/cortex-turn/src/meta/adaptive.rs` tests, `docs/testing.md`, `docs/executive.md`, `docs/config.md`. | Expand typed alerts to the full roadmap taxonomy and prove alert-to-intervention mappings change control flow with recorded outcomes. | Partial |
| Attention / Scheduler | `crates/cortex-types/src/attention.rs`, `crates/cortex-turn/src/attention/mod.rs`, and runtime heartbeat/maintenance paths expose foreground, maintenance, emergency channels, task metadata, maintenance debt, emergency debounce, actor budgets, deadlines, risk/cost, priority inheritance, and schedule explanations through `ChannelScheduled`. | `crates/cortex-turn/src/attention/mod.rs` unit tests, README/docs wording, runtime status behavior, and event serialization contracts. | Connect scheduler reports to a broader daemon-level resource report that includes long-running maintenance debt and per-actor fairness over time. | Partial |
| Risk / Permission | `crates/cortex-types/src/tool_effect.rs`, `crates/cortex-turn/src/risk/assessor.rs`, `crates/cortex-types/src/permission.rs`, and policy code model effect floors, confirmations, dry-run posture, unknown-tool risk, and risk overrides. | `crates/cortex-turn/tests/safety_contracts.rs`, `crates/cortex-types/tests/contracts.rs`, `crates/cortex-kernel/src/policy.rs` tests, `docs/config.md`. | Finish effect-targeted policy explanations end to end, including operator prompts that show paths/domains, reversibility, dry-run status, approval actor, and rollback path. | Partial |
| Guardrails | `crates/cortex-turn/src/guardrails.rs`, `crates/cortex-turn/src/security.rs`, and orchestrator guardrail handling classify prompt injection, role override, leakage, exfiltration, hostile source memory, safe transformations, and taint disposition. | `crates/cortex-turn/tests/safety_contracts.rs`, `crates/cortex-turn/src/tests/orchestrator_guardrails.rs`, `docs/maturity.md`, `docs/testing.md`. | Keep the finite rule corpus honest: add scenario-level adversarial harness reports and ensure taint propagation is tested through web, file, plugin, channel, workspace, and tool-argument paths. | Partial |
| Plugin System | `crates/cortex-types/src/plugin.rs`, `crates/cortex-runtime/src/plugin_loader.rs`, `crates/cortex-runtime/tests/process_plugin.rs`, and app plugin commands expose trust tiers, manifests, native ABI, process tools, capability requests, conformance, signed package metadata, Ed25519 verification, publisher trust-on-first-use, and effect derivation. | `crates/cortex-runtime/tests/process_plugin.rs`, `crates/cortex-sdk/tests/native_abi.rs`, `crates/cortex-app/tests/plugin_manager.rs`, `docs/plugins.md`, `docs/testing.md`. | Signed `.cpx` installs now verify package hashes, file payloads, publisher key signatures, and local publisher trust. Remaining limitation: there is no central registry, transparency log, revocation service, or independent conformance authority yet; trust remains local and operator-governed. | Surface present |
| Sandbox / Containment | Plugin manifests include sandbox levels, network/filesystem modes, uid-drop fields, seccomp labels, CPU/memory limits, runtime process-plugin controls for path/env/timeout/output, and governance rejection for sandbox-enforcement claims the runtime cannot provide. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-runtime/tests/process_plugin.rs`, `docs/maturity.md`, `docs/plugins.md`, `docs/testing.md`. | Add real container/seccomp/uid/no-network enforcement before allowing those manifest claims; current release behavior blocks overclaiming instead of pretending OS isolation exists. | Partial |
| Replay / Journal | `crates/cortex-kernel/src/journal.rs`, `crates/cortex-kernel/src/replay.rs`, `crates/cortex-types/src/event.rs`, and `crates/cortex-types/src/causal.rs` expose event sourcing, projection versions, replay diffs, side-effect substitution, causal edges, externalized payloads, and replay fixture shapes. | `crates/cortex-kernel/tests/persistence_replay.rs`, `crates/cortex-kernel/tests/fixtures/replay/*`, `docs/testing.md`. | Add release-review output for replay diffs and current fixtures, including side-effect idempotency/rollback receipt coverage for mutating external actions. | Partial |
| Actor / Ownership | Runtime stores and routes enforce actor-scoped session, memory, task, goal, audit, RPC, HTTP, WS, socket, stdio, channel binding, and transport-rebind visibility. Task and goal storage are daemon-owned and exposed through checked JSON-RPC methods rather than remaining test-only persistence surfaces. Open goals are injected into active turn context through the same actor scope. | `crates/cortex-runtime/src/tests/*`, `crates/cortex-runtime/src/tests/rpc_tasks.rs`, `crates/cortex-runtime/src/tests/rpc_goals.rs`, `crates/cortex-kernel/tests/persistence_replay.rs`, `crates/cortex-turn/tests/memory_tools.rs`, `crates/cortex-runtime/src/tests/http_audit.rs`, `docs/testing.md`. | Move beyond string actor checks where possible: introduce or enforce scope proof objects at sensitive load/query boundaries and audit private-data flows into workspace, transport, and tool inputs. | Partial |
| Prompt / Executive | `crates/cortex-kernel/src/prompt_manager.rs`, `crates/cortex-types/src/prompt.rs`, `crates/cortex-turn/src/context/builder.rs`, `crates/cortex-turn/src/skills/defaults.rs`, core tool schema descriptions, runtime policy rendering, evidence/memory context rendering, `docs/executive.md`, and prompt tests cover prompt responsibilities, bootstrap, self-evolution, evidence boundaries, runtime-schema precedence, cache-friendly stable-prefix assembly, skill procedures, tool contracts, prompt storage, and checked prompt updates through a compiler/linter. | `crates/cortex-kernel/tests/prompt_manager.rs`, `crates/cortex-turn/src/tests/orchestrator_guardrails.rs`, `crates/cortex-turn/src/context/builder.rs` tests, `crates/cortex-turn/src/context/evidence.rs` tests, `docs/executive.md`, `docs/testing.md`, README contract tests. | Continue surfacing prompt lint reports in operator review and expand live self-evolution acceptance tests beyond prompt-manager unit coverage. | Surface present |
| Skills / Repertoire | `crates/cortex-types/src/skills.rs`, `crates/cortex-turn/src/skills/*`, and default skills expose manifests, triggers, effects, risk, observability, execution modes, and trace capture. | `crates/cortex-turn/src/skills/mod.rs` tests, `crates/cortex-runtime/src/tests/*` skill RPC visibility checks, `docs/executive.md`, `docs/testing.md`. | Add candidate quarantine lifecycle and stronger success/failure criteria with user-feedback-linked utility updates. | Partial |
| Tool Execution | Tool effects, runtime event payloads, write/media/web/memory/ACP tools, and daemon timelines expose effect declarations, preview, verification, commit records, receipts, and mutating side-effect categories. ACP client calls are configured through `[acp].clients`, exposed through `acp_agent`, and journaled through ACP invocation/response events. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-kernel/tests/fixtures/replay/tool_effect_transaction.toml`, `crates/cortex-turn/src/acp_client/mod.rs` tests, `crates/cortex-turn/src/tools/acp_agent.rs` tests, `crates/cortex-turn/tests/safety_contracts.rs`, `docs/testing.md`. | Enforce transactional sequence for every mutating tool: plan, preview, permission, execute, verify, commit, rollback handle, and record. | Partial |
| Model / Provider Routing | `crates/cortex-types/src/model_routing.rs`, config/provider metadata, OpenAI-compatible/Anthropic usage parsers, and runtime daemon routing expose capability registry, route explanations, fallback, schema-invalid fallback, high-risk escalation, cost/latency/safety/reasoning signals, and provider cache-read/cache-creation token usage. | `crates/cortex-types/tests/model_routing.rs`, `crates/cortex-turn/src/llm/openai.rs` tests, `crates/cortex-turn/src/llm/anthropic.rs` tests, `docs/config.md`, `docs/testing.md`. | Feed route outcomes back into calibration and expose route decisions in release reports/operator traces. | Partial |
| Evaluation | Retrieval evaluation, model routing tests, safety red-team corpus, actor isolation tests, actor isolation tests, strict gate scripts, and `scripts/release-behavior-report.sh` provide the release behavior evidence surface. | `docs/testing.md`, `docs/ops.md`, `scripts/gate.sh`, `scripts/release-behavior-report.sh --check`, `crates/cortex-retrieval/tests/rag_pipeline.rs`, `crates/cortex-turn/tests/safety_contracts.rs`. | Attach the generated `--run` report to the final release review and keep missing soak evidence as an explicit limitation until the soak/fault harness row is complete. | Partial |
| Observability | Runtime metrics, HTTP operator dashboard, status surfaces, journal timeline normalization, timeline categories, token/cost/provider/session summaries, provider cache read/write counters, and health endpoints exist. | `crates/cortex-runtime/src/tests/http_operator.rs`, `docs/ops.md`, `docs/testing.md`. | Add complete turn timeline views for workspace, retrieval, memory, control, tools, guardrails, risk ledger, and memory change review. | Partial |
| Configuration / Policy | `crates/cortex-kernel/src/config_loader.rs`, `config_validator.rs`, `policy.rs`, CLI policy commands, docs, and gate scripts cover config loading, validation, lint, simulation, plugin/tool danger findings, and Docker gate requirements. | `crates/cortex-kernel/tests/config_loader.rs`, `crates/cortex-kernel/src/policy.rs` tests, `crates/cortex-app/tests/plugin_manager.rs`, `docs/config.md`, `docs/testing.md`. | Add explicit policy profiles and schema/lint reports that are persisted or surfaced at daemon startup and release review. | Partial |
| Operations / Soak | Install/ops docs, runtime stability modules, channel/session tests, current persistence tests, fault evidence tests, and `scripts/soak-fault-harness.sh` cover bounded provider, channel, SQLite, plugin crash, disk/config, rate-limit/backpressure, replay determinism, and reconnect evidence. | `docs/ops.md`, `docs/maturity.md`, `scripts/soak-fault-harness.sh --check`, `crates/cortex-runtime/src/tests/*`, `crates/cortex-kernel/tests/*`. | Attach a generated `--run` bounded soak/fault report to release review. Long 24h/72h/7d daemon soak remains a separate limitation unless run for the candidate. | Partial |
| Multimodal / Media | Media config, Telegram/QQ/WhatsApp channel media paths, `send_media`, `image_gen`, attachment DTOs, SDK media result surfaces, and core attachment governance fields now exist. Runtime channel downloads attach source URI, source actor, media id, hash, taint, and default policies that block silent durable memory, publishing, and cross-actor external delivery. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-runtime/src/channels/{telegram,qq,whatsapp}.rs`, `crates/cortex-turn/src/orchestrator/tpn.rs`, `docs/config.md`, `docs/ops.md`, SDK/tool media DTO tests. | Route OCR/vision-derived confidence from every provider-specific parser, and surface media-governance decisions in operator review before memory/publish approval. | Partial |
| Delegation / Multi-worker | `agent` tool execution now routes through the online sub-turn path with explicit `DelegationContract` enforcement. Worker prompts include scope, allowed tools, forbidden actions, token/iteration/evidence budgets, expected artifact, merge verifier, review requirement, and minimal authority inheritance. Worker tool registration and turn budgets are filtered by that contract. | `crates/cortex-turn/src/agent_pool/delegation.rs` tests, `crates/cortex-turn/src/orchestrator/tpn.rs` contract-enforcement tests, and runtime skill/delegation visibility checks listed in `docs/testing.md`. | Persist delegation-contract records into the journal and operator timeline for every live delegated worker. | Partial |
| Security / Secrets | Risk scoring, sensitive-path detection, secret scan script, plugin secret capability, MCP process environment clearing, app/SDK package checks, secret handles, and default secret sink policy exist. Secret references rendered to the model carry handle/source/purpose metadata without values; default policy permits runtime-broker use and blocks provider, web, plugin, channel, memory, and log sinks unless explicitly allowed. | `scripts/check-secrets.sh`, `crates/cortex-types/src/secret.rs`, `crates/cortex-turn/tests/safety_contracts.rs`, `crates/cortex-runtime/tests/process_plugin.rs`, `docs/testing.md`. | Wire secret handles into every real provider/tool/plugin call path so broker injection is the only path that can materialize values at execution time. | Partial |
| Data Model / Schema | Event payloads, execution version, current persistence tests, replay fixtures, plugin manifest version targeting, and current store tests exist. | `crates/cortex-types/tests/contracts.rs`, `crates/cortex-kernel/tests/persistence_replay.rs`. | Add generated runtime specs and current fixture report for accepted and rejected data. | Partial |
| Human Feedback | Feedback memory type, memory usage outcomes, user confirmation, prompt/user model docs, adaptive alert feedback, memory tool guidance, typed feedback attribution, and feedback replay checks exist. Feedback can be classified as style, fact, tool choice, memory, evidence, permission judgment, prompt, skill, policy, or unknown, and MemoryEntry can retain both attribution records and future similar-task replay evidence. | `crates/cortex-types/src/feedback.rs`, `crates/cortex-types/tests/contracts.rs`, `crates/cortex-turn/tests/memory_tools.rs`, `crates/cortex-turn/src/meta/adaptive.rs`, `docs/executive.md`. | Route live user correction events into feedback memories automatically and expose replay-check status in operator review. | Partial |

## 1.5.11 Patch Evidence

`v1.5.11` is the current harness contract and adds cache-aware Executive assembly plus provider cache observability on top of the existing cleanup and plugin-tool hardening:

- The system prompt assembly now keeps durable prompt files and stable skill summaries before live runtime policy, active goals, resume/bootstrap context, retrieved evidence, recalled memory, and metacognitive hints. This reduces unnecessary provider prompt-cache invalidation while preserving runtime-schema and policy precedence.
- OpenAI-compatible responses now parse cached prompt tokens from `prompt_tokens_details.cached_tokens`, `input_tokens_details.cached_tokens`, or top-level `cached_tokens`. Anthropic responses now parse `cache_read_input_tokens` and `cache_creation_input_tokens`.
- `LlmCallCompleted` events, turn aggregation, runtime metrics, HTTP daemon status, operator timeline payloads, and slash-command status now carry provider cache-read and cache-creation input token counts. `/status` presents context usage, cache read/write, and cumulative global/session token spend as separate lines.
- Journal startup now creates its database parent directory explicitly and
  retries transient SQLite open failures. Kernel atomic writes and channel
  pairing-state writes use unique temporary files before rename, which keeps
  release-gate concurrency and runtime persistence from colliding on fixed temp
  names.
- Executive refactor: durable prompts, system templates, bootstrap templates,
  worker prompts, batch/causal/summarization templates, metacognitive hints,
  default skills, tool descriptions, runtime policy rendering, retrieved
  evidence rendering, recalled-memory rendering, and untrusted tool-output
  wrapping were rewritten as responsibility-bound operating contracts.
- README and Executive docs now describe Cortex as one daemon-backed cognitive
  harness instance, with soul as the durable seed and runtime schemas as the
  authority for real capabilities.
- Bootstrap now initializes instance identity plus collaborator profile,
  work context, communication style, environment, autonomy boundaries, privacy
  boundaries, and approval expectations.
- Telegram streaming drafts, final answers, callbacks, and inline-keyboard
  messages share one Markdown-to-HTML rendering path.
- Long Telegram responses split by rendered size into independently renderable
  Markdown chunks, reject truncating edits, clean up stale drafts, and keep
  fallback sends free of visible Markdown syntax.
- The release has targeted tests for long Markdown, fenced code blocks,
  response-tail preservation, chunk-size limits, closed Markdown state,
  strong-marker streaming drafts, and untrusted evidence/tool-output wrapping.
- Runtime home protection now treats the Cortex instance directory as a
  protected root during foreground and sub-turn tool evaluation. Ordinary tools
  cannot access protected instance-state files, symlinked paths are resolved
  before policy evaluation, and process/script tools remain available through
  the normal permission gate unless an invocation directly targets protected
  runtime state.
- Process-isolated plugin tools are forced to expose
  `RunProcess:plugin subprocess` at load time, even when a plugin manifest
  omits the process capability. LLM-triggered plugin tool calls therefore enter
  the same effect preview, risk gate, protected-root policy, and approval path
  as built-in tools.
- Plugin tools that present prompt, config, session, journal, memory, or
  runtime-state mutation are denied as direct model-callable mutation paths
  under protected roots. Self-evolution plugins can return structured
  proposals, but applying those proposals belongs to checked PromptManager or
  runtime-command flows with linting, backups, atomic writes, and audit records.
- Trusted native plugin tool descriptors now preserve their per-tool declared
  effects. Package-level manifest capabilities remain install/review metadata
  rather than being copied onto every native tool. Trusted native plugins remain
  in-process trusted code rather than a sandbox; the release claim is governance
  and visibility, not containment against malicious native code.
- Status output now separates last-call context usage from cumulative token
  spend. Session metadata persists per-session input/output token totals, so
  `/status` can report global and current-session spend without presenting
  lower-value turn/daemon token counters as user-facing state.
- QQ now checks pairing before slash-command routing. An unpaired first
  `/status` message receives only the plain pairing prompt and does not attach
  the QQ command card/keyboard.
- ACP client support is connected as a first-class configured tool path:
  `[acp].clients` defines external agent processes, `acp_agent` performs
  initialize/session/prompt with timeout and JSON-RPC id validation, streamed
  ACP text is collected into the tool result, and ACP invocation/response
  events are written to the journal.
- Task persistence is connected to the daemon JSON-RPC surface. `task/create`,
  `task/list`, `task/get`, `task/delete`, `task/claim`, and `task/update` are
  actor-scoped, `task/claim` is atomic, and `task/update` enforces the task
  state machine instead of leaving task storage as an internal-only contract.
- Goal persistence is reintroduced as the connected runtime contract rather
  than the old unused JSON store. `goal/create`, `goal/list`, `goal/get`,
  `goal/delete`, and `goal/update` are actor-scoped; `goal/update` enforces
  the goal state machine; open goals are read from daemon-owned SQLite state
  and rendered into active turn context, while goal lifecycle changes continue
  to journal `GoalSet`, `GoalShifted`, and `GoalCompleted` events.

## Release Audit Requirements

Before `v1.5.11` can be cut:

1. Every **release blocker** row must be implemented or explicitly reclassified
   with code evidence, test evidence, documentation, and a limitation statement.
2. Every **partial** row must have an acceptance test proving the release claim
   or a public limitation that prevents overclaiming.
3. The release report must include the final state of this table, the strict
   Docker Compose gate result, and behavior/safety/replay/soak evidence.
4. No P2 claim may be promoted to README or release notes unless the relevant
   audit row is no longer partial or blocked.
