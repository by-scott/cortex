# Cortex Usage Guide

Language: English | [简体中文](zh_CN/usage.md)

This guide is the full operator manual for Cortex. It covers installation,
first-run initialization, daily CLI use, instances, services, configuration,
permissions, policy, plugins, channels, managed tools, dashboard access, source
builds, skill governance, and troubleshooting.

Related documents: [README](../README.md), [Plugin Development Guide](plugin-development.md)

## Operating Model

Cortex has two user-facing surfaces and one durable runtime boundary:

- The `cortex` CLI is the operator surface. It starts interactive sessions,
  sends one-shot prompts, manages systemd services, installs plugins, edits
  selected config values, and inspects runtime state.
- The daemon is the long-lived runtime. It owns RPC, sockets, HTTP, channel
  delivery, plugins, memory, journaled state, prompt layers, and the embedded
  dashboard.
- An instance is the durable boundary for identity and state. Each instance has
  its own config, data, memory, sessions, prompts, skills, channels, and enabled
  plugin list.

The default base directory is `~/.cortex`, and the default instance id is
`default`. Most daily work happens through `cortex` and `cortex "prompt"`;
service commands are for installing, restarting, inspecting, and repairing the
daemon.

Core terms:

| Term | Meaning |
| --- | --- |
| Instance | A named runtime identity under the Cortex base directory. |
| Session | A conversation scope inside an instance. |
| Actor | The identity that owns work across CLI, HTTP/RPC, socket, stdio, and channel transports. |
| Skill | A reusable `SKILL.md` procedure loaded from system, instance, or plugin skill directories, with runtime health and evolution governance. |
| Plugin | A signed and declared capability package that contributes tools, skills, prompts, or trusted native code. |
| Permission mode | The runtime rule for which tool effects can proceed without interactive confirmation. |

## Installation

The release installer downloads the GitHub release archive for the current
platform, installs the `cortex` binary into the default binary directory, then
runs `cortex install`.

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
```

Default binary destinations:

- User install: `~/.local/bin/cortex`
- System install with `--system`: `/usr/local/bin/cortex`

Useful installer environment variables:

```sh
CORTEX_VERSION=1.6.16
CORTEX_REPO=by-scott/cortex
CORTEX_INSTALL_DIR="$HOME/.local/bin"
CORTEX_INSTALL_ARGS="--id work --permission-level strict"
```

Use `--system` when the service should be installed as a system-level unit:

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --system --permission-level balanced
```

For user services that should keep running after logout, enable systemd linger:

```sh
sudo loginctl enable-linger "$USER"
```

## First-Run Provider Setup

`cortex install` reads first-run configuration from environment variables before
the daemon starts. Set provider values before running the installer or before
running `cortex install` directly.

OpenAI-compatible example:

```sh
export CORTEX_PROVIDER=openai
export CORTEX_MODEL=gpt-4.1
export CORTEX_API_KEY=sk-...
cortex install --permission-level balanced
```

Custom provider endpoint:

```sh
export CORTEX_PROVIDER=sglang
export CORTEX_BASE_URL=http://127.0.0.1:11990
export CORTEX_MODEL=qwen3.6-27b
export CORTEX_API_KEY=sk-local
cortex install --permission-level balanced
```

Embedding endpoint:

```sh
export CORTEX_EMBEDDING_PROVIDER=ollama
export CORTEX_EMBEDDING_BASE_URL=http://127.0.0.1:11434
export CORTEX_EMBEDDING_MODEL=nomic-embed-text
export CORTEX_EMBEDDING_API_KEY=sk-local
```

Search and thinking controls:

```sh
export CORTEX_BRAVE_KEY=...
export CORTEX_SHOW_THINKING=false
```

Common first-install environment variables:

| Variable | Purpose |
| --- | --- |
| `CORTEX_PROVIDER` | LLM provider name. |
| `CORTEX_MODEL` | LLM model name. |
| `CORTEX_API_KEY` | LLM API key. |
| `CORTEX_BASE_URL` | Custom provider base URL. |
| `CORTEX_LLM_PRESET` | Preset: `minimal`, `standard`, `cognitive`, or `full`. |
| `CORTEX_EMBEDDING_PROVIDER` | Embedding provider name. |
| `CORTEX_EMBEDDING_MODEL` | Embedding model name. |
| `CORTEX_EMBEDDING_BASE_URL` | Embedding provider base URL. |
| `CORTEX_EMBEDDING_API_KEY` | Embedding provider API key. |
| `CORTEX_SHOW_THINKING` | Enable provider thinking request/output when supported. |
| `CORTEX_BRAVE_KEY` | Brave Search API key. |
| `CORTEX_PERMISSION_LEVEL` | Same values as `--permission-level`. |

