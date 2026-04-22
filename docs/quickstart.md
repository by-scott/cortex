# Quick Start

From zero to a running Cortex instance.

## Requirements

- Linux (x86_64)
- systemd (for service management)
- One LLM provider API key (Anthropic, OpenAI, or Ollama endpoint)

## First Run

On first launch, Cortex runs a bootstrap conversation — a genuine first meeting between you and your instance. Bootstrap establishes the instance's initial name or unnamed state, your preferred language, work, environment, communication style, autonomy expectations, approval boundaries, and first working context. All of this initializes the Executive prompt layers that shape how the instance thinks and communicates going forward.

## Install

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install
```

The installer downloads the latest release binary, runs `cortex install`, and starts the daemon as a systemd user service.

### Install variations

```bash
# Named instance (isolated config, data, and service)
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install --id work

# System service (runs under a dedicated user, survives logout)
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" bash -s -- install --system
```

### Full Experience

Use this form when you want the daemon, provider configuration, browser support, messaging credentials, and the official development plugin in one pass. Replace every placeholder with your own value; do not paste secrets into shared logs or screenshots.

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_PROVIDER="anthropic" \
  CORTEX_API_KEY="your-llm-api-key" \
  CORTEX_MODEL="your-model" \
  CORTEX_LLM_PRESET="full" \
  CORTEX_EMBEDDING_PROVIDER="openai" \
  CORTEX_EMBEDDING_MODEL="text-embedding-3-small" \
  CORTEX_BRAVE_KEY="your-brave-key" \
  CORTEX_TELEGRAM_TOKEN="your-telegram-bot-token" \
  CORTEX_QQ_APP_ID="your-qq-app-id" \
  CORTEX_QQ_APP_SECRET="your-qq-app-secret" \
  bash -s -- install && \
  "$HOME/.local/bin/cortex" browser enable && \
  "$HOME/.local/bin/cortex" plugin install by-scott/cortex-plugin-dev && \
  "$HOME/.local/bin/cortex" restart
```

### Build from source

```bash
docker compose run --rm dev cargo build --release
./target/release/cortex install
```

## Install-Time Variables

Environment variables read by `cortex install`:

| Variable | Purpose |
|----------|---------|
| `CORTEX_API_KEY` | Primary provider API key |
| `CORTEX_PROVIDER` | Provider name (default: `anthropic`) |
| `CORTEX_MODEL` | Model identifier |
| `CORTEX_LLM_PRESET` | Endpoint preset: `minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_EMBEDDING_PROVIDER` | Embedding provider |
| `CORTEX_EMBEDDING_MODEL` | Embedding model |
| `CORTEX_BRAVE_KEY` | Brave Search API key (for `web_search` tool) |
| `CORTEX_TELEGRAM_TOKEN` | Telegram bot token |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp token |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ bot credentials |

## Verify

```bash
cortex status          # Check daemon health
cortex                 # Start interactive REPL
```

## Browser Extension and Plugins

```bash
cortex browser enable
cortex plugin install by-scott/cortex-plugin-dev
cortex restart
```

## Actor Mapping

Map multiple transports to one identity for cross-interface session continuity:

```bash
cortex actor alias set telegram:123456789 user:alice
cortex actor transport set all user:alice
```

## Channel Subscription

Messaging channels require pairing first. Pairing prompts show both forms:

```bash
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
```

Subscription is bound to that paired user, not to the whole platform. Later changes use:

```bash
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
```

## Common Commands

```bash
cortex start                  # Start daemon
cortex stop                   # Stop daemon
cortex restart                # Restart daemon
cortex ps                     # List all instances
cortex status                 # Instance health
cortex plugin list            # Installed plugins
cortex actor alias list       # Identity mappings
cortex actor transport list   # Transport bindings
```

## Next

- [Configuration](config.md) — Config layout, providers, hot reload
- [Executive](executive.md) — Prompt layers, bootstrap, skills, LLM input surface
- [Operations](ops.md) — Service lifecycle, channels, diagnostics
- [Plugins](plugins.md) — SDK, manifests, distribution
