# Plugin Development Guide

This guide covers the supported Cortex plugin path: process-isolated tools declared in `manifest.toml` and invoked through the JSON stdin/stdout protocol.

## Overview

Cortex plugins can contribute tools, skills, prompt layers, and structured media attachments without depending on Cortex internal crates. Tool execution is process-isolated: Cortex starts the manifest-declared command for each tool call, writes a JSON request to stdin, and reads a JSON result from stdout.

The process JSON protocol is the only documented plugin execution boundary. It supports hot-reload of manifest, schema, and tool-set changes, keeps plugin commands outside the daemon process, and avoids Rust trait-object ABI coupling.

## Scaffold

```bash
cortex --new-process-plugin example
cd cortex-plugin-example
```

The scaffold creates:

```text
cortex-plugin-example/
├── manifest.toml
├── bin/
│   └── example-tool
├── skills/
├── prompts/
└── README.md
```

Replace `bin/example-tool` with your implementation and keep the manifest command path inside the plugin directory unless you explicitly set `allow_host_paths = true`.

## Manifest

Every plugin ships `manifest.toml`:

```toml
name = "example"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools", "skills"]

[native]
isolation = "process"

[[native.tools]]
name = "example"
description = "Example process-isolated tool"
command = "bin/example-tool"
args = []
working_dir = "."
inherit_env = ["PATH"]
env = { CORTEX_PLUGIN_MODE = "isolated" }
timeout_secs = 5
max_output_bytes = 1048576
max_memory_bytes = 67108864
max_cpu_secs = 2
input_schema = { type = "object", properties = { input = { type = "string" } }, required = ["input"] }
```

Rules:

- `cortex_version` is required.
- `[native].isolation` must be `"process"` for documented plugins.
- `command` and `working_dir` are relative to the plugin directory by default.
- Absolute host paths are rejected unless `allow_host_paths = true`.
- The process environment is cleared, then `inherit_env` and `env` are applied.
- `timeout_secs`, `max_output_bytes`, `max_memory_bytes`, and `max_cpu_secs` constrain each invocation.

## Protocol

Cortex sends one JSON request on stdin:

```json
{"tool":"example","input":{"input":"hello"}}
```

The tool returns either a JSON string:

```json
"Processed: hello"
```

or an object:

```json
{"output":"Processed: hello","is_error":false}
```

Set `is_error = true` when the command completed but the tool result should be treated as a failed tool call.

## Packaging

From the plugin directory:

```bash
cortex plugin pack .
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx
cortex restart
```

## Hot Reload

Process-isolated command implementation changes apply on the next tool invocation. Manifest, schema, and tool-set changes are detected by the hot-reload watcher; Cortex unregisters the plugin's previous proxy tools and registers the new manifest-declared set.

## Skills And Prompts

Place optional skills under `skills/<skill-name>/SKILL.md` and optional prompt fragments under `prompts/`. They are packaged with the plugin and loaded with the plugin manifest.