## First Interactive Session

Start the CLI:

```sh
cortex
```

A new instance begins in bootstrap mode. During the first conversation, Cortex
asks for the instance name, collaboration style, and durable preferences it
should carry across future sessions. Treat this as setting up how this Cortex
instance should work with you; after the identity layer is formed, later turns
use the normal prompt layers.

A good first session is short and explicit:

```text
Name this instance for my main workstation.
Prefer concise status updates, ask before destructive actions, and remember
that this workspace is for Cortex development.
```

Then ask Cortex to confirm what it knows:

```text
Summarize your instance identity, current permission mode, and available tools.
```

One-shot prompt mode:

```sh
cortex "summarize the current repository status"
```

Select an instance:

```sh
cortex --id work
cortex --id work "what are you currently configured to do?"
```

Select a base directory:

```sh
cortex --home ~/.cortex-lab --id research
```

## CLI Modes

Main modes:

```sh
cortex
cortex "question"
cortex --daemon
cortex --acp
cortex --mcp-server
```

Use the plain CLI for normal operator work. `--daemon` is the runtime service
mode used by service installation. `--acp` and `--mcp-server` expose protocol
bridges for clients that speak those protocols.

`--acp` lets another ACP-capable client connect to Cortex. To let Cortex connect
out to another ACP-capable agent, use the runtime `acp` tool.

Global options:

```sh
cortex --home <PATH>
cortex --id <ID>
cortex --new-process-plugin <NAME>
cortex --help
cortex --version
```

`--new-process-plugin` creates a process-isolated plugin scaffold in the current
directory.

## ACP Agent Clients

Cortex can actively connect to external ACP agents through the `acp` tool. Use
this when another agent runtime should own a specialized task, another
workspace, or a bounded sub-problem.

The `acp` tool has two layers. `add`, `remove`, `list`, `status`, `connect`,
`disconnect`, and `prompt` are Cortex convenience actions for managing the
client registry and active processes. ACP protocol methods are exposed directly
as tool actions: `initialize`, `authenticate`, `logout`, `providers/*`,
`session/*`, plus raw `request` and `notify` for extension methods or protocol
features that do not need a dedicated helper. `add` and `remove` persist the
client list to the instance `[acp].clients` config, so connections survive
daemon restarts.
The initialize handshake can also be controlled by the tool: use
`initialize_format`, `protocol_version`, `client_name`, and `client_version`
when an agent expects a protocol variant. `initialize_format` accepts
`standard`, `codex`, or `hybrid`. Use `standard` for normal ACP agents such as
`codex-acp`; Cortex only uses the Codex-specific shape for direct experimental
`codex exec-server` launchers.

Add a local ACP agent:

```json
{
  "action": "add",
  "agent_id": "codex",
  "command": "codex-acp",
  "args": [],
  "cwd": "/work/repo"
}
```

If you prefer the npm package without a global install, use `npx` as the
command:

```json
{
  "action": "add",
  "agent_id": "codex-npx",
  "command": "npx",
  "args": ["@zed-industries/codex-acp"],
  "cwd": "/work/repo"
}
```

Add an ACP agent reached through SSH. Cortex launches `ssh runtime` locally and
uses stdio to speak ACP with the remote command. `ssh_host` also accepts
`host:port`, `user@host:port`, and `[ipv6]:port`.

```json
{
  "action": "add",
  "agent_id": "runtime-codex",
  "ssh_host": "runtime",
  "command": "/home/scott/.local/bin/codex-acp",
  "args": [],
  "cwd": "/home/scott/project/cortex",
  "initialize_format": "standard",
  "protocol_version": "1",
  "client_name": "cortex",
  "client_version": "1.6.16"
}
```

Connect and create a session explicitly:

```json
{
  "action": "session/new",
  "agent_id": "runtime-codex",
  "cwd": "/home/scott/project/cortex"
}
```

List sessions known by the external agent:

```json
{
  "action": "session/list",
  "agent_id": "runtime-codex",
  "cwd": "/home/scott/project/cortex"
}
```

