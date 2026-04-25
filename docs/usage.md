# Usage

## Run Modes

| Mode | Command | Description |
|------|---------|-------------|
| Interactive CLI | `cortex` | REPL with line editing and completion |
| Single prompt | `cortex "question"` | One turn, then exit |
| Pipe | `cat file \| cortex "summarize"` | Read stdin as context |
| Named instance | `cortex --id work` | Connect to a specific instance |
| ACP | `cortex --acp` | Agent Control Protocol mode |
| MCP server | `cortex --mcp-server` | Expose tools via Model Context Protocol |

## CLI Reference

### Service

```bash
cortex install [--system] [--id NAME] [--permission-level strict|balanced|open]
cortex uninstall [--purge] [--id NAME]
cortex start [--id NAME]
cortex stop [--id NAME]
cortex restart [--id NAME]
cortex status [--id NAME]
cortex permission [strict|balanced|open] [--id NAME]
cortex ps
```

Recommended permission modes:

- `balanced`: default and recommended. Auto-approves `Allow`, confirms `Review` and above.
- `strict`: more conservative. Only `Allow` runs without confirmation.
- `open`: most permissive. Auto-approves all non-blocking tools; keep it to a strongly trusted single-user machine.

`cortex permission` updates the current instance config and hot-applies the new mode for user services.

### Plugins

```bash
cortex plugin install owner/repo
cortex plugin install owner/repo@1.3.0
cortex plugin install ./plugin-dir
cortex plugin install ./plugin.cpx
cortex plugin enable NAME
cortex plugin disable NAME
cortex plugin uninstall NAME
cortex plugin list
cortex plugin pack ./plugin-dir
```

### Browser

```bash
cortex browser enable
cortex browser disable
cortex browser status
```

### Actors

```bash
cortex actor alias list
cortex actor alias set <from> <to>
cortex actor alias unset <from>

cortex actor transport list
cortex actor transport set <transport> <actor>
cortex actor transport unset <transport>
```

### Channels

```bash
cortex channel pair [platform]
cortex channel approve <platform> <user_id>
cortex channel approve <platform> <user_id> --subscribe
cortex channel subscribe <platform> <user_id>
cortex channel unsubscribe <platform> <user_id>
cortex channel revoke <platform> <user_id>
cortex channel policy <platform> whitelist
```

Channel subscribe/unsubscribe changes hot-apply while the daemon is running.

## Slash Commands

Three groups:

- **Control** — `/help`, `/status`, `/stop`, `/permission ...`, `/approve <id>`, `/deny <id>`.
- **Session / Config** — `/session ...`, `/config ...`.
- **Turn-bound** — Skill and prompt commands that inject into the active turn's execution context.

`/stop` executes immediately, resolves against the active actor session, interrupts the current turn, and clears pending confirmations for that turn.

Telegram and QQ prefer card-style interaction for `/help`, `/status`, `/permission`, `/session`, and `/config` where the platform supports it. Text slash commands remain as the fallback path.

## Session Ownership

Identity-based access control:

| Actor | Scope |
|-------|-------|
| `local:default` | Admin — sees all sessions |
| `user:alice` | Canonical user — sees own sessions |
| `telegram:<user_id>` | Channel actor — sees own sessions |
| `whatsapp:<user_id>` | Channel actor — sees own sessions |
| `qq:<user_id>` | Channel actor — sees own sessions |

Transports and channel actors can be aliased to canonical actors via `cortex actor alias set`, enabling cross-interface session continuity. An `http` request and a Telegram message can resolve to the same user, sharing history and memory.

Channel delivery follows platform capability. Web, SSE, WebSocket, CLI, and Telegram can receive live user-visible text. Telegram edits a live draft message and then replaces it with the final response. QQ direct turns deliver the complete final reply without an extra Cortex-generated processing bubble; QQ subscribed broadcasts ignore incremental text and send only the final `done` response. Both Telegram and QQ use button-driven permission, session, config, and status flows where the platform supports interactions.

