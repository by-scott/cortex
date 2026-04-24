<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center"><strong>Cognitive Runtime for Language Models</strong></p>
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

Modern agent frameworks have brought language models remarkably far — persistent memory, tool orchestration, multi-step planning, and context management are increasingly mature capabilities across the ecosystem. Cortex takes a complementary approach: rather than assembling these capabilities ad hoc, it organizes them around cognitive-science-inspired runtime constraints.

Global Workspace Theory shapes the concurrency model. Complementary Learning Systems inform memory consolidation. Metacognitive conflict monitoring becomes a first-class subsystem with self-tuning thresholds, not a logging layer. Drift-diffusion evidence accumulation is approximated as a bounded confidence tracker. Cognitive load theory drives graduated context pressure response. These are engineering implementations inspired by the theories, not formal cognitive-science models.

The result is a runtime intended to help a language model sustain coherent, self-correcting, goal-directed behavior across time, across interfaces, and under pressure, while keeping the major runtime mechanisms explicit and inspectable.

## What Cortex Is

The shortest accurate description is:

> Cortex is a long-running local agent runtime, closer to an agent OS substrate than a prompt loop framework.

That difference shows up in three places:

- **State is durable.** Turns, memory, tool calls, confirmations, compaction boundaries, and replay inputs are journaled.
- **Identity is continuous.** Sessions, memory, and actor ownership can persist across CLI, HTTP, Telegram, QQ, and other transports.
- **Control is explicit.** Risk gates, turn states, replay behavior, plugin boundaries, and operator actions are first-class runtime mechanisms rather than prompt folklore.

## Architecture

Cortex organizes cognition across three cooperating planes. They describe responsibilities, not separate identities:

| Plane | Name | Substance |
|-------|------|-----------|
| **Substrate** | Cognitive Hardware | Rust type system + persistence + cognitive subsystems |
| **Executive** | Execution Protocol | Prompt system + metacognition protocol + system templates |
| **Repertoire** | Behavioral Library | Skills + learned patterns + utility tracking |

### Substrate

The foundation encoded in Rust's type system. An event-sourced journal records every cognitive act as one of 74 event variants with deterministic replay capability. A ten-state turn machine governs lifecycle transitions. Memory flows through a three-stage pipeline (Captured → Materialized → Stabilized) with trust tiers, temporal decay, and graph relationships; recall ranks candidates across six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity). Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded) monitor reasoning health with Gratton-adaptive thresholds. A drift-diffusion confidence model accumulates evidence across turns. Three attention channels (Foreground, Maintenance, Emergency) schedule work with anti-starvation guarantees. Goals organize into strategic, tactical, and immediate tiers. Risk assessment scores four axes with depth-scaled delegation.

### Executive

The Executive is Cortex's operating discipline: prompts, templates, hints, and skills that turn implemented capabilities into coherent action. It is not a second hardware description and not a tool catalog; runtime schemas remain the source of truth. Four durable prompt files have separate responsibilities and change rates:

- **Soul** — Origin of autonomy and cognition: continuity, attention, judgment, truth discipline, and collaboration. It changes only through profound tested experience.
- **Identity** — Self-model: name, continuity, capability boundaries, memory model, channels, and evolution posture. Runtime schemas override stale self-description.
- **Behavioral** — Operating protocol: sense-plan-execute-verify-reflect, metacognition response, context pressure, risk, delegation, communication, and adaptation.
- **User** — Collaborator model: identity, work, expertise, communication, environment, autonomy, boundaries, and durable corrections.

Normal user turns assemble the LLM request from these prompt files, runtime policy context, active skill summaries, situational bootstrap or resume context, recalled memory, reasoning state, tool schemas, and message history.

### Repertoire

An independent behavioral library with its own learning cycle. Five system skills — `deliberate`, `diagnose`, `review`, `orient`, `plan` — encode cognitive strategies as executable SKILL.md programs. Skills activate through five paths: input pattern matching, context pressure threshold, metacognitive alert, event trigger, or autonomous judgment. Each skill tracks its own utility via EWMA scoring. The Repertoire evolves independently of the Executive: tool-call pattern detection discovers new skill candidates, utility evaluation prunes weak performers, and materialization writes instance skills to disk for hot-reload into the live registry.

## Runtime Guarantees

The practical value of Cortex is not just feature count. It is the fact that the runtime surface is explicit:

