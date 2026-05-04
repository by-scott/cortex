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
    prompts/                     # Executive prompt files + system templates
    skills/                      # Built-in and instance-level skills
    data/                        # Runtime state: journal, embeddings, task/goal DBs
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
- ACP client definitions for configured external agent processes
- Plugin enablement
- Tool risk policies
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

### ACP clients

ACP clients are declared in `config.toml` under `[acp]`. When at least one client is configured, Cortex registers the `acp_agent` tool so a turn can delegate to an external ACP-compatible agent process through stdio JSON-RPC.

```toml
[acp]
request_timeout_secs = 120

[[acp.clients]]
id = "reviewer"
command = "reviewer-agent"
args = ["--stdio"]
cwd = "/workspace/project"
env = { REVIEWER_MODE = "strict" }
```

| Field | Purpose |
|-------|---------|
| `request_timeout_secs` | Per-request timeout for initialize, session creation, and prompt calls |
| `clients[].id` | Stable id used by the `acp_agent` tool |
| `clients[].command` | Executable to spawn |
| `clients[].args` | Command arguments |
| `clients[].cwd` | Session root sent to `session/new`; relative paths resolve from the daemon process cwd |
| `clients[].env` | Extra environment variables passed to the child process |

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

For local Ollama and vLLM examples, see [Local Models](local-models.md).

### Model token limits

Cortex treats configured token limits as operator overrides, not universal
model facts. If `[api].max_tokens`, `[context].max_tokens`, or an LLM group's
`context_tokens` / `output_tokens` are `0`, Cortex first asks the provider model
metadata endpoint and caches the result. When a provider omits those fields or
is offline, Cortex falls back to conservative provider/model-family defaults
from the configured model name, such as `claude-*`, `gpt-4o`, `qwen*`,
`glm*`, `llama*`, `mistral*`, or explicit name markers like `32k` and `128k`.

This means Cortex should not treat every model as `200k` input and `300k`
output. Operators can still pin exact values in config when a gateway has a
custom deployment limit.

### `[llm_groups.*]` and model routing

LLM groups are the model capability registry used by background endpoints and
route decisions. The default groups are `heavy`, `medium`, and `light`; explicit
`[api.endpoint_groups]` entries still express operator preference, but the
runtime resolver now scores the available groups by capability, health posture,
cost, latency, safety, and reasoning depth before choosing a sub-endpoint model.

| Field | Purpose |
|-------|---------|
| `provider` | Provider name; empty inherits `[api].provider` |
| `model` | Model name; empty inherits `[api].model` or the provider's first known model |
| `api_key` | Optional group-specific key; empty inherits `[api].api_key` |
| `max_tokens` | Output cap; `0` inherits the parent or model-specific inferred cap |
| `capabilities` | Optional declared capability list: `coding`, `long_context`, `vision`, `tool_calling`, `json_reliability`, `low_latency`, `low_cost`, `high_safety`, `deep_reasoning` |
| `context_tokens` | Input context window; `0` lets Cortex infer |
| `output_tokens` | Output token ceiling; `0` lets Cortex infer |
| `latency_ms` | Expected median latency; `0` lets Cortex infer by tier |
| `input_cost_per_million` | Input-token cost hint; `0` lets Cortex infer by tier |
| `output_cost_per_million` | Output-token cost hint; `0` lets Cortex infer by tier |
| `safety_score` | Safety score in `[0, 1]`; `0` lets Cortex infer |
| `reasoning_depth` | Reasoning-depth score in `[0, 1]`; `0` lets Cortex infer |
| `json_reliability` | Structured-output reliability in `[0, 1]`; `0` lets Cortex infer |

Route requests also carry intent, required/preferred capabilities, confidence,
risk, failed targets, and fallback reasons such as provider failure or invalid
schema output. A low-confidence high-risk request escalates toward `high_safety`
and `deep_reasoning`; schema-invalid fallback requires `json_reliability`.

## Memory Behavior

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

Extraction records source, memory kind, and confidence. Explicit user statements and direct tool evidence remain distinct from model inference. Active reconsolidation windows are injected into extraction so newly observed corrections can update stabilized memories instead of creating disconnected duplicates.

## Turn Timeouts

`[turn].execution_timeout_secs` controls the foreground turn as a whole, including LLM calls, tool calls, delegated workers, and final delivery. The default is `0`, which disables the whole-turn timeout.

