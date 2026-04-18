# Usage Reference

## Running Modes

| Mode | Command / URL | Transport | Notes |
|------|---------------|-----------|-------|
| Interactive REPL | `cortex` | Unix socket | Default; slash commands available |
| Single prompt | `cortex "question"` | Unix socket | Single RPC call, returns and exits |
| Pipe input | `echo "content" \| cortex` | Unix socket | Reads stdin, sends as prompt |
| Named session | `cortex --session name "prompt"` | Unix socket | Switches to or creates named session |
| Named instance | `cortex --id work "prompt"` | Unix socket | Targets a named instance |
| Web UI | `http://127.0.0.1:PORT/` | HTTP | Chat interface embedded in binary |
| Dashboard | `http://127.0.0.1:PORT/dashboard.html` | HTTP | Metrics and status |
| Audit trail | `http://127.0.0.1:PORT/audit.html` | HTTP | Decision audit log viewer |
| ACP agent | `cortex --acp` | stdio | Agent Communication Protocol (JSON-RPC 2.0) |
| MCP server | `cortex --mcp-server` | stdio | Model Context Protocol (JSON-RPC 2.0) |
| Daemon | `cortex --daemon` | HTTP + socket + stdio | Full runtime; all transports |

---

## CLI Subcommands

### Service

| Command | Description |
|---------|-------------|
| `cortex install [--system] [--id ID]` | Install as systemd service |
| `cortex uninstall [--purge]` | Remove service (--purge removes all data) |
| `cortex start` | Start the daemon |
| `cortex stop` | Stop the daemon |
| `cortex restart` | Restart the daemon |
| `cortex status` | Show daemon status |
| `cortex ps` | List running instances |
| `cortex reset [--factory] [--force]` | Reset instance data |
| `cortex help [subcommand]` | Show help |

### Node and Browser

| Command | Description |
|---------|-------------|
| `cortex node setup` | Install Node.js + pnpm (for MCP servers) |
| `cortex node status` | Show Node.js environment status |
| `cortex browser enable` | Configure Chrome DevTools MCP server |
| `cortex browser status` | Show browser integration status |

### Plugin

| Command | Description |
|---------|-------------|
| `cortex plugin install <source>` | Install from GitHub, URL, .cpx, or directory |
| `cortex plugin uninstall <name> [--purge]` | Remove a plugin |
| `cortex plugin list` | List installed plugins |
| `cortex plugin pack <dir> [output.cpx]` | Package a plugin directory |

### Channel

| Command | Description |
|---------|-------------|
| `cortex channel telegram` / `whatsapp` | Show channel config info |
| `cortex channel pair [platform]` | List pending and paired users |
| `cortex channel approve <plat> <id>` | Approve a pending user |
| `cortex channel revoke <plat> <id>` | Remove a paired user |
| `cortex channel allow <plat> <id>` | Add to whitelist |
| `cortex channel deny <plat> <id>` | Add to blacklist |
| `cortex channel policy <plat> [mode]` | Get or set policy (pairing/whitelist/open) |

### Instance Targeting

Any command accepts `--id <instance>` to target a named instance instead of the default.

---

## REPL Commands

| Command | Description |
|---------|-------------|
| `/help` | Show available commands |
| `/status` | Show runtime status (tokens, turns, uptime, config) |
| `/stop` | Cancel the running turn |
| `/quit` or `/exit` | Exit the REPL |
| `/session list` | List all sessions |
| `/session new` | Create a new session |
| `/session switch <id>` | Switch to a session |
| `/config list` | List config sections |
| `/config get <section>` | Show a config section |

All platforms (REPL, Telegram, WhatsApp, WebSocket) support `/stop` and `/status` during a turn. Messages sent while a turn is running are injected mid-turn so the LLM sees them in the next iteration. In the REPL, Ctrl+C sends `/stop` automatically.

Config sections: `daemon`, `api`, `embedding`, `web`, `plugins`, `llm_groups`, `mcp`, `memory`, `turn`, `metacognition`, `autonomous`, `context`, `skills`, `auth`, `tls`, `rate_limit`, `tools`, `health`, `evolution`, `router`, `ui`, `memory_share`

---

## HTTP API

### Standard Turn

```
POST /api/turn
Content-Type: application/json

{
  "session_id": "my-session",
  "input": "What is the weather?",
  "images": [
    {
      "media_type": "image/jpeg",
      "data": "<base64>"
    }
  ]
}
```

The `images` field is optional. Each entry requires `media_type` and `data` (base64-encoded).

### SSE Streaming

```
POST /api/turn/stream
Content-Type: application/json

{ "session_id": "my-session", "input": "Explain recursion" }
```

Event types: `text`, `tool`, `trace`, `done`

### WebSocket

```
GET /api/ws
```

Bidirectional JSON-RPC over WebSocket.

### REST Endpoints

