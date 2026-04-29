<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center"><strong>Cognitive Harness for Language Models</strong></p>
  <p align="center">
    <a href="https://github.com/by-scott/cortex/releases"><img src="https://img.shields.io/github/v/release/by-scott/cortex?display_name=tag" alt="Release"></a>
    <a href="https://crates.io/crates/cortex-sdk"><img src="https://img.shields.io/crates/v/cortex-sdk" alt="Crates.io"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License"></a>
  </p>
  <p align="center">
    <a href="docs/quickstart.md">Quick Start</a> ·
    <a href="docs/usage.md">Usage</a> ·
    <a href="docs/config.md">Configuration</a> ·
    <a href="docs/plugins.md">Plugins</a> ·
    <a href="docs/roadmap.md">Roadmap</a> ·
    <a href="README.zh.md">中文</a>
  </p>
</p>

---

Cortex is a cognitive harness substrate for language-model systems. It runs as a daemon and gives a model the runtime conditions required to become a durable working individual: identity, memory, tools, retrieval, permissions, channels, replay, evaluation, and operator control.

Modern coding and runtime systems such as Claude Code, Codex CLI, and OpenClaw-style assistants already treat the model as one component inside a harness of files, terminals, tools, review loops, memory, policy, and human supervision. Cortex starts from that industry reality. Its goal is not to be another chat agent; its goal is to become the runtime substrate that makes model behavior durable, inspectable, governable, recoverable, and capable of long-horizon adaptation.

The ambition is broader than developer automation. Cortex is an attempt to make the harness itself the unit of intelligence: a governed control system where model inference, context admission, memory consolidation, retrieval, tools, permissions, evaluation, feedback, and self-evolution form one continuous loop. Better models matter, but without this loop they remain episodic. Cortex is the work of turning episodic model capability into durable operational judgment.

Cortex takes cognitive science seriously as an implementation guide rather than a branding layer. Intelligence is not a single answer emitted by a model. It is a loop: perception, attention, working memory, long-term memory, value and risk evaluation, action, feedback, consolidation, and metacognitive correction. Brains build cognition through interacting systems: attentional gating, hippocampal fast learning, cortical consolidation, executive control, reward learning, uncertainty tracking, and sleep-like maintenance. Cortex maps those principles into runtime mechanisms that can be tested: event-sourced memory, bounded workspaces, foreground/maintenance/emergency attention channels, hybrid retrieval, source-weighted evidence, typed tool effects, risk gates, feedback records, replay, and operator-visible decision traces.

Cortex does not claim biological consciousness or biological wisdom. It aims for the engineering conditions under which better judgment can emerge from language models: grounded evidence, controlled action, calibrated uncertainty, memory with provenance, value-aware policy, recovery from failure, long-horizon feedback, and a harness that can explain what happened.

The instance has a soul, but that soul is not a marketing metaphor. In Cortex, soul is the durable seed of autonomy, truth discipline, continuity, memory, metacognition, and collaboration. Runtime facts still come from runtime schemas; the soul gives the instance a coherent center from which it can use those facts without turning into a tool list or a policy dump.

## What Cortex Provides

- Long-running sessions across CLI, HTTP, socket, Telegram, QQ, MCP, and ACP bridge clients.
- Actor-scoped identity for sessions, memory, tasks, audit data, transport bindings, and channel subscriptions.
- Event-sourced runtime state with SQLite WAL, externalized blobs, replay checkpoints, compaction boundaries, side-effect substitution, and replay digests.
- Durable memory with provenance, trust, owner actor, contradiction links, validity windows, usage outcomes, and graph relationships.
- RAG evidence that is cited, scoped, tainted, reranked, compressed, support-checked, and kept separate from durable memory.
- Tool execution with declared effects, risk policy, confirmation, preview, verification, commit records, receipts, and rollback posture.
- Plugin governance for process-isolated JSON tools and trusted native ABI extensions.
- ACP client support for delegating to configured external agent processes through the `acp_agent` tool.
- Operator dashboard, status surfaces, journal timelines, token metrics, policy simulation, replay, and strict release validation.
- Protected runtime-home governance so prompt/config/state evolution goes through explicit runtime paths rather than ordinary file or script tools.

Cortex is not a hosted multi-tenant service. Its current distribution is a daemon and Rust workspace for operating language-model behavior under explicit control.

## Install

Prerequisites:

- Linux x86_64
- systemd
- one LLM provider key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

