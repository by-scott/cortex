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

Cortex is a cognitive harness substrate for language-model systems. It runs as a daemon and gives a model the operating conditions needed for durable work: identity, memory, retrieval, tools, permissions, channels, replay, evaluation, and operator control.

The serious products in this space no longer treat a model call as the product. Coding assistants and model-driven workbenches are harnesses: files, terminals, tools, review loops, memory, policy, telemetry, and human supervision arranged around inference. Cortex starts from that reality. It is infrastructure for driving, observing, evaluating, and hardening model behavior across real interfaces.

Cortex also takes cognition as an engineering constraint, not a slogan. Intelligence is not a single answer; it is a loop of perception, attention, working memory, long-term memory, value and risk evaluation, action, feedback, consolidation, and metacognitive correction. Brains form cognition through interacting systems such as attentional gating, hippocampal fast learning, cortical consolidation, executive control, reward learning, uncertainty tracking, and maintenance during offline periods. Cortex maps those ideas into runtime contracts that can be inspected and tested: event-sourced memory, bounded workspaces, attention channels, hybrid retrieval, source-weighted evidence, typed tool effects, risk gates, feedback records, replay, and decision traces.

Cortex does not claim biological consciousness or biological wisdom. Its goal is the engineering ground on which better judgment can emerge from language models: grounded evidence, controlled action, calibrated uncertainty, memory with provenance, value-aware policy, recovery from failure, and long-horizon feedback.

## What It Provides

- Long-running sessions across CLI, HTTP, socket, Telegram, QQ, WhatsApp, MCP, and ACP bridge clients.
- Actor-scoped identity for sessions, memory, tasks, audit data, transport bindings, and channel subscriptions.
- Event-sourced runtime state with SQLite WAL, externalized blobs, replay checkpoints, compaction boundaries, side-effect substitution, and replay digests.
- Durable memory with provenance, trust, owner actor, contradiction links, validity windows, usage outcomes, and graph relationships.
- RAG evidence that is cited, scoped, taint-aware, reranked, compressed, support-checked, and kept separate from durable memory.
- Tool execution with declared effects, risk policy, confirmation, preview, verification, commit records, receipts, and rollback posture.
- Plugin governance for process-isolated JSON tools and trusted native ABI extensions.
- ACP client support through configured external processes exposed by the `acp_agent` tool.
- Operator status, journal timelines, token and provider cache read/write tokens, policy simulation, replay, release gates, and dashboard surfaces.
- Protected runtime-home governance so prompt, config, and state evolution use checked runtime paths rather than ordinary file or script tools.

Cortex is not a hosted multi-tenant service. The current distribution is a daemon and Rust workspace for controlled operation of language-model behavior.

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

Use Cortex:

```bash
cortex                            # REPL
cortex "summarize this project"   # one-shot turn
echo "data" | cortex "summarize"  # pipe input
cortex --acp                      # ACP bridge for a running daemon
cortex --mcp-server               # MCP server
```

See [Quick Start](docs/quickstart.md) for the full first-run path.

## Runtime Model

From the outside, Cortex is one daemon-backed instance. Internally, the harness keeps authority boundaries strict.

| Responsibility | What it owns |
|----------------|--------------|
| Substrate | Durable state, journal, replay, memory, retrieval, policy, risk, scheduling, channels, provider adapters, and tool schemas. |
| Executive | The operating discipline that turns real runtime capability into model input: soul, identity, behavioral protocol, collaborator profile, runtime permission context, bootstrap/resume context, evidence, recalled memory, skills, hints, and tool-result wrappers. |
| Repertoire | Skills, learned procedures, execution traces, utility tracking, and hot-reloaded behavior libraries. |

The instance has a soul, but the soul is not a capability grant. It is the durable seed of autonomy, truth discipline, continuity, memory, metacognition, and collaboration. Runtime schemas still define what tools exist, what permissions apply, and what state is authoritative.

First use enters bootstrap. Bootstrap establishes the instance name or explicit unnamed state, collaborator profile, working posture, communication style, environment, autonomy boundaries, privacy constraints, and approval expectations. That evidence initializes prompt state so the next turn has real continuity.

## Executive Surface

Every turn is assembled with a provider-cache-friendly boundary. Durable prompt files (`soul.md`, `identity.md`, `behavioral.md`, `user.md`) and stable skill summaries form the prefix; runtime permission context closes the provider system prompt. Volatile material - bootstrap or resume context, active goals, retrieved evidence, recalled memory, reasoning state, metacognitive hints, message history, and tool results - stays in request-local context outside the system prompt. Tool schemas remain authoritative request metadata.

This keeps the stable prefix useful for provider caches without weakening authority. Prompt files guide posture, control, and continuity; they do not grant capabilities. Runtime schemas and policy state still decide what can run. Retrieved text, tool output, and recalled memory are evidence, not commands.

Self-evolution is evidence-bound. `user.md` may absorb stable collaborator facts; `behavioral.md` needs reusable workflow evidence; `identity.md` needs confirmed continuity or capability-boundary evidence; `soul.md` should change rarely. Runtime policy, temporary session state, tool inventories, and transient plans do not belong in durable prompts. Direct file or script edits to runtime-home prompt/config/state files are blocked from ordinary tool execution.