- **Journaled turns and replay** — compaction boundaries, side-effect substitution, and replay digests are part of the system design.
- **Typed turn states** — tool wait, permission wait, human input, compaction, completion, interruption, and suspension are modeled states.
- **Scoped ownership** — sessions, memories, tasks, and audit visibility follow canonical actor ownership.
- **Operator control** — permission modes, explicit confirmations, `/stop`, status, and plugin/channel toggles are runtime operations, not prompt conventions.

## Permissions and Risk

The default permission mode is `balanced`.

- `strict` — only `Allow` runs without confirmation.
- `balanced` — `Allow` runs directly; `Review` and above require confirmation.
- `open` — all non-blocking tools run without confirmation. Use this only on a strongly trusted single-user machine.

You can set the mode at install time:

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

Or switch it live later:

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

Interactive confirmations remain pending until someone approves, denies, or stops the turn. They no longer auto-deny simply because time elapsed.

## Getting Started

**Prerequisites:** Linux x86_64 · systemd · one LLM provider key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

```bash
cortex                            # REPL
cortex "hello"                    # Single prompt
echo "data" | cortex "summarize"  # Pipe
cortex --mcp-server               # MCP server
```

For the complete first-run path, see the [Quick Start](docs/quickstart.md).

On first launch, a bootstrap conversation establishes mutual identity, collaborator profile, and working agreements.

<details>
<summary><strong>Build from source</strong></summary>

```bash
docker compose run --rm dev cargo build --release
./target/release/cortex install
```
</details>

## Interfaces

| | |
|---|---|
| CLI | `cortex` |
| HTTP | `POST /api/turn/stream` |
| JSON-RPC | Unix socket · WebSocket · stdio · HTTP |
| Telegram | `cortex channel pair telegram` |
| WhatsApp | `cortex channel pair whatsapp` |
| QQ | `cortex channel pair qq` |
| MCP | `cortex --mcp-server` |
| ACP | `cortex --acp` |

Actor identity maps across transports — `telegram:id`, `qq:id`, `http`, and local transports can resolve to the same canonical actor.

Streaming clients receive token-level user-visible text and a final structured `done` event. Telegram edits a live draft bubble and replaces it with the final response. QQ follows the platform's reply model and delivers complete final replies without an extra Cortex-generated processing bubble.

Telegram and QQ now prefer card-style interaction for `/help`, `/status`, `/permission`, `/session`, and `/config` where the platform supports it. `/stop` resolves against the active actor session, interrupts the current turn, and clears any pending confirmations for that turn.

Cross-client channel subscription is explicit, per paired user, and disabled by default. Pairing itself does not create a session. After approval, the first real message from that client reuses an existing visible session for the same canonical actor when one already exists; otherwise Cortex creates a fresh session for that client at that point. Pairing prompts show both administrative choices:

```bash
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
```

Enable or disable it later with:

```bash
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
```

These subscribe/unsubscribe changes hot-apply without a daemon restart. Subscription follows that paired client's currently active session only; it does not mirror unrelated sessions owned by the same canonical actor. When enabled for a QQ user, subscribed broadcasts suppress incremental text and deliver only the final message.

## Tools

| Category | Tools |
|----------|-------|
| File I/O | `read` · `write` · `edit` |
| Execution | `bash` |
| Memory | `memory_search` · `memory_save` |
| Web | `web_search` · `web_fetch` |
| Media | `tts` · `image_gen` · `video_gen` · `send_media` |
| Delegation | `agent` (readonly / full / fork / teammate) |
| Scheduling | `cron` |

Extended at runtime via MCP servers and plugins.

## Plugins

Cortex supports two plugin boundaries:

- **Process JSON** — the default external boundary. Plugins are manifest-declared child-process tools invoked over stdin/stdout JSON. Manifest and tool-set changes hot-apply without a daemon restart.
- **Trusted native ABI** — low-latency in-process extensions built against `cortex-sdk`. They export `cortex_plugin_init` through a stable native ABI. Shared-library code changes still require a daemon restart.

Both boundaries can contribute tools, skills, prompt layers, and structured media attachments.

Local installation supports both packed `.cpx` archives and plugin directories. Directory installs copy only supported plugin assets and automatically extract the built native library into `lib/` when the manifest declares one.

See [Plugin Development Guide](docs/plugins.md) for the complete walkthrough from scaffold to distribution.

### [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev)

The official development plugin. Turns Cortex into a full coding agent — comparable to tools like Claude Code, Codex, and OpenCode, with the cognitive runtime's Substrate providing metacognition, memory consolidation, and self-evolving skills.