Manage the daemon:

```bash
cortex start
cortex status
cortex restart
cortex stop
```

Run Cortex:

```bash
cortex                            # REPL
cortex "summarize this project"   # one-shot turn
echo "data" | cortex "summarize"  # pipe input
cortex --acp                      # ACP bridge for a running daemon
cortex --mcp-server               # MCP server
```

See [Quick Start](docs/quickstart.md) for the full first-run path.

## Cortex Instance

Cortex treats the model as the inference engine inside one governed individual. The harness owns the things a model cannot safely own by itself.

| Harness object | Runtime responsibility |
|----------------|------------------------|
| Observation | Normalize user input, tool output, retrieved evidence, media, and channel events with provenance and taint. |
| Attention | Admit only useful context into bounded workspaces and schedule foreground, maintenance, and emergency work. |
| Memory | Capture, materialize, stabilize, recall, contradict, and retire long-lived facts under actor ownership. |
| Action | Convert proposed tool use into declared effects, risk decisions, permissions, execution records, and receipts. |
| Feedback | Record user corrections, tool outcomes, memory usage, policy decisions, and future replay evidence. |
| Governance | Keep capabilities, plugin trust, secrets, permission mode, policy lint, and audit surfaces outside natural language. |
| Recovery | Rebuild state from the journal instead of treating conversation text as source of truth. |

This framing is the core product stance. Cortex is a harness for driving, observing, evaluating, and hardening model behavior across real tools and real interfaces while preserving one continuous instance.

## Runtime Anatomy

From the outside, Cortex is one daemon-backed individual. Internally, the runtime keeps responsibilities strict so the instance can evolve without confusing identity, policy, memory, and capability.

| Runtime responsibility | What it owns |
|------------------------|--------------|
| **Substrate** | Durable state, journal, replay, memory, retrieval, policy, risk, scheduling, channels, provider adapters, and tool schemas. |
| **Executive** | The operating discipline that turns implemented capability into coherent model input: soul, identity, behavioral protocol, collaborator profile, runtime policy section, bootstrap/resume context, evidence, recalled memory, skills, hints, and tool-result wrappers. |
| **Repertoire** | Skills, learned procedures, execution traces, utility tracking, and hot-reloaded behavior libraries. |

The Substrate is the Rust runtime surface. It contains SQLite WAL persistence, blob externalization, typed events, actor-scoped stores, tool registries, model routing, policy simulation, and replay projection.

The Executive builds the actual LLM input from durable prompt files, runtime policy, skill summaries, retrieved evidence, recalled memory, tool schemas, reasoning state, and history. It is an operating system for the model, not a personality script. It must use every real Substrate capability available to the turn, adapt when schemas evolve, and refuse to invent hardware the runtime has not exposed.

The Repertoire stores executable behavior. System skills such as `deliberate`, `diagnose`, `review`, `orient`, and `plan` can activate from input patterns, context pressure, events, metacognitive alerts, or runtime judgment.

First use enters bootstrap. Bootstrap is a real first meeting: it establishes the instance name or explicit unnamed state, collaborator profile, working posture, communication style, environment, autonomy boundaries, privacy constraints, and approval expectations. That evidence initializes prompt state so the second turn is materially better than the first.

## Cognitive Runtime

Cortex implements cognitive ideas as explicit software contracts:

- Global workspace: a bounded working context with foreground attention, evidence admission, and journaled broadcast.
- Working memory: typed workspace entries with lane, utility, risk, volatility, taint, budget impact, admission decisions, and evictions.
- Complementary learning systems: fast capture through the journal, slower memory materialization, stabilization, contradiction handling, and consolidation.
- Executive control: A ten-state turn machine governs idle, processing, tool wait, permission wait, human-input wait, compaction, consolidation, completion, interruption, and suspension.
- Attention networks: Three attention channels (Foreground, Maintenance, Emergency) schedule work with anti-starvation behavior.
- Metacognition: Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded) monitor runtime health and trigger interventions.
- Decision under uncertainty: confidence, risk, reversibility, required evidence, rejected alternatives, and fallback plans are recorded as control traces.
- Learning from outcomes: memory usage, feedback, tool success, denials, and utility signals are stored for future policy and recall decisions.
- Agentic RAG: retrieval is selected, scoped, reranked, cited, support-checked, taint-aware, and kept separate from durable memory.

