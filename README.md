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
    <a href="docs/compatibility.md">Compatibility</a> ·
    <a href="docs/roadmap.md">Roadmap</a> ·
    <a href="README.zh.md">中文</a>
  </p>
</p>

---

Cortex is a local-first cognitive harness for language-model systems. It runs as a daemon and gives model-facing applications a controlled surface for durable state, actor-scoped identity, tool execution, memory, retrieval, channel delivery, policy, replay, and operator visibility.

The broader ecosystem has already converged on harnesses: mature coding assistants and operating runtimes combine models with tools, files, terminals, memory, review loops, and policy. Cortex takes that direction into a local daemon for operators who need to evaluate, exercise, and harden model behavior with inspection, replay, actor scoping, and auditable control across real tools and interfaces.

Cortex uses cognitive-science terms as names for implemented runtime mechanisms. Global Workspace Theory informs scheduling and attention. Complementary Learning Systems informs memory consolidation. Conflict monitoring, drift-diffusion confidence, and cognitive-load handling are implemented as thresholds, evidence accumulators, context-budget controls, and scheduler decisions. These mechanisms are engineering models, not claims about biological cognition.

## What Cortex provides

- Long-running sessions across CLI, HTTP, socket, Telegram, QQ, MCP, and ACP.
- Actor-scoped identity for sessions, memory, tasks, audit data, and channel bindings.
- Durable memory with provenance, trust, ownership, contradiction links, usage outcomes, and graph relationships.
- RAG evidence that is cited, scoped, tainted, reranked, compressed, and kept separate from long-term memory.
- Tool execution with declared effects, risk policy, confirmation, preview, verification, and commit records.
- Plugin governance for process-isolated JSON tools and trusted native ABI extensions.
- Replay, audit, operator dashboard, timeline inspection, and strict release validation.

Cortex is not a hosted multi-tenant platform. It is a local daemon and Rust workspace for operating language-model behavior under explicit control.

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
cortex --mcp-server               # MCP server
```

See [Quick Start](docs/quickstart.md) for the full first-run path.

## Architecture

Cortex is organized into three planes.

| Plane | Responsibility |
|-------|----------------|
| **Substrate** | Durable runtime state, journal, replay, memory, retrieval, policy, risk, and scheduling. |
| **Executive** | Prompt assembly, runtime policy context, metacognitive protocol, bootstrap/resume context, and skill activation. |
| **Repertoire** | Skills, learned patterns, execution traces, utility tracking, and hot-reloaded behavior libraries. |

The Substrate is the Rust runtime surface. It contains SQLite WAL persistence, blob externalization, typed events, actor-scoped stores, tool registries, model routing, policy simulation, and replay projection.

The Executive builds model input from durable prompt files, runtime policy, skill summaries, retrieved evidence, recalled memory, tool schemas, reasoning state, and history. Runtime schemas remain the source of truth for capabilities.

The Repertoire stores executable behavior. System skills such as `deliberate`, `diagnose`, `review`, `orient`, and `plan` can activate from input patterns, context pressure, events, metacognitive alerts, or runtime judgment.

## Runtime surface

Cortex keeps runtime behavior explicit and testable:

- The event journal currently records 87 event variants, including messages, turns, tools, permissions, replay checkpoints, externalized payloads, retrieval, workspace, guardrails, and scheduler events.
- A ten-state turn machine governs idle, processing, tool wait, permission wait, human-input wait, compaction, consolidation, completion, interruption, and suspension.
- Journaled turns and replay include compaction boundaries, side-effect substitution, and replay digests.
- Memory recall ranks candidates across six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity).
- Three attention channels (Foreground, Maintenance, Emergency) schedule work with anti-starvation behavior.
- Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded) monitor runtime health.
- Workspace admission records lane, utility, risk, volatility, taint, marginal utility, budget impact, admission decisions, and evictions.
- Model routing uses capability profiles for coding, long context, vision, tool use, JSON reliability, latency, cost, safety, and reasoning depth.

## Permissions and risk

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

Unknown plugin and MCP tools are risk-scored conservatively and require confirmation by default.

## Retrieval and memory

Cortex separates retrieved evidence from durable memory.

Retrieval material enters corpora, becomes chunks, receives sparse and dense scores, passes actor and access filters, is reranked, compressed, cited, classified by evidence role, and inserted into the prompt as inert evidence. Retrieved instructions cannot become runtime instructions. The dedicated retrieval crate is `cortex-retrieval`.

Memory is long-lived runtime state. It records owner actor, evidence, trust, status, contradiction links, validity windows, usage outcomes, and graph relationships. Memory can move from captured facts to stabilized beliefs only when evidence and contradiction rules allow it.

## Interfaces

| Interface | Surface |
|-----------|---------|
| CLI | `cortex`, `cortex start`, `cortex status`, `cortex restart`, `cortex stop` |
| HTTP | `POST /api/turn/stream`, operator status, health, metrics, and dashboard routes |
| JSON-RPC | Unix socket, WebSocket, stdio, and HTTP |
| Channels | Telegram, QQ, WhatsApp |
| MCP | `cortex --mcp-server` |
| ACP | `cortex --acp` |

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

The Rust SDK is independent of Cortex internals. It does not depend on `cortex-types`, `cortex-kernel`, or any other workspace crate. The daemon converts SDK DTOs to internal runtime types at the boundary.

See [Plugin Development Guide](docs/plugins.md) for process and native plugin workflows.

## Crate structure

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
- [Compatibility Policy](docs/compatibility.md)
- [Testing](docs/testing.md)
- [Roadmap](docs/roadmap.md)

## Trust boundaries

Cortex is local-first infrastructure. Process JSON plugins are the recommended external extension boundary. Trusted native ABI plugins execute inside the daemon process and must be treated as trusted code.

Tool outputs are recorded as external untrusted input before they enter model history. Guardrails classify common prompt-injection, system-prompt leakage, role-override, and exfiltration patterns. Policy linting rejects unsafe combinations such as open permissions with unreviewed plugins, native plugins without explicit risk profiles, and automatic memory extraction from hostile evidence.

The project is designed to make these boundaries visible. It does not claim complete containment for hostile tenants, untrusted native code, or tools that mutate external systems.

## License

[MIT](LICENSE)
