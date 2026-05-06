# Cortex SDK

The official Rust SDK for Cortex trusted native plugins.

`cortex-sdk` is the stable Rust facade for Cortex's native ABI. It lets a plugin author implement `Tool` and `MultiToolPlugin`, then export a C-compatible `cortex_plugin_init` entry point with `export_plugin!`. The daemon loads that function table, while Rust trait objects stay inside the plugin library.

The SDK is deliberately standalone. It does not depend on `cortex-types`, `cortex-kernel`, `cortex-runtime`, or any other Cortex workspace crate. Stable DTOs for invocation context, tool results, media attachments, tool effects, and runtime progress live in this crate. Cortex converts those DTOs to internal runtime types at the plugin boundary.

Process JSON plugins do not need this crate. Use process plugins for third-party and cross-language tools. Use `cortex-sdk` when the plugin is trusted local Rust code that should run in-process for lower latency or direct runtime callbacks.

## Boundaries

| Boundary | Use when | Requires `cortex-sdk` | Isolation |
|----------|----------|-----------------------|-----------|
| Process JSON | You want a simple, cross-language, child-process tool | No | A manifest-declared command per tool call |
| Trusted native ABI | You want a Rust shared library loaded by the daemon | Yes | In-process, trusted code |

## Install The SDK

For a native plugin:

```toml
[dependencies]
cortex-sdk = "1.6.4"
serde_json = "1"
```

The plugin crate should build a shared library:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

## Path 1: Process Plugin From Scaffold To Release

Use this path when you do not need native in-process execution.

### Create The Scaffold

```bash
cortex --new-process-plugin hello
cd cortex-plugin-hello
```

The scaffold contains:

```text
cortex-plugin-hello/
├── manifest.toml
├── bin/
│   └── hello-tool
├── skills/
├── prompts/
└── README.md
```

`manifest.toml` declares the tool. `bin/hello-tool` receives one JSON request on stdin and writes one JSON result to stdout.

### Implement The Tool

The process receives:

```json
{"tool":"hello","input":{"input":"hello world"}}
```

It may return a JSON string:

```json
"Processed: hello world"
```

or an object:

```json
{"output":"Processed: hello world","is_error":false}
```

Use `is_error = true` when the command completed but the result should be treated as a failed tool call.

### Review And Test

```bash
cortex plugin review .
cortex plugin test .
```

`review` shows requested capabilities, sandbox posture, signature state, conformance state, and recommended risk policy. `test` runs the local conformance checks that should pass before a package is published.

### Sign, Pack, Publish, Install

Create a publisher key once and keep the private key outside the repository:

```bash
mkdir -p ~/.config/cortex/plugin-signing
cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
```

Sign and pack each release:

```bash
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
sha256sum cortex-plugin-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-hello-v0.1.0-linux-amd64.cpx.sha256
```

Upload the `.cpx` and `.sha256` files to a GitHub Release. Users can install the release by repository name:

```bash
cortex plugin install owner/cortex-plugin-hello
```

or by explicit version:

```bash
cortex plugin install owner/cortex-plugin-hello@0.1.0
```

For non-interactive install after reviewing the verified publisher fingerprint:

```bash
cortex plugin install owner/cortex-plugin-hello --yes
```

`--yes` does not bypass signature, hash, manifest, package, or archive safety checks. It only accepts the verified publisher key into the local trust store when policy allows that non-interactive trust decision.

## Path 2: Trusted Native Plugin From Crate To Release

Use this path when the plugin is trusted Rust code and should run inside the Cortex daemon.

### Create The Crate

```bash
cargo new --lib cortex-plugin-native-hello
cd cortex-plugin-native-hello
```

`Cargo.toml`:

```toml
[package]
name = "cortex-plugin-native-hello"
version = "0.1.0"
edition = "2024"
license = "MIT"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cortex-sdk = "1.6.4"
serde_json = "1"
```

### Implement The Plugin

`src/lib.rs`:

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct NativeHelloPlugin;

impl MultiToolPlugin for NativeHelloPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "native-hello".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example Cortex native plugin".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(WordCountTool)]
    }
}

struct WordCountTool;

impl Tool for WordCountTool {
    fn name(&self) -> &'static str {
        "word_count"
    }

    fn description(&self) -> &'static str {
        "Count words in a text string."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(NativeHelloPlugin);
```

### Add The Manifest

`manifest.toml`:

```toml
name = "native-hello"
version = "0.1.0"
description = "Example trusted native Cortex plugin"
cortex_version = "1.6.4"
trust = "trusted_native"

[capabilities]
provides = ["tools"]
secrets = false

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_native_hello.so"
isolation = "trusted_in_process"
abi_version = 1
```

The Linux shared library name is derived from the crate name with hyphens converted to underscores. For `cortex-plugin-native-hello`, Cargo writes `target/release/libcortex_plugin_native_hello.so`.

### Build, Review, Test

```bash
cargo build --release
cortex plugin review .
cortex plugin test .
```

When `lib/` is missing, Cortex can auto-resolve the native library from `target/release/` during install, signing, and packing as long as the manifest declares `[native].library`.

### Sign And Pack

```bash
mkdir -p ~/.config/cortex/plugin-signing
cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
sha256sum cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx.sha256
```

`cortex plugin sign` writes `package.toml`. `cortex plugin pack` includes supported package assets and rejects unsafe archive shapes. A packaged install verifies the signature before copying files.

### Publish And Install

Create a GitHub Release named `v0.1.0` and upload:

```text
cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx
cortex-plugin-native-hello-v0.1.0-linux-amd64.cpx.sha256
```

Users install:

```bash
cortex plugin install owner/cortex-plugin-native-hello@0.1.0
cortex restart
```

Trusted native plugins require a daemon restart after install or replacement because the daemon keeps loaded shared libraries mapped for its process lifetime.

## Runtime-Aware Tools

Tools that need runtime metadata can override `execute_with_runtime`:

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    runtime: ToolRuntime<'_>,
) -> Result<ToolResult, ToolError> {
    runtime.progress("starting work")?;
    let actor = runtime
        .context()
        .actor
        .clone()
        .unwrap_or_else(|| "unknown".to_string());
    Ok(ToolResult::success(format!("actor: {actor}; input: {input}")))
}
```

The runtime surface can expose session id, canonical actor, transport source, foreground/background scope, progress updates, observer text, declared effects, and structured media.

## Publishing `cortex-sdk`

Maintainers publish the SDK crate separately from the Cortex binary release:

```bash
cargo package -p cortex-sdk
cargo publish -p cortex-sdk
```

The package must pass the repository Docker gate before publish. Once a version is uploaded to crates.io, it cannot be overwritten.

## References

- API docs: <https://docs.rs/cortex-sdk>
- Runtime/plugin guide: <https://github.com/by-scott/cortex/blob/main/docs/plugins.md>