Send a prompt. If the client is not connected yet, Cortex connects first:

```json
{
  "action": "prompt",
  "agent_id": "runtime-codex",
  "prompt": "Inspect the repository and summarize the current branch state."
}
```

Call an ACP method that does not have a dedicated helper by sending its method
and params directly:

```json
{
  "action": "request",
  "agent_id": "runtime-codex",
  "method": "session/list",
  "params": {
    "cwd": "/home/scott/project/cortex"
  }
}
```

Use `list`, `status`, `disconnect`, and `remove` to inspect and manage the
runtime connection pool.

## Day-To-Day Workflow

Start with service health when something feels off:

```sh
cortex status
cortex doctor
```

Use the interactive CLI for exploratory work, longer tasks, and multi-turn
collaboration:

```sh
cortex
```

Use one-shot prompts from a shell, editor task, or script when the request is
self-contained:

```sh
cortex "review the last commit and list behavioral risks"
```

Use named instances when you want separate identity, memory, configuration, and
plugin enablement:

```sh
cortex --id work
cortex --id lab "what plugins are enabled here?"
```

When changing runtime capabilities, use this loop:

```sh
cortex plugin install dev
cortex policy lint
cortex doctor
cortex restart
```

Restart is required for trusted in-process native plugin code. Most config,
process plugin, skill, and prompt changes are hot-reloaded, but `restart` is the
clean operational reset when the runtime state is uncertain.

Review skill evolution as normal runtime maintenance:

```text
/skill list
/skill proposals
/skill accept <proposal-id-or-prefix>
/skill reject <proposal-id-or-prefix>
```

Treat generated instance skills like code: inspect the proposed `SKILL.md`,
accept the proposal when the new workflow should replace or improve an older
one, and reject it when the signal is noisy or too narrow. Accepting a proposal
updates lifecycle state; it does not delete the old source.

## Instances And Services

Install or reinstall a user service:

```sh
cortex install --permission-level balanced
cortex install --id work --permission-level strict
```

Install a system service:

```sh
cortex install --system --permission-level balanced
```

Manage service state:

```sh
cortex status
cortex start
cortex stop
cortex restart
cortex ps
```

Remove a service:

```sh
cortex uninstall
cortex uninstall --id work
cortex uninstall --purge
```

`uninstall` removes the systemd service. With `--purge`, it also deletes the
instance data.

Reset data while preserving `config.toml`:

```sh
cortex reset
cortex reset --id work --force
```

Factory reset an instance:

```sh
cortex reset --factory --force
```

Default reset clears data, memory, sessions, prompts, and skills while
preserving config. Factory reset deletes the entire instance directory and
recreates it from scratch.

## Runtime Layout

Default user layout:

```text
~/.cortex/
  providers.toml
  plugins/
  default/
    config.toml
    config.defaults.toml
    actors.toml
    mcp.toml
    data/
      cortex.sock
      cortex.db
      embedding_store.db
      memory_graph.db
      blobs/
    memory/
    sessions/
    prompts/
    skills/
    channels/
```

System instances use `/var/lib/cortex` as the base directory. `--home <PATH>`
selects another base directory; `--id <ID>` selects an instance inside that base.

The base-level `plugins/` directory stores installed plugin packages. Each
instance decides which installed plugins are enabled through its own
`config.toml`.

## Configuration

Show the config summary:

```sh
cortex config list
```

Read a section:

```sh
cortex config get api
cortex config get providers
cortex config get embedding
cortex config get turn
cortex config get plugins
```

Supported section names include:

```text
api, context, memory, embedding, metacognition, turn, autonomous,
tools, acp, providers, daemon, web, skills, auth, risk, rate_limit,
health, evolution, ui, tls, plugins, mcp, llm_groups, memory_share
```

Update supported writable keys:

```sh
cortex config set turn.show_thinking false
cortex config set turn.strip_think_tags true
cortex config set embedding.api_key sk-local
```

Writable keys:

| Key | Effect |
| --- | --- |
| `turn.show_thinking` | `true` enables provider thinking request/output. |
| `turn.strip_think_tags` | `true` hides provider thinking output. |
| `embedding.api_key` | Updates embedding provider API key. |

Config changes are written to `config.toml` and hot-reloaded when the user
daemon is running. System instance config changes may require a restart.

## Permissions