`[turn].tool_timeout_secs` controls one tool invocation. The default is `1800` seconds. Tools may define a stricter timeout for their own safety.

`[turn].llm_transient_retries` controls how many times Cortex retries a transient LLM transport/provider failure before any user-visible text has been emitted. The default is `5`; set it to `0` to disable this safety net.

## Tool Risk Policies

`[risk.tools.<name>]` defines explicit risk policy for one tool. Use this for plugin and MCP tools after reviewing what the tool can do.

```toml
risk.allow = ["read", "memory_*", "word_count"]
risk.deny = ["deploy_*", "*_shell"]
auto_approve_up_to = "Review"

[risk.tools.word_count]
tool_risk = 0.1
blast_radius = 0.0
irreversibility = 0.0
allow_background = true

[risk.tools.deploy_production]
require_confirmation = true
blast_radius = 0.9
irreversibility = 0.8

[risk.tools.unknown_shell_bridge]
block = true
```

Available fields:

| Field | Purpose |
|-------|---------|
| `tool_risk` | Override the base tool-risk axis, `0.0` to `1.0` |
| `file_sensitivity` | Override file/path sensitivity, `0.0` to `1.0` |
| `blast_radius` | Override potential impact scope, `0.0` to `1.0` |
| `irreversibility` | Override reversibility risk, `0.0` to `1.0` |
| `require_confirmation` | Force at least `RequireConfirmation` |
| `block` | Block the tool regardless of score |
| `allow_background` | Document whether the tool is intended for background use |

`risk.deny` always wins. If `risk.allow` is non-empty, tools not matching it are blocked. `auto_approve_up_to` controls which non-block risk levels run without confirmation: `Review` is the default standard mode, `Allow` is the stricter mode, and `RequireConfirmation` is the most permissive setting for normal execution. `Block` still denies without prompting. Background execution additionally requires either the tool's declared `background_safe` capability or `allow_background = true` for that tool.

Static policy checks are available through the CLI:

```bash
cortex policy lint
cortex policy simulate deploy --effect deploy:production --actor user:alice
```

`cortex policy lint` reads the current instance config and enabled plugin manifests, then reports dangerous combinations before use: open permission mode with unreviewed plugins, unreadable plugin manifests, native/process plugins without explicit `[risk.tools.<name>]` profiles, secret-capable plugins without confirmation/block policy, `web_fetch` with automatic memory extraction, and high-impact background tool policies. The daemon logs the same findings during startup. `cortex policy simulate` explains one tool/effect decision: actor, effective risk level, auto-approval, confirmation requirement, background eligibility, and the policy reasons.

For copyable local-first posture examples, see [Policy Profiles](policy-profiles.md). They are `config.toml` fragments, not an automatic profile loader.

The instance directory is a protected runtime root. Ordinary tools cannot access prompt, config, session, journal, memory, or channel state under the instance home, including paths reached through symlinks. Process and script tools are not disabled globally: builds, tests, diagnostics, shell inspection, project writes, and helper scripts run through the normal permission gate unless the invocation directly targets protected instance state. Plugin tools cannot directly present prompt, config, session, journal, memory, or runtime-state mutation as an LLM-callable shortcut; self-evolution plugins must return proposals and let checked runtime paths apply validated changes. Use checked runtime commands, PromptManager flows, or governed package workflows for instance changes.

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
| `CORTEX_PERMISSION_LEVEL` | Install-time permission mode: `strict` / `balanced` / `open` |
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
- `prompts/` — Executive prompt files and system templates
- `skills/` — Skill definitions and SKILL.md files

Changes take effect on the next turn. Active turns complete with the previous configuration.

The CLI also hot-applies several operator flows without a restart in the normal user-service path:

- `cortex permission ...`
- `cortex browser enable` / `cortex browser disable`
- `cortex plugin enable` / `cortex plugin disable`
- `cortex channel subscribe ...` / `cortex channel unsubscribe ...`

Plugin package governance is declared in each plugin `manifest.toml`, not in the instance config. Use `cortex plugin review <dir>` and `cortex plugin test <dir>` before install to inspect capability requests, sandbox profile, signature metadata, conformance state, and recommended `[risk.tools.<name>]` policy.
