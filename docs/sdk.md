# Cortex SDK Guide

[English](sdk.md) · [简体中文](zh_CN/sdk.md)

This guide covers the Rust SDK used by trusted native Cortex plugins. For
process-isolated JSON plugins, start with the [Plugin Development Guide](plugin-development.md)
instead.

## What the SDK Provides

`cortex-sdk` is the stable Rust surface for native plugins:

- `MultiToolPlugin` describes plugin metadata and creates tools.
- `Tool` describes a model-callable tool.
- `ToolResult` and `ToolError` define tool outcomes.
- `InvocationContext` identifies session, actor, source, and execution scope.
- `ToolRuntime` lets tools emit progress and observer text.
- `ToolEffect` and `ToolCapabilities` describe side effects for policy gates.
- `Attachment` returns image, audio, video, or file outputs.
- `export_plugin!` exposes the native ABI entry point.

The SDK does not depend on Cortex internal crates. Plugins should depend on
`cortex-sdk`, `serde`, `serde_json`, and their own implementation dependencies.

## Project Setup

Create a Rust library crate:

```sh
cargo new cortex-plugin-example --lib
cd cortex-plugin-example
```

Configure `Cargo.toml`:

```toml
[package]
name = "cortex-plugin-example"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cortex-sdk = "1.6.12"
serde_json = "1"
```

`cdylib` is required for the daemon to load the plugin. `rlib` is useful for
local checks and editor tooling.

## Implement a Tool

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct ExamplePlugin;

impl MultiToolPlugin for ExamplePlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "example".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example Cortex native plugin".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(WordCount)]
    }
}

struct WordCount;

impl Tool for WordCount {
    fn name(&self) -> &'static str {
        "word_count"
    }

    fn description(&self) -> &'static str {
        "Count words in provided text."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to count"
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(ExamplePlugin);
```

Tool descriptions are read by the model. Write them as operational guidance:
what the tool does, when to use it, and what inputs are expected.

## Use Runtime Context

Override `execute_with_runtime` when a tool needs runtime metadata or progress
events:

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    ctx: InvocationContext,
    runtime: &dyn ToolRuntime,
) -> Result<ToolResult, ToolError> {
    runtime.progress("analyzing input");
    runtime.observer_text("example", &format!("source={}", ctx.source));
    self.execute(input)
}
```

Runtime events are for observability. The returned `ToolResult` is still the
authoritative tool output.

## Declare Effects

Effects let Cortex apply risk policy before a tool runs:

```rust
fn effects(&self, input: &serde_json::Value) -> Vec<ToolEffect> {
    let path = input["path"].as_str().unwrap_or_default();
    vec![ToolEffect::file_read(path)]
}
```

Declare filesystem, process, network, or state effects honestly and precisely.
The policy system can only protect users when tools describe their blast radius.

## Return Media

Use `Attachment` for generated files:

```rust
Ok(ToolResult::success("image ready").with_media(Attachment {
    media_type: "image".into(),
    mime_type: "image/png".into(),
    url: "/tmp/example.png".into(),
    caption: Some("Example output".into()),
    size: None,
}))
```

Do not call Telegram, QQ, HTTP, or other transport APIs from a plugin. Cortex
routes media through the active transport.

## Manifest

Create `manifest.toml` next to the library:

```toml
name = "example"
version = "0.1.0"
description = "Example Cortex native plugin"
cortex_version = "1.6.12"
trust = "trusted_native"

[capabilities]
provides = ["tools"]
file_read = []
file_write = []
network = []
process = false
secrets = false
background = false

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_example.so"
isolation = "trusted_in_process"
abi_version = 1
```

`cortex_version` is the minimum Cortex runtime version the plugin supports.
`abi_version` must match the SDK native ABI version.

## Build and Package

```sh
cargo build --release
mkdir -p lib
cp target/release/libcortex_plugin_example.so lib/

cortex plugin review .
cortex plugin test .
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example.ed25519 --publisher example
cortex plugin pack .
```

Keep signing keys outside the repository.

## API Reference

Read the generated API reference on docs.rs:

<https://docs.rs/cortex-sdk/latest/cortex_sdk/>