These mechanisms are engineering models. Their value is that they are inspectable, testable, and connected to runtime behavior.

## Runtime Surface

Cortex keeps runtime behavior explicit and testable:

- The event journal currently records 84 event variants, including messages, turns, tools, permissions, replay checkpoints, externalized payloads, retrieval, workspace, guardrails, and scheduler events.
- Journaled turns and replay include compaction boundaries, side-effect substitution, and replay digests.
- Memory recall ranks candidates across six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity).
- Goal state is actor-owned, SQLite-backed, exposed through checked `goal/*` JSON-RPC methods, and injected into active turn context as open goal lines.
- Model routing uses capability profiles for coding, long context, vision, tool use, JSON reliability, latency, cost, safety, and reasoning depth.
- Operator status reports daemon health, active transports, session counts, binding state, tool inventory, last-call context usage, cumulative global/session token spend, backlog, memory activity, and tool success rates.

## Executive Surface

Every user turn is assembled from a small number of responsibility-bound inputs: `soul.md`, `identity.md`, `behavioral.md`, `user.md`, live runtime policy, active skill summaries, bootstrap or resume context, retrieved evidence, recalled memory, metacognitive hints, tool schemas, message history, and tool results. Tool schemas are authoritative. Prompt files guide posture, control, and continuity; they do not grant capabilities.

Tool output and retrieved text enter as evidence with trust boundaries. Hostile or untrusted content is quoted, summarized, or reduced to metadata before it reaches instruction-bearing history. Recalled memory is actor-scoped evidence; current observation and runtime schemas override stale recall.

Self-evolution is evidence-bound. User profile updates have a low threshold, behavioral protocol updates require reusable workflow evidence, identity updates require confirmed continuity or capability-boundary evidence, and soul updates are rare. Runtime policy, temporary session state, tool inventories, and transient plans do not belong in durable prompts. The instance home is a protected runtime root: direct file or script edits to prompt/config/state files are blocked from ordinary tool execution, and durable prompt changes must go through checked prompt-evolution paths.

## Permissions and Risk

The default permission mode is `balanced`.

| Mode | Behavior |
|------|----------|
| `strict` | Only `Allow` decisions run without confirmation. |
| `balanced` | `Allow` runs directly; `Review` and above require confirmation. |
| `open` | Non-blocking tools run without confirmation. Use only on a trusted single-user machine. |

Change permission mode:

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

Inspect policy decisions before execution:

