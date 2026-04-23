# Configuration

## Directory Layout

```text
~/.cortex/
  providers.toml                 # Shared provider registry
  plugins/                       # Shared plugin install root
  <instance>/
    config.toml                  # Declarative instance configuration
    config.defaults.toml         # Generated defaults reference (read-only)
    actors.toml                  # Actor aliases + transport bindings
    mcp.toml                     # MCP server definitions
    prompts/                     # Prompt layers + system templates
    skills/                      # Built-in and instance-level skills
    data/                        # Runtime state (managed by Cortex)
    memory/                      # Persistent memory store
    sessions/                    # Session history
    channels/                    # Channel auth + runtime pairing state
```

**Rule of thumb:** root files define what the instance _should_ look like. `data/` records what _happened_ while running.

## Key Files

### `config.toml`

Primary instance configuration. Covers:

- Daemon and transport settings (HTTP bind address, Unix socket path, TLS)
- API provider defaults (provider, model, base URL)
- Embedding configuration (provider, model, dimensions)
- Memory behavior (recall count, extraction cadence, consolidation interval, decay rates, similarity thresholds)
- Turn behavior (max iterations, whole-turn timeout, per-tool timeout, token limits)
- Metacognition (detector weights, health check interval, fatigue thresholds)
- Context handling (pressure thresholds, summarization strategy)
- Plugin enablement
- Auth (OAuth, JWT)
- Rate limiting (per-session and global RPM)
- Media generation defaults

### `config.defaults.toml`

Auto-generated reference showing all default values. Cortex writes this on install and after config changes. Not a configuration file — do not edit it. Use it to discover available settings and their defaults.

### `actors.toml`

Identity mapping between transports, channel actors, and canonical users:

```toml
[aliases]
"telegram:123456789" = "user:alice"

[transports]
http = "user:alice"
rpc = "user:alice"
ws = "user:alice"
sock = "user:alice"
stdio = "user:alice"
```

### `mcp.toml`

MCP server definitions. Each entry names an external MCP server that Cortex can connect to for additional tools and prompts.

### `providers.toml`

Shared provider registry. Each provider entry defines protocol, base URL, auth style, model list, and optional multimodal routing:

| Field | Purpose |
|-------|---------|
| `protocol` | `anthropic`, `openai`, or `ollama` wire format |
| `base_url` | Provider API root |
| `auth_type` | `x-api-key`, `bearer`, or `none` |
| `models` | Known text models; empty means runtime discovery or explicit config |
| `vision_provider` | Optional provider used only for vision requests |
| `vision_model` | Default multimodal model; empty means auto-discovery |
| `image_input_mode` | OpenAI-compatible image mode: `data-url`, `upload-then-url`, or `remote-url-only` |
| `files_base_url` | File upload/content API root for `upload-then-url` |
| `openai_stream_options` | Whether the endpoint accepts OpenAI `stream_options` |
| `vision_max_output_tokens` | Output cap for vision calls; `0` uses the safe default |
| `capability_cache_ttl_hours` | Model/capability cache TTL; `0` uses runtime default |

Cortex keeps text and vision routing separate. Pure text turns use the configured text endpoint. Turns with image attachments resolve the vision endpoint from explicit config, then `vision_provider` / `vision_model`, then discovery and cache.

### Memory Behavior

`[memory]` controls durable memory extraction, recall, consolidation, decay, and semantic upgrade:

| Field | Default | Purpose |
|-------|---------|---------|
| `max_recall` | `10` | Maximum recalled memories injected into a turn |
| `auto_extract` | `true` | Whether post-turn memory extraction runs automatically |
| `extract_min_turns` | `5` | Minimum turns between automatic extraction passes |
| `consolidate_interval_hours` | `24` | Maintenance cadence for consolidation and decay |
| `decay_rate` | `0.05` | Time-decay rate for stale memories |
| `consolidation_similarity_threshold` | `0.85` | Embedding similarity required for smart merge candidates |
| `semantic_upgrade_similarity_threshold` | `0.90` | Similarity required to upgrade repeated episodic memories into semantic memory |

Extraction now records source, memory kind, and confidence. Explicit user statements and direct tool evidence are kept distinct from model inference. Active reconsolidation windows are injected into extraction so newly observed corrections can update stabilized memories instead of creating disconnected duplicates.

### Turn Timeouts

`[turn].execution_timeout_secs` controls the foreground turn as a whole, including LLM calls, tool calls, sub-agents, and final delivery. The default is `0`, which disables the whole-turn timeout. This lets long multi-step work continue as long as each individual operation remains healthy.

`[turn].tool_timeout_secs` controls one tool invocation. The default is `1800` seconds. Tools may define a stricter timeout for their own safety.

`[turn].llm_transient_retries` controls how many times Cortex retries a transient LLM transport/provider failure before any user-visible text has been emitted. The default is `5`; set it to `0` to disable this safety net.

## Runtime Data (`data/`)

Runtime-managed files — do not edit directly:

- `cortex.db` — Event journal (SQLite WAL)
- `embedding_store.db` — Vector embedding index
- `memory_graph.db` — Memory relationship graph
- `cortex.sock` — Unix domain socket
- `actor_sessions.json`, `client_sessions.json` — Session mappings
- Model and capability caches

## Channel Configuration

Each channel directory (`channels/<platform>/`) separates declarative auth from runtime state:

| File | Managed by | Purpose |
|------|-----------|---------|
| `auth.json` | You | Bot token and credentials |
| `policy.json` | Runtime | Access policy (open / whitelist / pairing) |
| `paired_users.json` | Runtime | Approved user list |
| `pending_pairs.json` | Runtime | Pending pairing requests |

## Install-Time Environment Variables

Read by `cortex install` to seed initial configuration:

| Variable | Purpose |
|----------|---------|
| `CORTEX_API_KEY` | Primary provider API key |
| `CORTEX_PROVIDER` | Provider name |
| `CORTEX_MODEL` | Model identifier |
| `CORTEX_BASE_URL` | Custom provider endpoint |
| `CORTEX_LLM_PRESET` | Endpoint preset: `minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_EMBEDDING_PROVIDER` | Embedding provider |
| `CORTEX_EMBEDDING_MODEL` | Embedding model |
| `CORTEX_BRAVE_KEY` | Brave Search API key |
| `CORTEX_TELEGRAM_TOKEN` | Telegram bot token |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp token |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ bot credentials |

## Hot Reload

These files reload without restarting the daemon:

- `config.toml` — All runtime-safe settings
- `providers.toml` — Provider registry
- `mcp.toml` — MCP server definitions
- `prompts/` — All prompt layers
- `skills/` — Skill definitions and SKILL.md files

Changes take effect on the next turn. Active turns complete with the previous configuration.
