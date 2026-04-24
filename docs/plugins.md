# Plugin Development Guide

This guide covers Cortex's two public plugin boundaries: process-isolated JSON tools and trusted native ABI tools.

## Overview

Cortex plugins can contribute tools, skills, prompt layers, and structured media attachments without depending on Cortex internal crates.

Process JSON is the default boundary: Cortex starts the manifest-declared command for each tool call, writes a JSON request to stdin, and reads a JSON result from stdout.

Trusted native ABI is the low-latency boundary for local plugins that must run inside the daemon process. Native plugins export `cortex_plugin_init`, which returns a C-compatible function table. Cortex does not load Rust trait-object symbols.

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

## Process JSON Manifest

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
```

Folder installs are supported too:

```bash
cortex plugin install ./cortex-plugin-example/
```

Local installs copy only supported plugin assets: `manifest.toml`, `lib/`, `skills/`, and `prompts/`. Hidden entries, backup directories, and unsupported extra files are ignored. If the manifest declares `[native].library` and `lib/` is missing, Cortex automatically copies the built shared library from `target/release/` or `target/debug/` into the installed plugin `lib/` directory.

## Hot Reload

Process-isolated command implementation changes apply on the next tool invocation. Manifest, schema, and tool-set changes are detected by the hot-reload watcher; Cortex unregisters the plugin's previous proxy tools and registers the new manifest-declared set.

## Trusted Native ABI

Trusted native plugins are shared libraries built against `cortex-sdk`. They are not sandboxed. Installing or replacing a trusted native shared library requires a daemon restart to load the new code; enable/disable state and manifest-driven process-plugin changes hot-apply without that restart.

```toml
name = "dev"
version = "1.2.0"
description = "Trusted native development tools"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools", "skills"]

[native]
library = "lib/libcortex_plugin_dev.so"
isolation = "trusted_in_process"
abi_version = 1
```

Rules:

- Native plugins must export `cortex_plugin_init`.
- The runtime requires `abi_version = 1`.
- Legacy symbols such as `cortex_plugin_create` and `cortex_plugin_create_multi` are rejected.
- Native plugins are strong-trust extensions: a crash or undefined behavior can affect the daemon.

## Skills And Prompts

Place optional skills under `skills/<skill-name>/SKILL.md` and optional prompt fragments under `prompts/`. They are packaged with the plugin and loaded with the plugin manifest.