| Method | Path | Description | Auth/Rate Exempt |
|--------|------|-------------|:---:|
| GET | `/api/health` | Health check | Yes |
| GET | `/api/metrics/structured` | Structured metrics | Yes |
| GET | `/api/daemon/status` | Daemon status and uptime | No |
| POST | `/api/turn` | Execute a turn | No |
| POST | `/api/turn/stream` | SSE streaming turn | No |
| GET | `/api/ws` | WebSocket upgrade | No |
| GET | `/api/sessions` | List sessions (limit, offset params) | No |
| POST | `/api/session` | Create session | No |
| GET | `/api/session/:id` | Session details | No |
| GET | `/api/memory` | List memories | No |
| POST | `/api/memory` | Save memory | No |
| GET | `/api/audit/summary` | Audit summary | No |
| GET | `/api/audit/health` | Audit health score | No |
| GET | `/api/audit/decision-path/:id` | Decision trace (404 if not found) | No |
| POST | `/api/rpc` | JSON-RPC 2.0 endpoint | No |

### Constraints

- `session_id`: max 256 characters, alphanumeric plus hyphens, underscores, and dots
- Empty input returns 400
- Request body limit: 2 MB (413 if exceeded)
- Maximum 10,000 concurrent sessions

---

## JSON-RPC 2.0

Available at `/api/rpc` (HTTP), `/api/ws` (WebSocket), Unix socket, and stdio (ACP/MCP).

21 methods across 8 namespaces:

| Namespace | Method | Parameters |
|-----------|--------|------------|
| `session` | `prompt` | `prompt` (string), `session_id` (optional) |
| `session` | `new` | -- |
| `session` | `list` | `limit` (optional), `offset` (optional) |
| `session` | `end` | `session_id` |
| `session` | `get` | `session_id` |
| `session` | `initialize` | -- |
| `session` | `cancel` | -- |
| `command` | `dispatch` | `command` (string) |
| `daemon` | `status` | -- |
| `skill` | `list` | -- |
| `skill` | `invoke` | `name`, `params` |
| `skill` | `suggestions` | -- |
| `memory` | `list` | -- |
| `memory` | `get` | `id` |
| `memory` | `save` | `type` ("User"/"Feedback"/"Project"/"Reference"), `content`, `description` |
| `memory` | `delete` | `id` |
| `memory` | `search` | `query` |
| `health` | `check` | -- |
| `meta` | `alerts` | `session_id` |
| `mcp` | `prompts/list` | -- |
| `mcp` | `prompts/get` | `name` |

### Error Codes

| Code | Meaning |
|------|---------|
| -32700 | Parse error |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid params |
| 1000 | Session not found |
| 1001 | Session already ended |
| 1100 | Turn execution failed |
| 1200 | Command dispatch failed |
| 1300 | Memory not found |
| 1301 | Memory operation failed |

---

## Messaging Channels

### Telegram

**Polling mode** (no public IP required):

Create `~/.cortex/<instance>/channels/telegram/auth.json`:

```json
{
  "bot_token": "123456:ABC-DEF...",
  "mode": "polling"
}
```

**Webhook mode** (requires public IP or reverse proxy):

```json
{
  "bot_token": "123456:ABC-DEF...",
  "mode": "webhook",
  "webhook_addr": "127.0.0.1:8443"
}
```

### WhatsApp

Webhook mode only. Create `~/.cortex/<instance>/channels/whatsapp/auth.json`:

```json
{
  "access_token": "...",
  "phone_number_id": "...",
  "verify_token": "...",
  "mode": "webhook",
  "webhook_addr": "127.0.0.1:8444"
}
```

### Shared Behavior

Both channels share the daemon state, automatically manage sessions (`tg-{uid}` for Telegram, `wa-{phone}` for WhatsApp), support slash commands, respect rate limits, and split long responses to fit platform message size limits.

---

## ACP and MCP

### ACP (Agent Communication Protocol)

Start with `cortex --acp`. Communicates over stdio using JSON-RPC 2.0.

Typical lifecycle:

1. `session/initialize` -- handshake
2. `session/new` -- create a session
3. `session/prompt` -- send prompts, receive responses

### MCP (Model Context Protocol)

Start with `cortex --mcp-server`. Communicates over stdio using JSON-RPC 2.0.

Typical lifecycle:

1. `initialize` -- capability negotiation
2. `tools/list` -- discover available tools
3. `tools/call` -- invoke a tool

Also supports `prompts/list` and `prompts/get` for prompt template discovery.

Both ACP and MCP require a running daemon and bridge to it through the Unix socket.

---

## Core Tools (17)

