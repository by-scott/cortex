# Quick Start

From zero to a running Cortex instance.

## Requirements

- Linux (x86_64)
- systemd (for service management)
- One LLM provider API key

## First Run

On first launch, Cortex runs a bootstrap conversation — a genuine first meeting between you and your instance. Bootstrap establishes the instance's initial name or unnamed state, your preferred language, work, environment, communication style, autonomy expectations, approval boundaries, and first working context. All of this initializes the Executive prompt state that shapes how the instance thinks and communicates going forward.

## Install

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | \
  CORTEX_API_KEY="your-key" \
  CORTEX_PERMISSION_LEVEL="balanced" bash -s -- install
```

The installer downloads the latest release binary, runs `cortex install`, and starts the daemon as a systemd user service.
Official prebuilt installer assets are currently published for Linux x86_64 (`linux-amd64`) only.
On macOS, ARM Linux, or other platforms, build from source until a matching release asset is published.

Environment variables must be placed on the `bash -s -- install` side of the pipe. Variables placed before `curl` apply only to the download step, not to `cortex install`.

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
  CORTEX_PERMISSION_LEVEL="balanced" \
  CORTEX_EMBEDDING_PROVIDER="openai" \
  CORTEX_EMBEDDING_MODEL="text-embedding-3-small" \
  CORTEX_BRAVE_KEY="your-brave-key" \
  CORTEX_TELEGRAM_TOKEN="your-telegram-bot-token" \
  CORTEX_QQ_APP_ID="your-qq-app-id" \
  CORTEX_QQ_APP_SECRET="your-qq-app-secret" \
  bash -s -- install && \
  "$HOME/.local/bin/cortex" browser enable && \
  "$HOME/.local/bin/cortex" plugin install by-scott/cortex-plugin-dev --yes
```

`browser enable` hot-applies immediately. Process-isolated plugin installs hot-apply too. A newly installed trusted native plugin may require a single daemon restart to load its shared library the first time.

### Build from source

```bash
docker compose run --rm dev cargo build --release
./target/release/cortex install
```

## Local Coding Demo

After the binary is available, `cortex demo` creates a local first-run fixture without starting a service. It uses the `demo` instance id by default, writes an Ollama-oriented config, keeps plugins disabled, adds a local-coding skill, and places the sample workspace outside the protected runtime root.

```bash
cortex demo
cortex doctor --id demo
cortex doctor --id demo --json
cortex policy lint --id demo
```

See [Local Coding Agent](local-coding-agent.md) for the full demo path and [Local Models](local-models.md) for Ollama/vLLM configuration.

## Install-Time Variables

Environment variables read by `cortex install`:

| Variable | Purpose |
|----------|---------|
| `CORTEX_API_KEY` | Primary provider API key |
| `CORTEX_PROVIDER` | Provider name (default: `anthropic`) |
| `CORTEX_MODEL` | Model identifier |
| `CORTEX_LLM_PRESET` | Endpoint preset: `minimal` / `standard` / `cognitive` / `full` |
| `CORTEX_PERMISSION_LEVEL` | Install-time confirmation policy: `strict` / `balanced` / `open` (defaults to `balanced`) |
| `CORTEX_EMBEDDING_PROVIDER` | Embedding provider |
| `CORTEX_EMBEDDING_MODEL` | Embedding model |
| `CORTEX_BRAVE_KEY` | Brave Search API key |
| `CORTEX_TELEGRAM_TOKEN` | Telegram bot token |
| `CORTEX_WHATSAPP_TOKEN` | WhatsApp token |
| `CORTEX_QQ_APP_ID` / `CORTEX_QQ_APP_SECRET` | QQ bot credentials |

Recommended permission levels:

- `balanced`: default and recommended for most local use. Auto-approves `Allow`, asks for `Review` and above.
- `strict`: tighter setup for cautious use. Only `Allow` runs without confirmation.
- `open`: only for a single-user, strongly trusted local machine. Auto-approves all non-blocking tools.

You can switch later without reinstalling:

```bash
cortex permission strict
cortex permission balanced
cortex permission open
```

## Verify

```bash
cortex status          # Check daemon health
cortex doctor          # Check local readiness and policy posture
cortex doctor --json   # Emit machine-readable findings and remediation hints
cortex                 # Start interactive REPL
```

`cortex status` shows the active permission mode, the most recent LLM-call context usage, and cumulative token spend. `cortex doctor` is a read-only readiness report for service state, config, provider key posture, permission mode, plugins, channels, policy findings, protected runtime roots, and local model endpoint hints. The `--json` form is for scripts and issue reports; it is still read-only.

## Browser Extension and Plugins

The official development plugin, [`by-scott/cortex-plugin-dev`](https://github.com/by-scott/cortex-plugin-dev), adds the project-maintenance tools that Cortex deliberately keeps outside the daemon core.

```bash
cortex browser enable
cortex plugin install by-scott/cortex-plugin-dev --yes
```

Packaged plugin installs are signed. `--yes` records the verified publisher key locally after signature validation; omit it in an interactive terminal if you prefer to confirm the key fingerprint manually. Restart only if the installed plugin ships a trusted native shared library that is being loaded for the first time.

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

Subscription is bound to that paired user, not to the whole platform. Pairing does not create a session by itself; the first real message after approval reuses an existing visible session for the same canonical actor when possible, otherwise it creates a new one. Later changes use:

```bash
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
```

These subscription changes hot-apply without a restart, and the watcher follows that client's active session only.

## Common Commands

```bash
cortex start                  # Start daemon
cortex stop                   # Stop daemon
cortex restart                # Restart daemon
cortex ps                     # List all instances
cortex demo                   # Create local first-run fixture
cortex status                 # Instance health
cortex doctor                 # Readiness and policy posture
cortex doctor --json          # Machine-readable readiness report
cortex permission balanced    # Hot-switch permission mode
cortex plugin list            # Installed plugins
cortex actor alias list       # Identity mappings
cortex actor transport list   # Transport bindings
```

## Next

- [Safe Use](safe-use.md) - Recommended local posture, plugin trust, and current non-goals
- [Local Coding Agent](local-coding-agent.md) - Generated demo fixture and bounded coding loop
- [Local Models](local-models.md) - Ollama and vLLM configuration
- [Configuration](config.md) — Config layout, providers, permission modes, hot reload
- [Executive](executive.md) — Prompt state, bootstrap, runtime policy context
- [Operations](ops.md) — Service lifecycle, channels, diagnostics
- [Plugins](plugins.md) — Plugin boundaries, manifests, packaging
