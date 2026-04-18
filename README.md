<p align="center">
  <h1 align="center">Cortex</h1>
  <p align="center">Persistent memory · Self-evolving identity · Metacognitive self-awareness</p>
  <p align="center">A cognitive runtime that makes AI agents dramatically better over long-term use.</p>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License: MIT"></a>
  <a href="https://www.rust-lang.org"><img src="https://img.shields.io/badge/rust-1.82%2B-orange.svg" alt="Rust 1.82+"></a>
  <a href="https://github.com/by-scott/cortex/releases"><img src="https://img.shields.io/github/v/release/by-scott/cortex?color=green" alt="Release"></a>
  <a href="https://github.com/by-scott/cortex/actions"><img src="https://img.shields.io/github/actions/workflow/status/by-scott/cortex/ci.yml?label=CI" alt="CI"></a>
  <a href="docs/quickstart.md"><img src="https://img.shields.io/badge/docs-quickstart-brightgreen.svg" alt="Quick Start"></a>
</p>

<p align="center">
  <a href="README.md">English</a> · <a href="README.zh.md">中文</a>
</p>

---

## Why Cortex

Most agent frameworks today offer persistent memory and layered prompts — that's table stakes. What they don't do is model *how cognition actually works*. They append memories until recall degrades. They let you configure a persona but never challenge it. They run tools in a loop until the context window fills or the user gives up.

Cortex takes a different approach: it implements computational models from cognitive neuroscience as Rust code. Every conversation turn runs a three-phase cycle modeled on brain networks — sensory preprocessing, focused execution with real-time self-monitoring, and reflective consolidation. Memory follows Complementary Learning Systems theory: fast episodic capture feeds slow semantic integration, with strategic forgetting that *improves* recall as the knowledge base grows. Five metacognitive detectors — inspired by anterior cingulate cortex conflict monitoring — catch reasoning failures mid-turn and self-tune their thresholds through experience.

The result is an agent runtime where the architecture itself drives improvement, not just the data it accumulates.

## What Makes It Different

**Memory that improves with scale, not degrades.** Most memory systems are append-only stores where recall gets noisier as they grow. Cortex implements a 4-stage lifecycle (Captured → Materialized → Stabilized → Deprecated) with a consolidation pipeline that converts episodic patterns into semantic knowledge. Recall fuses six dimensions — semantic similarity, BM25 keywords, recency decay, trust status, knowledge graph proximity, and access frequency. The 1000th memory is recalled as reliably as the 10th because the system actively curates what it knows.

**Self-correcting reasoning, not just tool loops.** Five metacognitive detectors run in parallel during every turn: doom loop detection (repeated identical actions), cognitive fatigue tracking, frame anchoring (stuck on wrong assumptions), reward prediction error (habitual vs. useful tool selection), and health degradation. When reasoning drifts, corrective hints inject directly into the next LLM call — the agent breaks its own loops before you notice. Thresholds self-tune via a Gratton effect: false alarms relax sensitivity, confirmed catches sharpen it.

**Prompts that evolve from evidence, not configuration.** Four prompt layers — Soul, Identity, Behavioral, User — change through accumulated interaction signals, not manual editing. Six evidence types (corrections, preferences, domain exposure, tool patterns, input complexity, first-turn signals) are scored and gated before any evolution occurs. The deepest layer changes last and requires the strongest evidence — convictions are earned, not configured.

**A cognitive cycle, not a while loop.** Each turn follows three phases mapped to neuroscience: SN (sensory — keyword extraction, working memory activation, input guards), TPN (task-positive — LLM inference and tool dispatch with metacognition checkpoints), DMN (default-mode — confidence assessment, memory extraction, prompt evolution). Between turns, a heartbeat engine drives autonomous maintenance: consolidation, embedding, knowledge graph updates, skill evolution.

## Features

- **17 core tools** — file I/O, shell execution, memory operations, web search, sub-agents, scheduled tasks, image/video/audio generation, introspection (audit, prompt_inspect, memory_graph)
- **5 cognitive skills** — deliberate, diagnose, review, orient, plan — auto-activate on detected patterns
- **Plugin ecosystem** — extend Cortex with third-party tools and skills via the MultiToolPlugin FFI interface and `.cpx` archives; official plugins like [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) available separately
- **11 LLM providers** — Anthropic, OpenAI, ZhipuAI, Kimi, MiniMax, OpenRouter, Ollama, with 3-tier routing and custom provider support
- **9 protocols** — HTTP REST, JSON-RPC 2.0, SSE, WebSocket, ACP, MCP server/client, Telegram, WhatsApp
- **Multi-instance** — fully isolated instances with independent config, data, memory, prompts, and systemd services

## Quick Start