| Tool | Description |
|------|-------------|
| `bash` | Shell execution with risk assessment |
| `read` | File reading with partial range support |
| `write` | File creation and replacement |
| `edit` | Search-and-replace editing |
| `memory_search` | 6D hybrid recall across memory stores |
| `memory_save` | Persist to long-term memory (types: User, Feedback, Project, Reference) |
| `agent` | Spawn sub-agents (modes: readonly, full, fork, teammate; max depth 3) |
| `skill` | Invoke reasoning protocols |
| `cron` | Schedule recurring or one-shot tasks |
| `web_search` | Brave Search API with domain filtering |
| `web_fetch` | URL content extraction (10 MB limit, 60s timeout) |
| `image_gen` | Generate images from text prompts |
| `tts` | Text-to-speech synthesis |
| `video_gen` | Generate video from text prompts |
| `audit` | Query the audit log -- event counts, health score, recent events |
| `prompt_inspect` | Read and inspect prompt layers (soul, identity, behavioral, user) |
| `memory_graph` | Query the entity relationship graph -- nodes, edges, neighbors |

### Introspection Tools

The three introspection tools (`audit`, `prompt_inspect`, `memory_graph`) give the LLM read-only access to Cortex's own internal state. They open read-only handles to the journal, prompt files, and graph database without sharing mutable state with the daemon.

- `audit` -- commands: `summary`, `health`, `recent`
- `prompt_inspect` -- read prompt layer contents by name
- `memory_graph` -- commands: `stats`, `neighbors`, `search`

### Plugin Tools

Additional tools can be installed via plugins. For example, the official [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) plugin provides development tools including code navigation (tree-sitter), git operations, docker management, task tracking, and more.

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

Plugin tools are loaded through the `MultiToolPlugin` FFI interface defined in `cortex-sdk`. See [docs/plugins.md](plugins.md) for details on the plugin system.

MCP-bridged tools extend the set further at runtime (named `mcp_{server}_{tool}`).

---

## Cognitive Skills (5)

| Skill | Purpose |
|-------|---------|
| `deliberate` | Structured evidence accumulation for high-stakes decisions |
| `diagnose` | Trace symptoms to root cause through hypothesis testing |
| `review` | Perspective-shifted adversarial examination |
| `orient` | Rapid comprehension of unfamiliar territory |
| `plan` | Hierarchical task decomposition |

Skills are loaded with 3-tier precedence: system < plugin < instance (instance overrides win). Plugins can provide additional skills; see [docs/plugins.md](plugins.md).

### Custom Skills

Create skills in `~/.cortex/<instance>/skills/<name>/SKILL.md` with YAML frontmatter:

```yaml
---
description: What this skill does
when_to_use: When the LLM should activate it
required_tools: [bash, read]
tags: [analysis]
activation:
  input_patterns: ["regex patterns"]
---
```

---

## Memory System

### Types

| Type | Purpose |
|------|---------|
| User | User preferences, corrections, personal context |
| Feedback | Behavioral adjustments from interactions |
| Project | Project-specific knowledge and conventions |
| Reference | Durable reference material |

### Kinds

- **Episodic** -- time-bound memories that decay over time
- **Semantic** -- durable generalized knowledge

### Lifecycle Stages

Captured --> Materialized --> Stabilized --> Deprecated

### 6D Recall Scoring

| Dimension | Weight |
|-----------|--------|
| BM25 (keyword match) | 0.25 |
| Semantic cosine similarity | 0.40 |
| Recency | 0.15 |
| Status | 0.10 |
| Access frequency | 0.05 |
| Graph connectivity | 0.10 |

---

## Prompt Evolution

### 4 Layers

| Layer | Purpose |
|-------|---------|
| Soul | Existential orientation — grows through sustained experience, changes most slowly |
| Identity | Role and capabilities |
| Behavioral | Interaction patterns and style |
| User | Personalized adaptations |

### 6 Evolution Signals

| Signal | Weight |
|--------|--------|
| Corrections | 1.0 |
| Preferences | 0.8 |
| New domains | 0.6 |
| First session | 0.5 |
| Tool-intensive turns | 0.4 |
| Long inputs | 0.3 |

---

## Turn Trace

Every turn produces a trace with events in 6 categories:

`phase`, `llm`, `tool`, `meta`, `memory`, `context`

---

## LLM Groups

### Default Tiers

| Tier | Purpose |
|------|---------|
| `heavy` | Primary reasoning (largest model) |
| `medium` | Analysis and extraction |
| `light` | Simple extraction tasks |

### Presets

4 built-in presets: `minimal`, `standard`, `cognitive`, `full`

### Sub-Endpoints

7 specialized sub-endpoints that can be routed to different tiers:

`memory_extract`, `entity_extract`, `compress`, `summary`, `self_update`, `causal_analyze`, `autonomous`

---

## Multi-Instance

Each instance is fully isolated with its own config, data, memory, prompts, skills, sessions, and socket.

```
cortex --id work "prompt"
cortex --id work status
cortex --id work install
```

Instance IDs: alphanumeric characters, hyphens, and underscores. Maximum 64 characters.

---

## Vision

Images can be sent through the HTTP API via the `images` field on turn requests. Resolution priority for model selection:

1. Explicit `[api.vision]` config setting
2. Provider `vision_model` field
3. Auto-discovery of vision-capable models
4. Primary model probing
