<div align="center">

# Cortex

**A cognitive runtime harness for durable, governed AI agents.**

[English](README.md) · [简体中文](README.zh.md)

![Version](https://img.shields.io/badge/version-1.6.9-blue)
[![License](https://img.shields.io/badge/license-MIT-green)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.95.0-orange)](Dockerfile)
[![Build](https://img.shields.io/badge/build-Docker-informational)](scripts/build.sh)
[![SDK](https://img.shields.io/badge/SDK-1.6.9-lightgrey)](crates/cortex-sdk)

</div>

Cortex belongs to the same broad family as modern agent harnesses such as
Claude Code, Codex, OpenClaw, and other tool-using coding runtimes: systems
that turn an LLM from a conversational model into an operator with files,
tools, memory, policy, and feedback.

Cortex's bet is that long-running agents need more than a larger prompt and a
larger tool list. A harness built for real multi-session work needs a runtime
model where attention, memory, permissions, channels, plugins, and side effects
are explicit operational objects. Cortex implements that model as an
operator-owned runtime:
agents should be able to preserve continuity across sessions, coordinate tools,
remember responsibly, expose what happened, and let humans govern consequential
actions.

The design is grounded in cognitive science and production runtime practice:
global workspace theory, working memory, complementary learning systems,
metacognition, hierarchical control, event sourcing, durable execution, and
explicit trust boundaries. These ideas are treated as engineering constraints,
not as decoration.

## Why Cortex Exists

Mature harnesses have already proven the core interaction pattern: let the model
inspect a workspace, call tools, iterate on feedback, and collaborate with the
operator. Cortex starts from that baseline and focuses on the runtime problems
that appear after the first impressive demo: continuity, governance, memory
quality, channel identity, plugin trust, and operational inspection.

Cortex treats the agent as a runtime concern:

- The foreground turn is a limited attention channel, not an unbounded stream.
- Memory is captured, materialized, and stabilized instead of being appended as
  loose notes.
- Tool effects pass through policy, risk, and permission gates.
- Runtime state is journaled so behavior can be inspected and recovered.
- Plugins and channels are explicit boundaries with declared capabilities.

## Capabilities

- Interactive CLI, single-prompt pipe mode, daemon mode, ACP bridge, and MCP
  server mode.
- systemd user or system service deployment with multiple named instances.
- LLM and embedding provider configuration, including custom provider endpoints.
- Permission modes for strict, balanced, and open tool confirmation behavior.
- Policy linting and simulation for tool effects.
- Plugin installation, review, conformance testing, signing, packing, and
  runtime enablement.
- Messaging channel pairing and policy for supported transports.
- Managed Node.js and browser integration helpers for MCP-based workflows.
- Local dashboard assets served by the daemon without remote CDN dependencies.

## Usage

Quick start:

```sh
curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
cortex doctor
cortex
```

For a provider-backed first install:

```sh
export CORTEX_PROVIDER=openai
export CORTEX_MODEL=gpt-4.1
export CORTEX_API_KEY=sk-...

curl -fsSL https://raw.githubusercontent.com/by-scott/cortex/main/scripts/install.sh | bash -s -- --permission-level balanced
```

After the daemon is installed, start the interactive CLI with `cortex` and
complete the first-run initialization in conversation. Use `cortex "question"`
for a one-shot prompt.

## Documentation

- [Usage Guide](docs/usage.md)
- [Plugin Development Guide](docs/plugin-development.md)

## Architecture

Cortex is split into small Rust crates with clear ownership:

- `crates/cortex-app`: CLI, deployment commands, service management, and
  operator workflows.
- `crates/cortex-runtime`: daemon, HTTP/RPC, channels, plugins, dashboard
  serving, and runtime orchestration.
- `crates/cortex-turn`: turn orchestration, LLM calls, tools, skills, risk, and
  memory workflows.
- `crates/cortex-kernel`: config, persistence, journal, policy, prompt, and
  storage primitives.
- `crates/cortex-types`: shared wire, config, event, memory, plugin, and policy
  contracts.
- `crates/cortex-sdk`: public Rust SDK for trusted native plugins.
- `static`: embedded dashboard assets.

## Security Model

Cortex assumes that model output, tool output, plugin data, channel messages,
network content, memory content, and dashboard API responses are untrusted until
they cross an explicit boundary. Security-sensitive behavior is designed around
fail-closed permission checks, declared plugin capabilities, policy simulation,
audit-friendly events, and embedded dashboard assets.

Trusted native plugins run in process and must be reviewed, tested, signed, and
packed before distribution. Process-isolated plugins use child-process JSON
tools with manifest-declared command, timeout, environment, and filesystem
rules.

## Development

Docker is the canonical development environment:

```sh
./scripts/build.sh
```

The build gate runs formatting, strict Clippy, workspace build, docs with warnings
denied, and repository hygiene checks. Cargo commands use locked dependencies,
and the Docker base image pins the Rust toolchain.

## License

Cortex is licensed under the [MIT License](LICENSE).