**Prerequisites:** Linux with systemd, an API key from a [supported provider](#llm-providers)

```bash
# Install the binary
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install

# Create an instance and start the daemon
CORTEX_API_KEY="your-key" cortex install

# Verify
cortex status

# Start a conversation
cortex
```

See [docs/quickstart.md](docs/quickstart.md) for the full installation guide.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Repertoire — memories, learned patterns, domain skills     │  Continuous
├─────────────────────────────────────────────────────────────┤
│  Executive — Soul / Identity / Behavioral / User prompts,   │  Signal-driven
│              cognitive skills, metacognition hints           │  evolution
├─────────────────────────────────────────────────────────────┤
│  Substrate — Rust runtime: event sourcing, memory pipeline, │  Version
│              tool dispatch, 3-phase cognitive cycle          │  release
└─────────────────────────────────────────────────────────────┘
```

Six crates — five in a layered chain, plus a standalone SDK:

| Crate | Role |
|-------|------|
| **cortex-sdk** | Published plugin interface — zero internal dependencies, standalone on crates.io (~150 lines, pure interface) |
| **cortex-types** | Domain types — zero logic, no dependencies on other crates |
| **cortex-kernel** | Event journal (SQLite WAL), embedding pipeline, prompt manager |
| **cortex-turn** | Turn execution, memory recall, metacognition, tool dispatch, skills (depends on cortex-sdk, re-exports its traits) |
| **cortex-runtime** | Daemon lifecycle, HTTP/RPC/SSE/WS server, hot-reload, heartbeat |
| **cortex-app** | CLI, REPL, systemd integration, plugin management |

## How It Works

Each conversation turn follows a three-phase cognitive cycle:

1. **Sense** (SN) — activate working memory, extract keywords, recall relevant context
2. **Execute** (TPN) — LLM inference and tool dispatch loop with metacognition checkpoints
3. **Reflect** (DMN) — assess confidence, extract memories, trigger prompt evolution

Between turns, a heartbeat engine runs maintenance: memory consolidation, embedding generation, knowledge graph updates, and prompt self-evolution — all rate-limited and prioritized by salience.

## Memory

Memories progress through four lifecycle stages — **Captured**, **Materialized**, **Stabilized**, **Deprecated** — and are recalled via a 6-dimensional hybrid score:

| Dimension | Weight | Method |
|-----------|--------|--------|
| Semantic | 0.40 | Embedding cosine similarity |
| BM25 | 0.25 | Keyword matching |
| Recency | 0.15 | Exponential time decay |
| Status | 0.10 | Lifecycle stage weighting |
| Graph | 0.10 | Entity relationship proximity |
| Access | 0.05 | Retrieval frequency |

## LLM Providers

11 built-in providers with 3-tier routing (**heavy** / **medium** / **light**):

`anthropic` · `openai` · `zai` · `zai-openai` · `zai-cn` · `zai-cn-openai` · `kimi` · `kimi-cn` · `minimax` · `openrouter` · `ollama`

Custom providers can be added to `providers.toml`. Four presets control which sub-endpoints are active: `minimal`, `standard`, `cognitive`, `full`.

## Usage

| Mode | Command |
|------|---------|
| Interactive REPL | `cortex` |
| Single prompt | `cortex "question"` |
| Pipe | `echo "data" \| cortex "summarize"` |
| Web UI | `http://127.0.0.1:PORT/` |
| Agent protocol | `cortex --acp` |
| Tool provider | `cortex --mcp-server` |

```bash
# Service management
cortex install [--system] [--id work]   # Register systemd service
cortex start | stop | restart            # Manage daemon
cortex status                            # Show health, address, LLM info
cortex ps                                # List all instances
cortex reset [--factory]                 # Reset data or full wipe

# Plugin management
cortex plugin install owner/repo         # Install from GitHub
cortex plugin list                       # List installed plugins
cortex plugin uninstall name             # Remove a plugin

# Environment setup
cortex node setup                        # Install Node.js + pnpm (for MCP servers)
cortex node status                       # Show Node.js environment status
cortex browser enable                    # Configure Chrome DevTools MCP server
cortex browser status                    # Show browser integration status
```

## Documentation

| | |
|---|---|
| **[Quick Start](docs/quickstart.md)** | Installation, first conversation, running modes |
| **[Usage Guide](docs/usage.md)** | CLI, API, tools, skills, memory, protocols |
| **[Configuration](docs/config.md)** | All config sections, providers, hot-reload |
| **[Operations](docs/ops.md)** | Service management, monitoring, security, backup |
| **[Plugins](docs/plugins.md)** | Plugin usage, development, `.cpx` format, native FFI |

## Development

```bash
docker compose run --rm dev cargo build --release
docker compose run --rm dev cargo test --workspace
docker compose run --rm dev cargo clippy --workspace --all-targets -- -D warnings
```

## License

[MIT](LICENSE)
