# Operations Guide

## Installation

### Binary Install

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s install
```

The binary is installed to `~/.local/bin/cortex`.

### Instance Install

```bash
cortex install              # user-level systemd service for default instance
cortex install --system     # system-level systemd service (requires sudo)
cortex install --id work    # user-level service for named instance
```

**User-level** installs a systemd user service managed with `systemctl --user`.

**System-level** installs a systemd system service managed with `sudo systemctl`.

### Environment Variables at Install Time

Environment variables set during install are captured into the systemd service unit:

```bash
CORTEX_API_KEY="sk-..." \
CORTEX_PROVIDER="zai" \
CORTEX_MODEL="glm-5.1" \
CORTEX_LLM_PRESET="full" \
CORTEX_EMBEDDING_PROVIDER="ollama" \
CORTEX_EMBEDDING_MODEL="nomic-embed-text" \
CORTEX_BRAVE_KEY="BSA..." \
cortex install
```

---

## Service Management

### Lifecycle

```
install --> start <--> stop --> restart --> uninstall [--purge] / reset [--factory]
```

### Commands

| Command | Description |
|---------|-------------|
| `cortex install [--system] [--id ID]` | Install as systemd service |
| `cortex start [--id ID]` | Start the daemon |
| `cortex stop [--id ID]` | Stop the daemon |
| `cortex restart [--id ID]` | Restart the daemon |
| `cortex status [--id ID]` | Show daemon status |
| `cortex ps` | List all running instances |
| `cortex uninstall [--purge] [--id ID]` | Remove service; --purge deletes all data |
| `cortex reset [--factory] [--force] [--id ID]` | Reset instance data; --factory removes everything |
| `cortex node setup` | Download and configure the Node.js runtime |
| `cortex node status` | Show Node.js runtime status |
| `cortex browser enable` | Enable the headless browser subsystem |
| `cortex browser status` | Show browser subsystem status |
| `cortex help [subcommand]` | Show help |

All commands accept `--id <instance>` to target a named instance.

---

## Plugin Management

No plugins are installed by default. The core runtime ships with 17 tools and 5 skills. Additional tools and skills are available through plugins.

### Official Plugin

The [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) plugin provides development tools (code navigation, git, docker, tasks, etc.) and workflow skills:

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

### Install Sources

| Source | Example |
|--------|---------|
| GitHub | `cortex plugin install owner/repo` |
| URL | `cortex plugin install https://example.com/plugin.cpx` |
| Local .cpx | `cortex plugin install ./my-plugin.cpx` |
| Local directory | `cortex plugin install ./my-plugin/` |

### Commands

| Command | Description |
|---------|-------------|
| `cortex plugin install <source>` | Install from any of the 4 sources |
| `cortex plugin uninstall <name> [--purge]` | Remove plugin; --purge deletes all files |
| `cortex plugin list` | List installed plugins |
| `cortex plugin pack <dir> [output.cpx]` | Package a directory into a .cpx archive |

### Storage and Enablement

Plugins are installed globally to `~/.cortex/plugins/<name>/`.

Enablement is per-instance via `config.toml`:

```toml
[plugins]
enabled = ["cortex-plugin-dev"]
```

Restart the daemon after changing enabled plugins.

---

## Multi-Instance

Each named instance is fully isolated:

- Config, data, memory, prompts, skills, sessions
- Separate Unix socket
- Separate systemd service

Service naming:

| Instance | Service Name |
|----------|-------------|
| default | `cortex` |
| named | `cortex@<id>` |

---

## Directory Layout

```
~/.cortex/
  providers.toml                          # Global provider credentials
  plugins/                                # Global plugin storage (empty by default)
  default/                                # Default instance
    config.toml                           # Instance configuration
    mcp.toml                              # MCP server definitions
    data/
      cortex.db                           # Primary database
      cortex.sock                         # Unix domain socket
      embedding_store.db                  # Embedding vectors
      memory_graph.db                     # Memory relationship graph
      cron_queue.json                     # Scheduled tasks
      model_info.json                     # Cached model metadata
      vision_caps.json                    # Cached vision capabilities
      defaults.toml                       # Runtime defaults
      node/                               # Node.js runtime and modules
    memory/                               # Memory storage
    prompts/
      soul.md                             # Soul layer prompt
      identity.md                         # Identity layer prompt
      behavioral.md                       # Behavioral layer prompt
      user.md                             # User layer prompt
      .initialized                        # Initialization marker
      system/                             # 18 system prompt templates
    sessions/                             # Session data
    skills/
      system/                             # Built-in skills
        deliberate/
        diagnose/
        review/
        orient/
        plan/
    channels/
      telegram/
        auth.json                         # Bot token and mode
        paired_users.json                 # Approved users
        pending_pairs.json                # Pending approval
        policy.json                       # Access policy
      whatsapp/
        auth.json                         # Access token and config
        ...
  work/                                   # Named instance (same structure)
    config.toml
    mcp.toml
    data/
    memory/
    prompts/
    sessions/
    skills/
    channels/
```

