# Quickstart

A zero-to-running guide for Cortex.

## Prerequisites

- Linux with systemd (for service management)
- API key from a supported provider: Anthropic, OpenAI, ZhipuAI (international or domestic), Kimi, MiniMax, OpenRouter, or a local Ollama instance

## Install the Binary

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install
```

Binary goes to `~/.local/bin/cortex`. Override with `CORTEX_INSTALL_DIR`.

Specific version: `bash -s install --version 1.0.0`

## Quick Setup

Environment variables read ONLY during `cortex install` to generate config.toml:

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

Example: `CORTEX_API_KEY="sk-ant-..." cortex install`

With OpenAI: `CORTEX_API_KEY="sk-..." CORTEX_PROVIDER="openai" CORTEX_MODEL="gpt-4o" cortex install`

This creates:

- config.toml with provider settings
- providers.toml (global provider registry)
- 4 prompt layers (Soul, Identity, Behavioral, User)
- System templates and cognitive skills
- A systemd user service

After install, 17 core tools and 5 cognitive skills are available. To add development tools (code navigation, git workflows, docker, etc.), install the official plugin:

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

## First Conversation -- Bootstrap

On first launch, Cortex runs a bootstrap sequence -- a genuine first encounter:

1. Exchange names (yours and the instance's)
2. Instance personality emerges through conversation
3. Share role, expertise, environment, preferences
4. Establish working agreements and goals

Not a form -- it's a conversation. Responses populate Identity and User prompt layers.

## Running Modes

- Interactive REPL: `cortex`
- Single prompt: `cortex "your question"`
- Pipe input: `cat file.txt | cortex "summarize"`
- Named session: `cortex --session name "prompt"`
- Named instance: `cortex --id work "prompt"`
- Web UI: http://127.0.0.1:PORT/ (port shown by `cortex status`)
- Dashboard: http://127.0.0.1:PORT/dashboard.html
- Audit trail: http://127.0.0.1:PORT/audit.html

## Service Management

```bash
cortex status    # Service health, HTTP address, LLM info
cortex start     # Start daemon
cortex stop      # Stop daemon
cortex restart   # Restart daemon
```

## Node and Browser Setup

MCP servers often require Node.js. Cortex provides built-in commands to manage the Node.js environment and browser integration:

```bash
cortex node setup     # Install Node.js + pnpm (for MCP servers)
cortex node status    # Show Node.js environment status
cortex browser enable # Configure Chrome DevTools MCP server
cortex browser status # Show browser integration status
```

Run `cortex node setup` before adding any MCP servers that use `npx`.

## Plugins

Cortex ships with 17 core tools. Additional tools and skills are available through plugins:

```bash
cortex plugin install owner/repo  # From GitHub
cortex plugin list                # List installed
cortex plugin uninstall name      # Remove
```

The official [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) plugin adds development tools (code navigation, git, docker, tasks, etc.) and workflow skills. See [docs/plugins.md](plugins.md) for developing your own plugins.

## MCP Servers

Add to `~/.cortex/<instance>/mcp.toml`:

```toml
[[servers]]
name = "fs"
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
```

Restart daemon after adding. Tools appear as `mcp_{server}_{tool}`.

## Multi-Instance

```bash
CORTEX_API_KEY="key" cortex install --id work
cortex --id work
cortex ps
```

Each instance has independent config, data, memory, prompts, skills, service, and socket.

## Next Steps

- Configuration: [docs/config.md](config.md)
- Usage: [docs/usage.md](usage.md)
- Operations: [docs/ops.md](ops.md)
- Plugins: [docs/plugins.md](plugins.md)