Session subscription is explicit, per paired user, and disabled by default. Pairing prompts show two administrative choices: `cortex channel approve <platform> <user_id>` for pair-only, and `cortex channel approve <platform> <user_id> --subscribe` for pair-and-subscribe. Pairing does not create a session by itself. After approval, the first real message from that client reuses an existing visible session for the same canonical actor when one already exists; otherwise it creates a new session then. You can also enable it later with `cortex channel subscribe <platform> <user_id>` and disable it with `cortex channel unsubscribe <platform> <user_id>`. When enabled, that user's watcher follows that client's currently active session and re-subscribes when that client switches sessions; it does not mirror unrelated sessions owned by the same canonical actor. To make multiple clients share one active session, map them to the same canonical actor with `cortex actor alias set` and then switch both clients to the same session explicitly when needed.

## HTTP API

### Create session

```http
POST /api/session
```

### Standard turn

```http
POST /api/turn
Content-Type: application/json

{
  "session_id": "session-id",
  "input": "Explain the change",
  "images": [],
  "attachments": []
}
```

Responses include `response`, `response_format`, and `response_parts`. Text and media are represented as structured parts; media is delivered through the active transport rather than by text markers.

### Multimodal turns

Text-only turns use the text LLM endpoint. A turn with image attachments uses the resolved vision endpoint for the first LLM call, then stores the model's visual understanding as text for the rest of the tool loop. Subsequent tool calls and follow-up LLM calls in the same turn do not keep resending image blocks unless the user sends new media. If a vision call fails, Cortex also strips image blocks from the live history before returning the error so one bad media payload cannot poison later turns in the same session.

### Streaming turn

```http
POST /api/turn/stream
Content-Type: application/json

{
  "session_id": "session-id",
  "input": "Explain the change"
}
```

Returns a stream of server-sent events with three categories: user-visible text, observer text, and tool progress.

The final `done` event carries the same structured `response_parts` shape as the standard turn endpoint.

Before a provider request is sent, Cortex projects the live history into an API-safe message sequence. The projection preserves conversation order while repairing provider-invalid shapes such as missing tool results, orphan tool results, duplicate tool-use IDs, empty messages, and assistant-leading histories.

When context pressure reaches the configured compression threshold, Cortex writes an explicit compact boundary to the journal. The boundary records summary metadata and the full replacement message history, so deterministic replay restores the compressed conversation rather than reconstructing it from a loose summary alone.

## JSON-RPC

Available over four transports: HTTP (`/api/rpc`), Unix socket, WebSocket, and stdio.

### Methods

| Namespace | Methods |
|-----------|---------|
| Session | `session/new`, `session/prompt`, `session/list`, `session/end`, `session/initialize`, `session/cancel`, `session/get` |
| Command | `command/dispatch` |
| Skill | `skill/list`, `skill/invoke`, `skill/suggestions` |
| Memory | `memory/list`, `memory/get`, `memory/save`, `memory/delete`, `memory/search` |
| Health | `health/check` |
| Meta | `meta/alerts` |
| MCP | Bridged from JSON-RPC to MCP protocol |

## Turn Events

Streaming transports receive events on two lanes:

- **UserVisible** — Final text, tool results, and status updates intended for the end user.
- **Observer** — Internal reasoning traces, sub-turn output, and diagnostic information.

Sub-turn output stays in the observer lane of its parent turn — it does not leak to channels or user-visible streams.

## Plugin Runtime Surface

Plugins built with `cortex-sdk` participate in the turn runtime:

- Read session ID, canonical actor, source transport, execution scope
- Detect foreground vs. background context
- Emit progress updates during long operations
- Send observer text to the parent turn
- Return structured media attachments from `ToolResult`

Plugins depend only on `cortex-sdk` — zero coupling to Cortex internals.