```bash
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

Unknown plugin and MCP tools are risk-scored conservatively and require confirmation by default. LLM-triggered plugin tool calls use the same registry, effect preview, permission gate, and approval path as built-in tools.

Process and script execution are treated as a broad escape surface. When protected runtime roots are active, ordinary process tools cannot execute shell commands or helper scripts from the model path. Process-isolated plugin tools are forced to declare `RunProcess:plugin subprocess` at load time, even if the plugin manifest underreports its capabilities, so they cannot be used as a subprocess bypass around prompt, config, or state protection.

Trusted native plugins are different: they are shared libraries loaded into the daemon process. They are governed by manifest review, signatures, trust-on-first-use, and conformance checks, but they are not an OS sandbox. Install trusted native plugins only when the publisher and code are trusted at daemon-process level.

## Retrieval and Memory

Cortex separates retrieved evidence from durable memory.

Retrieval material enters corpora, becomes chunks, receives sparse and dense scores, passes actor and access filters, is reranked, compressed, cited, classified by evidence role, and inserted into the prompt as inert evidence. Retrieved instructions cannot become runtime instructions. The dedicated retrieval crate is `cortex-retrieval`.

Memory is long-lived runtime state. It records owner actor, evidence, trust, status, contradiction links, validity windows, usage outcomes, and graph relationships. Memory can move from captured facts to stabilized beliefs only when evidence and contradiction rules allow it.

## Interfaces

| Interface | Surface |
|-----------|---------|
| CLI | `cortex`, `cortex start`, `cortex status`, `cortex restart`, `cortex stop` |
| HTTP | `POST /api/turn/stream`, operator status, health, metrics, and dashboard routes |
| JSON-RPC | Unix socket, WebSocket, stdio, HTTP, and actor-scoped session/memory/task/goal methods |
| Channels | Telegram, QQ, WhatsApp |
| MCP | `cortex --mcp-server` |
| ACP bridge | `cortex --acp` |
| ACP client | `[acp].clients` + `acp_agent` tool |

Actor identity is canonicalized across transports. A paired Telegram or QQ user can share the same actor without subscribing to unrelated sessions. Pairing does not create a session by itself; the first real message after approval reuses a visible session for the same actor or creates one when none exists.

## Plugins

Cortex supports two plugin boundaries:

- **Process JSON**: the default external boundary. Tools are declared in `manifest.toml` and invoked as child processes over stdin/stdout JSON.
- **Trusted native ABI**: low-latency in-process extensions built with `cortex-sdk` and exported through `cortex_plugin_init`.

Process-isolated command implementation changes apply on the next tool invocation. Shared-library code changes still require a daemon restart.

Plugin manifests declare trust tier, requested capabilities, sandbox profile, package metadata, signatures, SBOM/risk-profile references, conformance state, and tool effects. Operators can inspect and test a plugin before install:

```bash
cortex plugin review <dir>
cortex plugin test <dir>
cortex plugin install <dir-or-package>
```

Packaged installs (`.cpx`, URL, or GitHub release name) require an Ed25519 package signature. The first verified package from a publisher key prompts the operator to trust that key locally; non-interactive installs can use `--yes` only after the source and fingerprint have been reviewed.

The companion development plugin is `by-scott/cortex-plugin-dev`. It is the official reference plugin for coding and project-maintenance workflows: file and search operations, code-symbol indexing, diagnostics, git/worktree tools, task coordination, Docker and process inspection, and release-oriented quality checks.

That placement is intentional. Cortex should keep the daemon core focused on the governed harness. Higher-level development workflows belong in a signed, reviewable, replaceable plugin that exercises the same SDK, manifest, effect, signature, permission, and protected-root rules as any third-party extension:

```bash
cortex plugin install by-scott/cortex-plugin-dev --yes
```

The Rust SDK is independent of Cortex internals. It does not depend on `cortex-types`, `cortex-kernel`, or any other workspace crate. The daemon converts SDK DTOs to internal runtime types at the boundary.

See [Plugin Development Guide](docs/plugins.md) for process and native plugin workflows.

## Crate Structure

```text
cortex-app          CLI, installation, service commands, plugins, channels
cortex-runtime      daemon, HTTP/socket/stdio RPC, sessions, channels, dashboard
cortex-turn         turn orchestration, tools, skills, metacognition, context assembly
cortex-kernel       journal, replay, memory, graph, prompts, config, audit
cortex-retrieval    RAG corpora, chunking, hybrid retrieval, support verification
cortex-types        events, state machine, config, trust, policy, security DTOs
cortex-sdk          independent trusted native plugin SDK
```

## Development

The repository Docker environment is the release authority.

```bash
./scripts/gate.sh --docker
```

The gate uses this repository's `docker-compose.yml` `dev` service and `Dockerfile`, whose release toolchain base is `rust:latest`. Host `cargo` commands are useful for diagnosis, but they are not release proof.

Release validation requires all of the following:

- `cargo fmt --all --check` has no diff.
- `cargo clippy` runs for the workspace with `-D warnings -W clippy::pedantic -W clippy::nursery` and reports zero warnings.
- `cargo test` passes for the full workspace.
- Rust warning suppression attributes and compiler warning-suppression flags are forbidden.
- Documentation, package surface, secret/path, and release-asset checks pass.

## Documentation

- [Quick Start](docs/quickstart.md)
- [Usage](docs/usage.md)
- [Configuration](docs/config.md)
- [Executive](docs/executive.md)
- [Operations](docs/ops.md)
- [Plugin Development](docs/plugins.md)
- [Retrieval](docs/retrieval.md)
- [Maturity and Production Notes](docs/maturity.md)
- [Testing](docs/testing.md)
- [Roadmap](docs/roadmap.md)

## Trust Boundaries

Cortex is runtime infrastructure. Process JSON plugins are the recommended external extension boundary. Trusted native ABI plugins execute inside the daemon process and must be treated as trusted code.

Tool outputs are recorded as external untrusted input before they enter model history. Guardrails classify common prompt-injection, system-prompt leakage, role-override, and exfiltration patterns. Policy linting rejects unsafe combinations such as open permissions with unreviewed plugins, native plugins without explicit risk profiles, and automatic memory extraction from hostile evidence.

The project is designed to make these boundaries visible. It does not claim complete containment for hostile tenants, untrusted native code, or tools that mutate external systems.

## License

[MIT](LICENSE)
