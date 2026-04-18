# Configuration Reference

## File Layout

```
~/.cortex/
  providers.toml              # Global provider registry (hot-reloaded)
  plugins/                    # Global plugin storage
  <instance>/
    config.toml               # Instance config (hot-reloaded)
    mcp.toml                  # MCP server configuration
    cortex.sock               # Unix domain socket (in data/)
    prompts/
      soul.md                 # Existential orientation — grows through experience
      identity.md             # Self-model and architecture
      behavioral.md           # Tool usage and skill guidance
      user.md                 # Collaborator profile
      .initialized            # Bootstrap completion marker
      system/                 # System prompt templates
        memory-extract.md, memory-consolidate.md, bootstrap.md,
        bootstrap-init.md, hint-exploration.md, hint-doom-loop.md,
        hint-fatigue.md, hint-frame-anchoring.md, context-compress.md,
        context-summarize.md, entity-extract.md, self-update.md,
        causal-analyze.md, batch-analysis.md, summarize-system.md,
        agent-readonly.md, agent-full.md, agent-teammate.md
    skills/
      system/                 # Built-in skills (deliberate, diagnose, review, orient, plan)
      <custom>/SKILL.md       # User-defined skills
    data/
      cortex.db               # Event journal (SQLite WAL)
      embedding_store.db      # Embedding vectors
      memory_graph.db         # Entity relationship graph
      cron_queue.json         # Scheduled tasks
      model_info.json         # Cached provider model catalog
      vision_caps.json        # Cached vision capability probes
      defaults.toml           # Generated defaults reference
      node/                   # Node.js environment (created by `cortex node setup`)
    memory/                   # Persistent memory files (.md with YAML frontmatter)
    sessions/                 # Session history
    channels/                 # Channel auth (telegram/, whatsapp/)
```

## Environment Variables

Consumed ONLY during `cortex install` to generate config.toml.

| Variable | Purpose | Default |
|----------|---------|---------|
| CORTEX_API_KEY | Provider API key | (required) |
| CORTEX_PROVIDER | Provider name | anthropic |
| CORTEX_MODEL | Model identifier | (provider default) |
| CORTEX_BASE_URL | Custom endpoint URL | (provider default) |
| CORTEX_LLM_PRESET | Sub-endpoint preset | full |
| CORTEX_EMBEDDING_PROVIDER | Embedding provider | ollama |
| CORTEX_EMBEDDING_MODEL | Embedding model | nomic-embed-text |
| CORTEX_BRAVE_KEY | Brave Search API key | (empty) |
| CORTEX_TELEGRAM_TOKEN | Telegram bot token | (empty) |
| CORTEX_WHATSAPP_TOKEN | WhatsApp access token | (empty) |

## config.toml

Canonical section order: daemon, api, embedding, web, plugins, llm_groups, memory, turn, metacognition, autonomous, context, skills, auth, tls, rate_limit, tools, health, evolution, ui, memory_share, media.

---

### [daemon]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| addr | string | "127.0.0.1:0" | HTTP listen address. Port 0 assigns random port; actual port persisted on first bind. |
| maintenance_interval_secs | integer | 1800 | Heartbeat maintenance cycle interval (30 min). |
| model_info_ttl_hours | integer | 168 | Provider model catalog cache TTL (7 days). |

---

### [api]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| provider | string | "anthropic" | Provider name (must match providers.toml entry). |
| api_key | string | "" | Provider API key. |
| model | string | "" | Model identifier. Empty uses provider's first listed model. |
| max_tokens | integer | 0 | Max output tokens per LLM call. 0 uses system fallback of 300,000. |
| preset | string | "full" | Sub-endpoint activation preset: minimal, standard, cognitive, full. |

#### [api.endpoints]

Per-endpoint enable/disable toggles. Preset sets defaults; explicit values override.

| Key | Type | Description |
|-----|------|-------------|
| memory_extract | bool | Extract memories from turns |
| entity_extract | bool | Extract entity relationships |
| compress | bool | Compress context at pressure thresholds |
| summary | bool | Generate conversation summaries |
| self_update | bool | Evolve prompt layers from evidence |
| causal_analyze | bool | Analyze causal patterns |
| autonomous | bool | Enable autonomous cognition during idle |

Presets:

