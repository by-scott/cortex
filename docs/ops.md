# Operations

## Install and Uninstall

```bash
cortex install [--system] [--id NAME]
cortex uninstall [--purge] [--id NAME]
```

Default install creates a systemd user service. `--system` installs a system-wide service under a dedicated user. `--id` creates a named instance with isolated configuration, data, and service unit.

`--purge` removes all instance data including memory, sessions, and journals. Removing the last instance also cleans up the base directory (`~/.cortex/`).

## Service Lifecycle

```bash
cortex start [--id NAME]
cortex stop [--id NAME]
cortex restart [--id NAME]
cortex status [--id NAME]
cortex ps
```

`cortex ps` lists all installed instances and their current state.

## Browser Extension

```bash
cortex node setup          # Install Node.js bridge
cortex node status         # Check bridge health
cortex browser enable      # Enable browser extension
cortex browser status      # Check extension state
```

## Channel Operations

```bash
cortex channel pair [platform]                         # Show pair state
cortex channel approve <platform> <user_id>            # Pair only
cortex channel approve <platform> <user_id> --subscribe # Pair and subscribe this user
cortex channel subscribe <platform> <user_id>          # Enable subscription for one paired user
cortex channel unsubscribe <platform> <user_id>        # Disable subscription for one paired user
cortex channel revoke <platform> <user_id>             # Revoke access
cortex channel policy <platform> whitelist             # Set access policy
```

QQ uses the official bot reply flow. Direct user turns deliver the complete final response without an extra Cortex-generated processing bubble. When QQ is subscribed to a session initiated elsewhere, it receives only final `done` messages; incremental text is suppressed to avoid fragmented bubbles before the complete answer.

Channel runtime state lives under `channels/<platform>/`. Auth configuration (`auth.json`) is declarative and user-managed; policy and pairing state are runtime-managed.

## Actor Operations

```bash
cortex actor alias list
cortex actor alias set telegram:123456789 user:alice
cortex actor alias unset telegram:123456789

cortex actor transport list
cortex actor transport set all user:alice    # Bind all transports at once
cortex actor transport set http user:alice   # Bind a single transport
cortex actor transport unset http
```

Actor aliasing enables cross-interface session continuity. A Telegram message and an HTTP request from the same person resolve to the same canonical actor, sharing sessions and memory.

Session subscription is explicit, per paired user, and disabled by default. Pairing prompts show both choices: `cortex channel approve <platform> <user_id>` for pair-only, and `cortex channel approve <platform> <user_id> --subscribe` for pair-and-subscribe. `cortex channel subscribe <platform> <user_id>` enables a watcher for that paired user; `cortex channel unsubscribe <platform> <user_id>` disables it. Local transports can join the same continuity by aliasing or binding to that actor. Use `actor alias` for identity equivalence and `actor transport` for transport-wide defaults.

## Diagnostics

Multiple paths to the same runtime state:

| Method | Scope |
|--------|-------|
| `cortex status` | CLI — instance health, uptime, active sessions |
| `/status` | Slash command — same data, from within a session |
| `GET /api/daemon/status` | HTTP — programmatic access |
| `command/dispatch` with `/status` | JSON-RPC — remote diagnostics |

All paths reflect the same underlying state: actor mappings, session counts, transport health, memory statistics, and metacognition alerts.

## Backup and Reset

### Key paths to back up

| Path | Contains |
|------|----------|
| `~/.cortex/<instance>/config.toml` | Instance configuration |
| `~/.cortex/<instance>/actors.toml` | Identity mappings |
| `~/.cortex/<instance>/mcp.toml` | MCP server definitions |
| `~/.cortex/<instance>/prompts/` | Custom prompt layers |
| `~/.cortex/<instance>/skills/` | Custom skills |
| `~/.cortex/<instance>/data/` | Journal, embeddings, memory graph |
| `~/.cortex/<instance>/memory/` | Persistent memory store |
| `~/.cortex/<instance>/sessions/` | Session history |

### Reset

```bash
cortex reset                   # Reset runtime state, preserve config
cortex reset --factory         # Reset everything to install defaults
cortex reset --force           # Skip confirmation prompt
```

## Validation

```bash
# Code formatting
cargo fmt --all -- --check

# Lint
docker compose run --rm dev cargo clippy --workspace --all-targets -- \
  -D warnings -W clippy::pedantic -W clippy::nursery

# Tests
docker compose run --rm dev cargo test --workspace
```