42 native tools and 13 workflow skills: safe file read/write/replace, project mapping, test discovery, dependency manifest audit, secret scanning, quality gate reporting, file search (glob, grep), cached tree-sitter code intelligence (Rust, Python, TypeScript, TSX symbols, imports, definitions, references, hover), git integration (status, diff, log, commit, worktree isolation), task management with dependency tracking, language diagnostics (cargo, clippy, pyright, mypy, tsc, eslint), REPL (Python, Node.js), SQLite queries, HTTP client, Docker operations, process inspection, Jupyter notebook editing, and multi-agent team coordination.

13 workflow skills: `commit`, `review-pr`, `simplify`, `test`, `create-pr`, `explore`, `debug`, `implement`, `refactor`, `release`, `incident`, `security`, `context-budget`.

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

## Maturity and Trust Boundaries

Cortex is an early runtime with a large architectural surface. Event sourcing, replay, memory evolution, hot reload, multi-interface identity, plugins, and risk gates are implemented, but they have not yet had the long soak time expected from mature production infrastructure.

Important boundaries:

- Cognitive-science terms describe engineering inspiration. The implementations are practical approximations such as schedulers, thresholds, confidence scores, and consolidation heuristics.
- Process JSON is the default external plugin boundary. Trusted native ABI plugins are strong-trust in-process extensions, not a sandbox.
- Unknown plugin and MCP tools are risk-scored conservatively and require confirmation by default. Production deployments should add explicit `[risk.tools.<name>]` policies instead of relying only on generic scoring.
- Tool outputs are recorded as external untrusted input and wrapped before entering LLM history so web/file/plugin results are treated as evidence, not instructions.
- Guardrails return structured categories for common prompt-injection, leakage, role-override, and exfiltration patterns, and guardrail hits are journaled.
- Deterministic replay substitutes recorded or provider-supplied side-effect values during projection and exposes a stable replay digest for comparing equivalent runs.
- Session, task, audit, and long-term memory visibility are scoped by canonical actor; `local:default` remains the local administrator actor.

Not yet:

- No sandbox around trusted native plugins.
- No complete adversarial defense for prompt injection beyond provenance wrapping, structured guardrails, and audit events.
- No full containment for tools that mutate external systems.

See [Maturity and Production Notes](docs/maturity.md) for the fuller assessment and [Roadmap Review](docs/roadmap.md) for the staged follow-up priorities.

## Crate Structure

```text
cortex-app          CLI modes · install · auth · plugins
    │
cortex-runtime      Daemon (HTTP/socket/stdio) · JSON-RPC · sessions · multi-instance · maintenance
    │
cortex-turn         SN→TPN→DMN · dynamic tools · skills · metacognition · context builder
    │
cortex-kernel       Journal (WAL) · memory + graph · prompts · embedding
    │
cortex-types        events · state machine · config · trust · security

cortex-sdk          Trusted native plugin SDK
```

## Stack

| | |
|---|---|
| Rust | edition 2024 |
| Storage | SQLite WAL + blob externalization |
| Async | Tokio |
| HTTP | Axum · tower-http |
| Protocol | JSON-RPC 2.0 |
| LLM | Anthropic · OpenAI · Ollama (9 providers) |
| Parsing | tree-sitter |
| Plugins | libloading |

## Development

```bash
docker compose run --rm dev cargo test --workspace
docker compose run --rm dev cargo clippy --workspace --all-targets --all-features -- \
  -D warnings -W clippy::pedantic -W clippy::nursery
```

## Documentation

- **[Quick Start](docs/quickstart.md)** — Install, first run, common commands
- **[Usage](docs/usage.md)** — CLI modes, HTTP, JSON-RPC, sessions, channel interaction
- **[Configuration](docs/config.md)** — Layout, providers, permission modes, hot reload
- **[Executive](docs/executive.md)** — Prompt layers, runtime policy context, bootstrap, skills
- **[Operations](docs/ops.md)** — Lifecycle, channels, diagnostics
- **[Plugin Development](docs/plugins.md)** — From scaffold to distribution
- **[Testing](docs/testing.md)** — Test layout and required local gates
- **[Maturity](docs/maturity.md)** — Production readiness, trust boundaries, hardening backlog
- **[Roadmap](docs/roadmap.md)** — 1.3 / 1.4 / 1.5 staged priorities

## License

[MIT](LICENSE)