| Preset | Enabled |
|--------|---------|
| minimal | None |
| standard | memory_extract, compress, entity_extract |
| cognitive | standard + self_update, causal_analyze, autonomous |
| full | All 7 |

#### [api.endpoint_groups]

Map sub-endpoints to named LLM groups defined in `[llm_groups]`:

```toml
[api.endpoint_groups]
memory_extract = "light"
entity_extract = "light"
compress = "light"
summary = "light"
self_update = "medium"
causal_analyze = "medium"
```

#### [api.vision]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| provider | string | "" | Vision provider name |
| model | string | "" | Vision model identifier |

When empty, uses auto-discovery: provider's vision_model, then primary model probing.

---

### [embedding]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| provider | string | "ollama" | Embedding provider name |
| api_key | string | "" | Embedding API key |
| model | string | "nomic-embed-text" | Embedding model identifier |
| dimensions | integer | 0 | Vector dimensions. 0 uses native dimensions. |
| candidates | array | [] | Candidate models for auto-switch evaluation |
| min_samples | integer | 10 | Minimum samples before evaluation |
| auto_switch | bool | false | Enable automatic model switching |
| switch_threshold_samples | integer | 50 | Samples required before switching |
| switch_precision_delta | float | 0.1 | Precision improvement required to switch |

---

### [web]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| search_backend | string | "brave" | Web search backend |
| brave_api_key | string | "" | Brave Search API key |
| brave_max_results | integer | 10 | Default max results per search |
| brave_max_results_limit | integer | 20 | Hard ceiling on results |
| fetch_max_chars | integer | 100000 | Default max chars when fetching pages |
| fetch_max_chars_limit | integer | 500000 | Hard ceiling on fetch chars |

---

### [plugins]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| dir | string | "plugins" | Plugin directory path |
| enabled | array | [] | List of enabled plugin names |

Plugins use the `MultiToolPlugin` FFI interface, allowing a single shared library to register multiple tools at once. No plugins are enabled by default; install and enable plugins as needed. See [docs/plugins.md](plugins.md) for details.

---

### [llm_groups]

Named LLM configurations for routing sub-endpoints to different models and providers. Three tiers auto-generated during install.

```toml
[llm_groups.heavy]
provider = "anthropic"
model = "claude-sonnet-4-20250514"
api_key = ""   # empty inherits from [api]
max_tokens = 0

[llm_groups.medium]
provider = "zai"
model = "glm-4.7"

[llm_groups.light]
provider = "zai"
model = "glm-4.5-air"
```

Resolution order: endpoint field > group field > [api] field > provider default.

---

### [memory]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| max_recall | integer | 10 | Maximum memories recalled per query |
| decay_rate | float | 0.05 | Memory relevance decay rate |
| auto_extract | bool | true | Automatically extract memories from turns |
| extract_min_turns | integer | 5 | Minimum turns before extraction triggers |
| consolidate_interval_hours | integer | 24 | Hours between memory consolidation cycles |

---

### [turn]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| max_tool_iterations | integer | 1024 | Maximum tool calls per turn |
| tool_timeout_secs | integer | 300 | Timeout for individual tool execution |
| strip_think_tags | bool | true | Strip `<think>` tags from LLM output |

#### [turn.trace]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| level | string | "off" | Global default trace level (off/minimal/basic/summary/full/debug) |
| tool | string | "minimal" | Tool category override (default: minimal) |
| phase | string | — | Phase category override (default: uses global) |
| llm | string | — | LLM category override (default: uses global) |
| meta | string | — | Meta category override (default: uses global) |
| memory | string | — | Memory category override (default: uses global) |
| context | string | — | Context category override (default: uses global) |
Available categories: phase, llm, tool, meta, memory, context.

---

### [metacognition]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| doom_loop_threshold | integer | 3 | Repeated failures before intervention |
| duration_limit_secs | integer | 86400 | Max turn duration before duration warning (soft alert, does not interrupt) |
| fatigue_threshold | float | 0.8 | Fatigue score triggering intervention |
| frame_anchoring_threshold | float | 0.5 | Frame anchoring score triggering reframe |

#### [metacognition.frame_audit]

| Key | Type | Default |
|-----|------|---------|
| goal_stagnation_threshold | integer | 5 |
| monotony_threshold | float | 0.7 |
| correction_threshold | integer | 3 |
| failure_streak_threshold | integer | 3 |
| low_confidence_threshold | float | 0.3 |

