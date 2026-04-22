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
    <a href="README.zh.md">中文</a>
  </p>
</p>

---

Modern agent frameworks have brought language models remarkably far — persistent memory, tool orchestration, multi-step planning, and context management are increasingly mature capabilities across the ecosystem. Cortex takes a complementary approach: rather than assembling these capabilities ad hoc, it derives them systematically from cognitive science first principles.

Global Workspace Theory shapes the concurrency model. Complementary Learning Systems govern memory consolidation. Metacognitive conflict monitoring becomes a first-class subsystem with self-tuning thresholds, not a logging layer. Drift-diffusion evidence accumulation replaces ad hoc confidence heuristics. Cognitive load theory drives graduated context pressure response. Each principle is implemented as a type-level architectural constraint in Rust — not as metaphor, but as structure the compiler enforces.

The result is a runtime in which a language model can sustain coherent, self-correcting, goal-directed behavior across time, across interfaces, and under pressure — with every design decision traceable to peer-reviewed theory.

## Architecture

Cortex separates cognition into three layers with distinct lifecycles:

| Layer | Name | Substance |
|-------|------|-----------|
| **Substrate** | Cognitive Hardware | Rust type system + persistence + cognitive subsystems |
| **Executive** | Execution Protocol | Prompt system + metacognition protocol + system templates |
| **Repertoire** | Behavioral Library | Skills + learned patterns + utility tracking |

### Substrate

The foundation encoded in Rust's type system. An event-sourced journal records every cognitive act as one of 72 event variants with deterministic replay capability. A ten-state turn machine governs lifecycle transitions. Memory flows through a three-stage pipeline (Captured → Materialized → Stabilized) with trust tiers, temporal decay, and graph relationships; recall ranks candidates across six weighted dimensions (BM25, cosine similarity, recency, status, access frequency, graph connectivity). Five metacognitive detectors (DoomLoop, Duration, Fatigue, FrameAnchoring, HealthDegraded) monitor reasoning health with Gratton-adaptive thresholds. A drift-diffusion confidence model accumulates evidence across turns. Three attention channels (Foreground, Maintenance, Emergency) schedule work with anti-starvation guarantees. Goals organize into strategic, tactical, and immediate tiers. Risk assessment scores four axes with depth-scaled delegation.

### Executive

The Executive is the operating system that drives the Substrate. It is not a second hardware description and not a tool catalog; it is the policy for using whatever capabilities the runtime actually exposes. Four prompt layers have separate responsibilities and change rates:

- **Soul** — Sacred seed: continuity, values, epistemology, autonomy, and relationship to the collaborator. It changes only through sustained experience.
- **Identity** — Self-model: name, substrate awareness, capability boundaries, memory model, channels, and evolution posture. Runtime schemas override stale self-description.
- **Behavioral** — Operating protocol: sense-plan-execute-verify-reflect, metacognition response, context pressure, risk, delegation, communication, and adaptation.
- **User** — Collaborator model: identity, work, expertise, communication, environment, autonomy, boundaries, and durable corrections.

The actual LLM request combines these layers with active skill summaries, situational bootstrap or resume context, recalled memory, reasoning state, metacognitive hints, tool schemas, and message history. The Executive is designed to remain valid as the Substrate evolves: new tools, providers, channels, and plugins are discovered from runtime schemas before they are reflected in durable prompts.

### Repertoire

An independent behavioral library with its own learning cycle. Five system skills — `deliberate`, `diagnose`, `review`, `orient`, `plan` — encode cognitive strategies as executable SKILL.md programs. Skills activate through five paths: input pattern matching, context pressure threshold, metacognitive alert, event trigger, or autonomous judgment. Each skill tracks its own utility via EWMA scoring. The Repertoire evolves independently of the Executive: tool-call pattern detection discovers new skill candidates, utility evaluation prunes weak performers, and materialization writes instance skills to disk for hot-reload into the live registry. Layering is system / instance / plugin: system skills define the cognitive base, instance skills specialize a running instance, and plugin skills ship domain capabilities with their plugin.

## Cognitive Foundations