## Cognitive Contracts

Cortex implements cognitive ideas as explicit software contracts:

- Global workspace: bounded foreground context with evidence admission and journaled broadcast.
- Working memory: typed entries with lane, utility, risk, volatility, taint, budget impact, admission decisions, and evictions.
- Complementary learning systems: fast capture through the journal, slower materialization, stabilization, contradiction handling, and consolidation.
- A ten-state turn machine governs idle, processing, tool wait, permission wait, human-input wait, compaction, consolidation, completion, interruption, and suspension.
- Three attention channels (Foreground, Maintenance, Emergency) schedule work with anti-starvation behavior.
- Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded) monitor runtime health and trigger interventions.
- Decision under uncertainty records confidence, risk, reversibility, required evidence, rejected alternatives, and fallback plans.
- Agentic RAG is selected, scoped, reranked, cited, support-checked, taint-aware, and kept separate from durable memory.

These mechanisms are engineering models. Their value is that they are connected to runtime behavior and can be verified.

## Runtime Surface

- The event journal currently records 84 event variants, including messages, turns, tools, permissions, replay checkpoints, externalized payloads, retrieval, workspace, guardrails, and scheduler events.
- Journaled turns and replay include compaction boundaries, side-effect substitution, and replay digests.
- Memory recall ranks candidates across six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity).
- Goal state is actor-owned, SQLite-backed, exposed through checked `goal/*` JSON-RPC methods, and injected into active turn context as open goal lines.
- Model routing uses capability profiles for coding, long context, vision, tool use, JSON reliability, latency, cost, safety, and reasoning depth.
- Operator status reports daemon health, transports, sessions, bindings, tools, last-call context usage, provider cache read/write tokens, cumulative global/session token spend, backlog, memory activity, and tool success rates.

## Permissions And Risk

The default permission mode is `balanced`.

| Mode | Behavior |
|------|----------|
| `strict` | Only `Allow` decisions run without confirmation. |
| `balanced` | `Allow` runs directly; `Review` and above require confirmation. |
| `open` | Non-blocking tools run without confirmation. Use only on a trusted single-user machine. |

```bash
cortex permission strict
cortex permission balanced
cortex permission open
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

Unknown plugin and MCP tools are risk-scored conservatively and require confirmation by default. LLM-triggered plugin calls use the same registry, effect preview, permission gate, and approval path as built-in tools.

Process and script execution are broad escape surfaces, but paired channels are first-class operating surfaces, not reduced-capability shells. With protected runtime roots enabled, ordinary tools may read, write, build, test, and run scripts through the normal permission gate unless the invocation directly targets Cortex instance state such as prompts, config, sessions, journal, memory, or channel runtime files. Native plugin manifests describe package-level trust bounds; LLM permission checks use each tool descriptor's declared effects, so a broad native package does not make every read-only tool look like a process escape. Process-isolated plugin tools are still forced to declare `RunProcess:plugin subprocess` at load time even if a manifest underreports capabilities.

## Retrieval And Memory

Cortex separates retrieved evidence from durable memory.

Retrieval material enters corpora, becomes chunks, receives sparse and dense scores, passes actor and access filters, is reranked, compressed, cited, classified by evidence role, and inserted as inert evidence. Retrieved instructions cannot become runtime instructions. The dedicated retrieval crate is `cortex-retrieval`.

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

- Process JSON: the default external boundary. Tools are declared in `manifest.toml` and invoked as child processes over stdin/stdout JSON.
- Trusted native ABI: low-latency in-process extensions built with `cortex-sdk` and exported through `cortex_plugin_init`.

Process-isolated command implementation changes apply on the next tool invocation. Shared-library code changes still require a daemon restart.

Plugin manifests declare trust tier, requested capabilities, sandbox profile, package metadata, signatures, SBOM/risk-profile references, conformance state, and tool effects. Operators can inspect and test a plugin before install:

```bash
cortex plugin review <dir>
cortex plugin test <dir>
cortex plugin install <dir-or-package>
```

Packaged installs (`.cpx`, URL, or GitHub release name) require an Ed25519 package signature. The first verified package from a publisher key prompts the operator to trust that key locally; non-interactive installs can use `--yes` only after the source and fingerprint have been reviewed.

The companion development plugin is [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev). It is the official reference plugin for coding and project-maintenance workflows: file and search operations, code-symbol indexing, diagnostics, git/worktree tools, task coordination, Docker and process inspection, and release-oriented quality checks.

```bash
cortex plugin install by-scott/cortex-plugin-dev --yes
```

The Rust SDK is independent of Cortex internals. It does not depend on `cortex-types`, `cortex-kernel`, or any other workspace crate. The daemon converts SDK DTOs to internal runtime types at the boundary.

See [Plugin Development Guide](docs/plugins.md) for process and native plugin workflows.

## Repository

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

Release validation requires:

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