Weights (must sum to 1.0):

| Key | Default |
|-----|---------|
| goal_stagnation | 0.25 |
| tool_monotony | 0.25 |
| correction | 0.20 |
| low_confidence | 0.15 |
| failure_streak | 0.15 |

#### [metacognition.rpe]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| low_utility_threshold | float | 0.5 | Reward prediction error utility floor |
| drift_ratio_threshold | float | 10.0 | Drift ratio triggering reframe |

#### [metacognition.health_recovery]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| dimension_threshold | float | 0.7 | Per-dimension score triggering recovery |

#### [metacognition.denial]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| consecutive_threshold | integer | 3 | Consecutive denials before escalation |
| session_threshold | integer | 10 | Session-total denials before escalation |

---

### [autonomous]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| enabled | bool | true | Enable autonomous background cognition |
| heartbeat_interval_secs | integer | 10 | Heartbeat check interval |

#### [autonomous.thresholds]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| consolidate_count | integer | 5 | Pending memories before consolidation |
| deprecate_check | bool | true | Check for deprecated memories |
| embed_pending | bool | true | Embed unembedded memories |
| skill_evolve_calls | integer | 100 | Tool calls before skill evolution check |
| reflection_idle_secs | integer | 3600 | Idle seconds before reflection |
| self_update_corrections | integer | 3 | Corrections before self-update |

#### [autonomous.limits]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| max_llm_calls_per_hour | integer | 10 | LLM call budget per hour |
| max_concurrent | integer | 1 | Maximum concurrent autonomous tasks |
| cooldown_after_llm_secs | integer | 300 | Cooldown after LLM call (5 min) |

---

### [context]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| max_tokens | integer | 200000 | Context window budget |
| pressure_thresholds | array | [0.60, 0.75, 0.85, 0.95] | Four pressure levels |

Pressure levels:

1. **Normal** (< 0.60) -- monitoring only
2. **Alert** (0.60) -- compress older turns
3. **Compress** (0.75) -- aggressive compression
4. **Urgent** (0.85) -- emergency, drop low-priority content

---

### [skills]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| extra_dirs | array | [] | Additional skill directories |
| max_active_summaries | integer | 30 | Max skill summaries in context |
| default_timeout_secs | integer | 600 | Default skill execution timeout |
| inject_summaries | bool | true | Inject skill summaries into system prompt |

---

### [auth]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| enabled | bool | false | Enable JWT authentication |
| secret | string | "" | HMAC-SHA256 secret |
| token_expiry_hours | integer | 24 | Token expiry duration |

