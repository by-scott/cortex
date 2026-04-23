# Cortex SDK

The official Cortex SDK types for process-isolated plugin tools.

`cortex-sdk` exposes public DTOs for tool results, runtime context, progress hooks, and structured media attachments without depending on Cortex internal crates. Plugin commands use Cortex's JSON stdin/stdout protocol; they do not exchange Rust trait objects with the daemon.

## Supported Plugin Boundary

Process-isolated tools are the supported extension boundary. Each tool is a child process configured from `manifest.toml` with a controlled working directory, environment, timeout, output limit, and optional Unix CPU/memory limits.

## Scaffold

```bash
cortex --new-process-plugin hello
cd cortex-plugin-hello
```

The generated project contains:

```text
cortex-plugin-hello/
├── manifest.toml
├── bin/
│   └── hello-tool
├── skills/
├── prompts/
└── README.md
```

## Manifest

```toml
name = "hello"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools", "skills"]

[native]
isolation = "process"

[[native.tools]]
name = "word_count"
description = "Count words in text using a child process."
command = "bin/word-count"
inherit_env = ["PATH"]
timeout_secs = 5
max_output_bytes = 1048576
max_memory_bytes = 67108864
max_cpu_secs = 2
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

## Protocol

Cortex sends:

```json
{"tool":"word_count","input":{"text":"hello world"}}
```

The process returns:

```json
{"output":"2","is_error":false}
```

Use `is_error = true` for command-level failures that should be visible as failed tool calls.

## Packaging

```bash
cortex plugin pack .
cortex plugin install ./cortex-plugin-hello-v0.1.0-linux-amd64.cpx
cortex restart
```

## Structured Media

Tool output can include structured media by returning the SDK `ToolResult` shape from a host language binding or by emitting compatible JSON. Media attachments are delivered by Cortex transports independently from the text returned to the model.