---

## Monitoring

### CLI

- `cortex status` -- daemon uptime, session count, memory stats
- `cortex ps` -- list all running instances

### HTTP Endpoints

| Endpoint | Description |
|----------|-------------|
| `GET /api/health` | Health check (auth/rate exempt) |
| `GET /api/metrics/structured` | Structured metrics (auth/rate exempt) |
| `GET /api/daemon/status` | Daemon status and uptime |
| `GET /api/audit/summary` | Audit event summary |
| `GET /api/audit/health` | Audit health score |

### Web Interfaces

All embedded in the binary, no external dependencies:

| URL | Description |
|-----|-------------|
| `/` | Chat interface |
| `/dashboard.html` | Metrics and status dashboard |
| `/audit.html` | Decision audit log viewer |

---

## Heartbeat Engine

The daemon runs a background heartbeat loop that performs maintenance tasks in priority order.

### Non-LLM Actions

Run unconditionally when due:

- Memory consolidation
- Memory decay processing
- Pending embedding generation
- Skill evolution checks
- State checkpointing

### LLM Actions

Throttled to avoid excessive API usage:

- Prompt evolution
- Deep reflection
- Cron task execution

### Salience Ordering

Lower value = higher priority:

| Task | Salience |
|------|----------|
| DeprecateExpired | 10 |
| EmbedPending | 20 |
| Consolidate | 30 |
| EvolveSkills | 40 |
| Checkpoint | 50 |
| CronDue | 60 |
| SelfUpdate | 70 |
| DeepReflection | 80 |

---

## Networking

### Transports

| Transport | Binding | Use |
|-----------|---------|-----|
| HTTP | `127.0.0.1:PORT` | Web UI, REST API, SSE, WebSocket |
| Unix socket | `~/.cortex/<instance>/data/cortex.sock` | CLI, ACP/MCP bridge (mode 0700) |
| stdio | stdin/stdout | ACP and MCP protocols |

### CORS

- Allowed origins: localhost only
- Explicit method and header allowlists

### Signal Handling

- `SIGHUP`: graceful handling (log and continue, no restart)

---

## Backup and Recovery

### While Running

Use SQLite's `.backup` command for consistent database snapshots:

```bash
sqlite3 ~/.cortex/default/data/cortex.db ".backup /path/to/backup.db"
```

### While Stopped

Full directory copy:

```bash
cp -r ~/.cortex/default /path/to/backup/
```

### Recovery

Replace the instance directory with the backup and restart:

```bash
cortex stop
rm -rf ~/.cortex/default
cp -r /path/to/backup/ ~/.cortex/default
cortex start
```

---

## Security

### Network Isolation

- HTTP binds to `127.0.0.1` only (not exposed to network)
- Unix socket created with mode `0700` (owner-only access)

### HTTP Security Headers

- `X-Content-Type-Options: nosniff`
- `X-Frame-Options: DENY`
- `Referrer-Policy: strict-origin`

### Input Validation

- `session_id`: alphanumeric, hyphens, underscores, dots; max 256 characters
- Request body limit: 2 MB
- JSON content type required

### Optional Security Features

- **JWT authentication**: enable via `[auth]` config section
- **TLS**: enable via `[tls]` config section
- **Rate limiting**: configurable via `[rate_limit]` config section

### Tool Risk Assessment

Every tool invocation is evaluated on 4 axes:

- **Scope**: how much of the system is affected
- **Reversibility**: can the action be undone
- **Side effects**: external or persistent changes
- **Data sensitivity**: access to sensitive information

---

## Troubleshooting

### Viewing Logs

```bash
journalctl --user -u cortex          # default instance
journalctl --user -u cortex@work     # named instance
```

### Common Issues

**Daemon not responding**

Check if the socket file exists:

```bash
ls -la ~/.cortex/default/data/cortex.sock
```

If missing, the daemon is not running. Start it with `cortex start`.

**Port conflict**

If the HTTP port is already in use, set address to `0.0.0.0:0` in config to auto-assign a free port, or choose a different port in `[daemon].addr`.

**Stale data**

Reset non-critical runtime data:

```bash
cortex reset
```

**Full factory reset**

Remove all instance data and regenerate from defaults:

```bash
cortex reset --factory --force
```

**Embedding model unavailable**

Verify the embedding provider is reachable and the model name is correct in `[embedding]` config. Check logs for connection errors.