Exempt endpoints: /api/health, /api/metrics/*, /api/auth/*, static pages.

---

### [tls]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| enabled | bool | false | Enable TLS |
| cert_path | string | "" | Path to TLS certificate |
| key_path | string | "" | Path to TLS private key |

---

### [rate_limit]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| per_session_rpm | integer | 10 | Requests per minute per session |
| global_rpm | integer | 60 | Global requests per minute |

Exceeding returns HTTP 429 with Retry-After header. Exempt: /api/health, /api/metrics/*.

---

### [tools]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| disabled | array | [] | List of disabled tool names |

Hot-reloaded. Disabled tools are invisible to the LLM.

---

### [health]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| check_interval_turns | integer | 10 | Turns between health checks |
| degraded_threshold | float | 0.3 | Score below which dimension is degraded |
| weights | array | [0.25, 0.25, 0.25, 0.25] | Dimension weights |

Four dimensions: Memory fragmentation, Context pressure, Recall degradation, Fatigue.

---

### [evolution]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| source_modify_enabled | bool | false | Allow prompt source modification |

Signal weights:

| Signal | Weight |
|--------|--------|
| correction_weight | 1.0 |
| preference_weight | 0.8 |
| new_domain_weight | 0.6 |
| first_session_weight | 0.5 |
| tool_intensive_weight | 0.4 |
| long_input_weight | 0.3 |

Evolution triggers when weighted score >= 0.5.

---

### [ui]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| prompt_symbol | string | "cortex> " | REPL prompt symbol |
| locale | string | "auto" | UI locale |

---

### [memory_share]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| mode | string | "disabled" | Share mode: disabled, readonly, readwrite |
| instance_id | string | "" | Target instance ID for sharing |

---

### [media]

| Key | Type | Default | Description |
|-----|------|---------|-------------|
| stt | string | "local" | STT provider: local, openai, zai |
| tts | string | "edge" | TTS provider: edge, openai, zai |
| image_gen | string | "" | Image generation: zai, openai, "" (disabled) |
| video_gen | string | "" | Video generation: zai, "" (disabled) |
| image_understand | string | "" | Image understanding: "", zai, openai |
| video_understand | string | "" | Video understanding: zai, gemini, "" |
| api_key | string | "" | Shared media API key (empty inherits [api].api_key) |
| api_url | string | "" | Shared URL override |

Per-capability overrides:

| Key | Default | Description |
|-----|---------|-------------|
| stt_api_key | "" | STT-specific API key |
| stt_api_url | "" | STT-specific endpoint URL |
| whisper_model | "whisper" | Whisper model name |
| tts_api_key | "" | TTS-specific API key |
| tts_api_url | "" | TTS-specific endpoint URL |
| tts_voice | "zh-CN-XiaoxiaoNeural" | TTS voice name |
| image_gen_api_key | "" | Image generation API key |
| image_gen_api_url | "" | Image generation endpoint URL |
| image_gen_model | "" | Image generation model |
| image_understand_api_key | "" | Image understanding API key |
| image_understand_api_url | "" | Image understanding endpoint URL |
| image_understand_model | "" | Image understanding model |
| video_gen_api_key | "" | Video generation API key |
| video_gen_api_url | "" | Video generation endpoint URL |
| video_gen_model | "cogvideox-3" | Video generation model |
| video_understand_api_key | "" | Video understanding API key |
| video_understand_api_url | "" | Video understanding endpoint URL |
| video_understand_model | "glm-4v-plus" | Video understanding model |

Resolution order: capability_key > media.api_key > [api].api_key

---

## mcp.toml

Separate file alongside config.toml. Defines external MCP server connections.

stdio transport:

```toml
[[servers]]
name = "fs"
transport = "stdio"
command = "npx"
args = ["-y", "@mcp/server-fs"]
env = { NODE_ENV = "production" }
```

SSE transport:

```toml
[[servers]]
name = "remote"
transport = "sse"
url = "https://api.example.com/mcp"
headers = { Authorization = "Bearer token" }
```

Tool naming convention: `mcp_{server_name}_{tool_name}`

---

## providers.toml

Global provider registry. Hot-reloaded.

11 built-in providers:

| Name | Protocol | Base URL | Default Model | Vision Model | Auth |
|------|----------|----------|---------------|--------------|------|
| anthropic | anthropic | https://api.anthropic.com | claude-sonnet-4-20250514 | -- | x-api-key |
| openai | openai | https://api.openai.com | gpt-4o | gpt-4o | bearer |
| zai | anthropic | https://api.z.ai/api/anthropic | glm-5.1 | GLM-4.6V | x-api-key |
| zai-openai | openai | https://api.z.ai/api/coding/paas/v4 | glm-5.1 | GLM-4.6V | bearer |
| zai-cn | anthropic | https://open.bigmodel.cn/api/anthropic | glm-4-plus | GLM-4.6V | x-api-key |
| zai-cn-openai | openai | https://open.bigmodel.cn/api/paas/v4 | glm-4-plus | GLM-4.6V | bearer |
| kimi | openai | https://api.moonshot.cn/v1 | moonshot-v1-auto | -- | bearer |
| kimi-cn | openai | https://api.moonshot.cn/v1 | moonshot-v1-auto | -- | bearer |
| minimax | openai | https://api.minimax.chat/v1 | abab6.5s-chat | -- | bearer |
| openrouter | openai | https://openrouter.ai/api | -- | -- | bearer |
| ollama | ollama | http://localhost:11434 | -- | -- | none |

Custom provider format:

```toml
[my-provider]
name = "My Provider"
protocol = "openai"   # anthropic, openai, ollama
base_url = "https://api.example.com/v1"
auth_type = "bearer"  # bearer, x-api-key, none
models = ["model-a"]
vision_model = "model-v"
```

---

## Hot Reload

| File | Behavior |
|------|----------|
| config.toml | Applied on next turn |
| providers.toml | Applied immediately |
| prompts/*.md | Applied immediately |
| skills/*/SKILL.md | Applied on restart (new additions) or immediately (modifications) |

Uses `notify` crate with 500ms debounce.
