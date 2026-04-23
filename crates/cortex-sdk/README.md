# Cortex SDK

The official Cortex SDK facade for trusted native Cortex plugins.

`cortex-sdk` lets Rust plugin authors implement `Tool` and `MultiToolPlugin` while exporting Cortex's stable native ABI. The daemon loads a C-compatible function table through `cortex_plugin_init`; Rust trait objects stay inside the plugin library.

## Supported Plugin Boundaries

Cortex has two public plugin boundaries:

- **Process JSON** — child-process tools declared in `manifest.toml`, using stdin/stdout JSON. This is the default boundary for third-party and cross-language plugins.
- **Stable native ABI** — trusted in-process shared libraries that export `cortex_plugin_init`. This SDK is the Rust facade for that ABI.

## Process JSON Scaffold

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

## Native ABI Manifest

Trusted native plugins declare the stable native boundary explicitly:

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

The runtime does not load legacy Rust trait-object symbols. Native plugins must export `cortex_plugin_init`, which `cortex_sdk::export_plugin!` generates.