| Theory | Implementation | Source |
|--------|---------------|--------|
| Global Workspace [Baars] | Exclusive foreground turn + journal broadcast | `orchestrator.rs` |
| Complementary Learning Systems [McClelland] | Captured → Materialized → Stabilized | `memory/` |
| ACC Conflict Monitoring [Botvinick] | Five detectors + Gratton adaptive thresholds | `meta/` |
| Drift-Diffusion Model [Ratcliff] | Fixed-delta evidence accumulation | `confidence/` |
| Reward Prediction Error [Schultz] | EWMA tool utility + UCB1 explore-exploit | `meta/rpe.rs` |
| Prefrontal Hierarchy [Koechlin] | Strategic / tactical / immediate goals | `goal_store.rs` |
| Cognitive Load Theory [Sweller] | 7-region workspace + 5-level pressure | `context/` |
| Default Mode Network [Raichle] | DMN reflection + 30-min maintenance | `orchestrator.rs` |
| ACT-R Production Rules | System / instance / plugin skills + SOAR chunking | `skills/` |

## Crate Structure

```
cortex-app          CLI modes · install · auth · plugins
    │
cortex-runtime      Daemon (HTTP/socket/stdio) · JSON-RPC · sessions · multi-instance · maintenance
    │
cortex-turn         SN→TPN→DMN · dynamic tools · skills · metacognition · 7-region workspace
    │
cortex-kernel       Journal (WAL) · memory + graph · prompts · embedding
    │
cortex-types        72 events · 10-state machine · config · trust · security

cortex-sdk          Plugin development kit — zero-dependency public API for native plugins
```

## Getting Started

**Prerequisites:** Linux x86_64 · systemd · one LLM provider key

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install
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

Actor identity maps across transports — `telegram:id` and `http` resolve to the same `user:name`.

Streaming clients receive token-level user-visible text and a final structured `done` event. Telegram edits a live draft bubble and replaces it with the final response. QQ follows the platform's reply model and delivers complete final replies without an extra Cortex-generated processing bubble. Cross-client channel subscription is explicit, per paired user, and disabled by default. Pairing prompts show both administrative choices: `cortex channel approve <platform> <user_id>` and `cortex channel approve <platform> <user_id> --subscribe`. Enable subscription later with `cortex channel subscribe <platform> <user_id>`; disable it with `cortex channel unsubscribe <platform> <user_id>`. When enabled for a QQ user, subscribed broadcasts suppress incremental text and deliver only the final message.

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

Extended at runtime via MCP servers and native plugins.

## Plugins

Native FFI via `cortex-sdk`. Plugins contribute tools, skills, prompt layers, and structured media attachments with zero dependency on Cortex internals. See [Plugin Development Guide](docs/plugins.md) for the complete walkthrough from scaffold to distribution.

### [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev)

The official development plugin. Turns Cortex into a full coding agent — comparable to tools like Claude Code, Codex, and OpenCode, but running on the cognitive runtime's Substrate with metacognition, memory consolidation, and self-evolving skills.

42 native tools and 13 workflow skills: safe file read/write/replace, project mapping, test discovery, dependency manifest audit, secret scanning, quality gate reporting, file search (glob, grep), cached tree-sitter code intelligence (Rust, Python, TypeScript, TSX symbols, imports, definitions, references, hover), git integration (status, diff, log, commit, worktree isolation), task management with dependency tracking, language diagnostics (cargo, clippy, pyright, mypy, tsc, eslint), REPL (Python, Node.js), SQLite queries, HTTP client, Docker operations, process inspection, Jupyter notebook editing, and multi-agent team coordination.

13 workflow skills: `commit`, `review-pr`, `simplify`, `test`, `create-pr`, `explore`, `debug`, `implement`, `refactor`, `release`, `incident`, `security`, `context-budget` — each activating on natural language patterns and guiding structured multi-step workflows.

```bash
cortex plugin install by-scott/cortex-plugin-dev
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
- **[Usage](docs/usage.md)** — CLI modes, HTTP, JSON-RPC, sessions
- **[Configuration](docs/config.md)** — Layout, providers, hot reload
- **[Executive](docs/executive.md)** — Prompt layers, bootstrap, skills, LLM input surface
- **[Operations](docs/ops.md)** — Lifecycle, channels, diagnostics
- **[Plugin Development](docs/plugins.md)** — From scaffold to distribution

## License

[MIT](LICENSE)
