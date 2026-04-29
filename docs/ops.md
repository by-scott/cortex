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
cortex permission [strict|balanced|open] [--id NAME]
cortex ps
```

`cortex ps` lists all installed instances and their current state. `cortex status` reports permission mode, last-call context usage, and cumulative token spend in addition to service health and path information. Slash-command status can also show the current session's cumulative token spend when it is invoked from a session-bound channel.

## Browser Extension

```bash
cortex node setup          # Install Node.js bridge
cortex node status         # Check bridge health
cortex browser enable      # Enable browser extension
cortex browser disable     # Disable browser extension
cortex browser status      # Check extension state
```

`browser enable` and `browser disable` hot-apply in the normal user-service path.

## Channel Operations

```bash
cortex channel pair [platform]                          # Show pair state
cortex channel approve <platform> <user_id>             # Pair only
cortex channel approve <platform> <user_id> --subscribe # Pair and subscribe this user
cortex channel subscribe <platform> <user_id>           # Enable subscription for one paired user
cortex channel unsubscribe <platform> <user_id>         # Disable subscription for one paired user
cortex channel revoke <platform> <user_id>              # Revoke access
cortex channel policy <platform> whitelist              # Set access policy
```

QQ uses the official bot reply flow. Direct user turns deliver the complete final response without an extra Cortex-generated processing bubble. When QQ is subscribed to a session initiated elsewhere, it receives only final `done` messages; incremental text is suppressed to avoid fragmented bubbles before the complete answer.

Telegram and QQ prefer card-style interaction for `/help`, `/status`, `/permission`, `/session`, and `/config` where supported. Button actions update the current card instead of spawning a new administrative message each time. Text slash commands remain available as the fallback path. QQ checks pairing before command routing, so an unpaired first `/status` receives only the pairing prompt and no command card.

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

Session subscription is explicit, per paired user, and disabled by default. Pairing prompts show both choices: `cortex channel approve <platform> <user_id>` for pair-only, and `cortex channel approve <platform> <user_id> --subscribe` for pair-and-subscribe. Pairing itself does not allocate a session. After approval, the first real message from that client reuses an existing visible session for the same canonical actor when possible; otherwise Cortex creates a new one at that point. `cortex channel subscribe <platform> <user_id>` enables a watcher for that paired user; `cortex channel unsubscribe <platform> <user_id>` disables it. The watcher follows that client's active session only, not every session owned by the same canonical actor. Local transports can join the same continuity by aliasing or binding to that actor. Use `actor alias` for identity equivalence and `actor transport` for transport-wide defaults.

Channel subscribe/unsubscribe changes hot-apply while the daemon is running. `/stop` resolves against the active actor session, interrupts the running turn, and clears any pending confirmations for that turn.

## Diagnostics

Multiple paths to the same runtime state:

| Method | Scope |
|--------|-------|
| `cortex status` | CLI — instance health, uptime, permission mode, last-call context, cumulative tokens |
| `/status` | Slash command — same data, from within a session |
| `GET /api/daemon/status` | HTTP — programmatic access |
| `GET /api/operator/dashboard?limit=80` | HTTP — operator dashboard with state, metrics, sessions, provider profiles, backlog, and normalized turn timeline |
| `command/dispatch` with `/status` | JSON-RPC — remote diagnostics |
| `operator/dashboard` | JSON-RPC — same operator dashboard over HTTP, socket, WebSocket, or stdio |

All paths reflect the same underlying state: actor mappings, session counts, transport health, memory statistics, model routing profiles, backlog, metacognition alerts, and recent journal events grouped as lifecycle, message, LLM, tool, permission, workspace, retrieval, memory, control, guardrail, scheduler, or other runtime timeline entries.

## Backup and Reset

### Key paths to back up

| Path | Contains |
|------|----------|
| `~/.cortex/<instance>/config.toml` | Instance configuration |
| `~/.cortex/<instance>/actors.toml` | Identity mappings |
| `~/.cortex/<instance>/mcp.toml` | MCP server definitions |
| `~/.cortex/<instance>/prompts/` | Custom Executive prompt files |
| `~/.cortex/<instance>/skills/` | Custom skills |
| `~/.cortex/<instance>/data/` | Journal, embeddings, memory graph, task and goal state |
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
# Authoritative gate using this repository's docker-compose.yml
./scripts/gate.sh --docker

# Release gate after the release commit exists
./scripts/gate.sh --docker --require-clean
```

Use Docker Compose through the repository entrypoints. `./scripts/gate.sh
--docker` is the standard validation command and the only release-authoritative
path. It runs the `dev` service from this repository's `docker-compose.yml`,
built from the repository `Dockerfile` on `rust:latest`. Direct host `cargo` commands may help
diagnose a failure, but they do not replace the repository Docker Compose gate.

The gate checks Rust warning suppressions, compiler warning-suppression flags,
formatting, docs/package drift, secret and personal-path patterns, strict
clippy, and the full workspace test suite. There are no ignorable warnings.

Release candidates should also produce a behavior evidence report:

```bash
docker compose run --rm dev ./scripts/release-behavior-report.sh --run
docker compose run --rm dev ./scripts/soak-fault-harness.sh --run
```

The report records targeted behavior suites for memory, retrieval/RAG, tools,
safety, operator timeline, long-task recovery, replay, and soak posture. Attach
it to the release review together with the strict gate output. The bounded
soak/fault harness covers provider, channel, SQLite, plugin, disk/config,
rate-limit/backpressure, replay determinism, and reconnect evidence. Long
24h/72h/7d soak remains a separate release attachment when available.

Manual Docker Compose equivalents for debugging individual failures inside the
same repository `dev` service:

```bash
docker compose run --rm dev cargo fmt --all --check
docker compose run --rm dev cargo clippy --workspace --all-targets --all-features -- \
  -D warnings -W clippy::pedantic -W clippy::nursery
docker compose run --rm dev cargo test --workspace --all-features
```