Permission modes control which tool effects can proceed without interactive
confirmation:

| Mode | Behavior |
| --- | --- |
| `strict` | Auto-approve only Allow-level effects. |
| `balanced` | Auto-approve through Review-level effects. Default. |
| `open` | Auto-approve all non-blocking tools. |

Show current mode:

```sh
cortex permission
```

Change mode:

```sh
cortex permission strict
cortex permission balanced
cortex permission open
```

System instance:

```sh
cortex permission balanced --system
```

## Policy Checks

Lint current config and enabled plugins:

```sh
cortex policy lint
```

Simulate one tool/effect decision:

```sh
cortex policy simulate bash --effect run_process:/tmp --actor user:local
cortex policy simulate web_fetch --effect network_request:https://example.com
```

Simulation options:

```sh
cortex policy simulate <tool> --actor <actor> --effect <kind:target> --background
```

Effect kinds include `read_file`, `read_secret`, `write_file`, `delete_file`,
`run_process`, `network_request`, `send_message`, `spend_money`, `deploy`,
`modify_credential`, `persist_memory`, `publish_content`, `schedule_task`,
`generate_media`, `introspect_runtime`, and `delegate_work`.

## Plugins

Install from a short name:

```sh
cortex plugin install dev
```

Short names resolve to GitHub repositories named
`github.com/by-scott/cortex-plugin-<name>`.

Install from a specific repository, release package, or directory:

```sh
cortex plugin install by-scott/cortex-plugin-dev@1.6.10
cortex plugin install ./cortex-plugin-dev-v1.6.10-linux-amd64.cpx
cortex plugin install .
```

Packaged installs require a valid Ed25519 package signature. Use `--yes` only
after reviewing a new verified publisher key.

Manage plugins:

```sh
cortex plugin list
cortex plugin enable dev
cortex plugin disable dev
cortex plugin uninstall dev
cortex plugin uninstall dev --purge
```

Review, test, sign, and pack local plugins:

```sh
cortex plugin review .
cortex plugin test .
cortex plugin keygen ~/.config/cortex/plugin-signing/example.ed25519
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example.ed25519 --publisher example.dev
cortex plugin pack .
```

Native plugin replacements require `cortex restart` because the shared library
is loaded into the daemon process. Process plugin visibility hot-reloads when
the daemon sees the config change, but restarting is a safe operational repair.

## Skills

Skills are reusable `SKILL.md` procedures loaded from system, instance, or
plugin skill directories. You normally install skills through plugins. Instance
skills live under the instance `skills/` directory and are hot-reloaded by the
daemon watcher.

The runtime also tracks skill health. Each execution updates a utility score
and a state: `strong`, `healthy`, `needs_review`, `quarantined`, or
`deprecated`. Strong and healthy skills rank higher in automatic summaries;
skills that need review rank lower; quarantined and deprecated skills are kept
for inspection but are not auto-activated by the agent.

Health states:

| State | Runtime behavior |
| --- | --- |
| `strong` | Frequently useful; preferred in summaries and automatic activation. |
| `healthy` | Normal active skill. |
| `needs_review` | Still available, but lower priority and worth inspecting. |
| `quarantined` | Preserved for review, excluded from automatic activation. |
| `deprecated` | Preserved as history or fallback, excluded from automatic activation. |

Repeated successful tool patterns can create new instance-level skill
candidates. Cortex writes a new `SKILL.md` only when it does not already exist,
then records an evolution proposal instead of overwriting older source. A
proposal can describe a new pattern, an improvement, an alternative, or a
candidate replacement for a weak skill. Accepting a proposal marks the candidate
healthy and deprecates the target; rejecting it keeps both sources unchanged.
Proposal decisions are persisted and journaled.

Proposal relations:

| Relation | Meaning |
| --- | --- |
| `new_pattern` | Cortex found a repeated workflow with no close existing skill. |
| `improves` | The candidate appears to improve an existing skill. |
| `alternative_to` | The candidate covers the same tool pattern differently. |
| `candidate_replacement` | The candidate may replace a weak or quarantined skill. |

Inspect loaded skills and proposals inside the interactive CLI:

```text
/skill list
/skill proposals
/skill accept <proposal-id-or-prefix>
/skill reject <proposal-id-or-prefix>
```

Inspect skill-related config:

```sh
cortex config get skills
```

To develop plugin skills, use the [Plugin Development Guide](plugin-development.md).

