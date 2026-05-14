# Cortex

Cortex is a local-first agent runtime. It combines a CLI, daemon, plugin
runtime, messaging channels, memory, retrieval, and an embedded operator
dashboard in one Rust workspace.

## Build

Docker is the canonical build environment:

```sh
./scripts/build.sh
```

The script is intentionally strict. It checks formatting, Clippy, tests, docs,
locked dependencies, lint suppressions, pinned Docker images, and embedded
static assets.

## Workspace

- `crates/cortex-app`: CLI and operator commands.
- `crates/cortex-runtime`: daemon, HTTP/RPC, channels, plugins, and dashboard serving.
- `crates/cortex-turn`: turn orchestration, tools, skills, LLM clients, risk, and memory workflows.
- `crates/cortex-kernel`: config, persistence, journal, policy, prompt, and storage primitives.
- `crates/cortex-types`: shared wire, config, event, memory, plugin, and policy types.
- `crates/cortex-retrieval`: retrieval, reranking, evidence support, and workspace promotion.
- `crates/cortex-sdk`: public plugin SDK.
- `static`: embedded dashboard assets.

## Discipline

The repository favors a small root, explicit module boundaries, no generated
history clutter, and a single strict build command. New behavior should enter
through the owning crate, with focused tests close to the boundary being
changed.

## Architecture

- `cortex-app` owns CLI and operator workflows.
- `cortex-runtime` owns daemon transports, channels, plugins, heartbeat, and static serving.
- `cortex-turn` owns turn orchestration, LLM calls, tools, skills, risk, and memory workflows.
- `cortex-kernel` owns durable state, config, journal, policy, prompt, and storage primitives.
- `cortex-types` owns shared contracts only.
- `cortex-retrieval` stays deterministic and daemon-free.

## Security

Model output, tool output, plugin data, channel messages, memory content, and
dashboard API responses are untrusted by default. Dashboard assets are local
only. Unsafe FFI must stay isolated to plugin or ABI boundaries.