## Channels

Channels run inside the daemon when their auth material exists.

Show channel configuration hints:

```sh
cortex channel telegram
cortex channel whatsapp
cortex channel qq
cortex channel qclaw
```

Common channel environment variables:

```sh
export CORTEX_TELEGRAM_TOKEN=...
export CORTEX_WHATSAPP_TOKEN=...
export CORTEX_QQ_APP_ID=...
export CORTEX_QQ_APP_SECRET=...
export CORTEX_QQ_MARKDOWN=true
export CORTEX_QCLAW_TOKEN=...
```

QClaw is the Weixin iLink adapter. It can be configured from an existing token
or through the QR login flow:

```sh
cortex channel qclaw login
cortex restart
```

Advanced QClaw login options are available when an iLink deployment requires a
custom endpoint or routing tag:

```sh
cortex channel qclaw login --base-url https://ilinkai.weixin.qq.com --route-tag <tag>
```

Pairing and user policy:

```sh
cortex channel pair telegram
cortex channel approve telegram 123456 --subscribe
cortex channel subscribe telegram 123456
cortex channel unsubscribe telegram 123456
cortex channel revoke telegram 123456
cortex channel policy telegram whitelist
cortex channel allow telegram 123456
cortex channel deny telegram 999999
cortex channel unallow telegram 123456
cortex channel undeny telegram 999999
```

Channel policy modes:

- `pairing`: users must pair and be approved.
- `whitelist`: only allowed users can interact.
- `open`: channel is open according to that transport's auth model.

## Actor Identity

Actor aliases and transport bindings map transport-specific identities to a
canonical actor. This keeps sessions and memory ownership consistent across
HTTP, RPC, websocket, socket, stdio, and chat transports.

```sh
cortex actor alias list
cortex actor alias set telegram:123 user:scott
cortex actor alias unset telegram:123
cortex actor transport list
cortex actor transport set all user:scott
cortex actor transport unset telegram
```

`all` binds `http`, `rpc`, `ws`, `sock`, and `stdio`.

## Node.js And Browser Integration

Node.js tooling is used for MCP servers that need Node or pnpm:

```sh
cortex node status
cortex node setup
```

Browser integration configures a Chrome DevTools MCP server:

```sh
cortex browser status
cortex browser enable
cortex browser disable
```

Run `cortex doctor` after setup to check whether required local tools and paths
are visible to the daemon.

## Dashboard And RPC

`cortex status` shows service state, PID, socket path, data directory, HTTP
address, current LLM provider/model/preset, permission mode, context, and token
usage.

```sh
cortex status
```

The dashboard is served by the daemon from embedded assets and does not depend
on remote CDN JavaScript or CSS.

## Build From Source

Docker is the canonical build environment:

```sh
./scripts/build.sh
```

The build gate currently runs:

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
RUSTFLAGS="-D warnings" cargo build --workspace --all-features --locked
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --all-features --no-deps --locked
```

Run direct Docker commands when debugging:

```sh
docker compose build dev
docker compose run --rm dev cargo check --workspace --all-features --locked
```

## Troubleshooting

Run readiness checks:

```sh
cortex doctor
cortex doctor --json
```

`doctor` checks OS and systemd availability, instance paths, service and socket
state, config, provider key posture, permission mode, enabled plugins, channel
auth, policy lint findings, protected runtime root paths, and local model
endpoint hints.

Common repairs:

```sh
cortex status
cortex restart
cortex config list
cortex config get api
cortex config get providers
cortex policy lint
cortex plugin list
cortex reset --force
```

Common symptoms:

| Symptom | First checks |
| --- | --- |
| CLI cannot connect | `cortex status`, socket path, service state. |
| Model calls fail | `cortex config get api`, provider key, base URL, model name. |
| Embeddings fail | `cortex config get embedding`, endpoint, API key. |
| Plugin tools missing | `cortex plugin list`, `cortex restart`, plugin review output. |
| Channel user cannot interact | `cortex channel pair <platform>`, channel policy, allow/deny lists. |
| Browser tools unavailable | `cortex browser status`, `cortex node status`, `cortex doctor`. |
| Thinking text leaks | `cortex config get turn`, then set `turn.strip_think_tags true`. |

Start with `cortex status` and `cortex doctor`; they tell you which instance,
paths, service unit, socket, and provider config you are actually using.
