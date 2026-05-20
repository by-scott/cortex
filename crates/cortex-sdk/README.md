# Cortex SDK

The official Rust SDK for Cortex trusted native plugins.

`cortex-sdk` is the public boundary between a plugin crate and the Cortex
runtime. It defines plugin metadata, tool traits, runtime invocation context,
structured results, media attachments, effect declarations, and the stable
C-compatible native ABI used by trusted in-process plugins.

Process-isolated JSON plugins do not need this crate. They are described by a
plugin manifest and a child-process command. Use `cortex-sdk` when you are
building a reviewed native plugin that exports `cortex_plugin_init` and is
loaded into the daemon process.

## Install

```toml
[dependencies]
cortex-sdk = "1.6.12"
serde_json = "1"
```

Native plugins are built as `cdylib` shared libraries:

```toml
[lib]
crate-type = ["cdylib", "rlib"]
```

## Minimal Plugin

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct DevPlugin;

impl MultiToolPlugin for DevPlugin {
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
                "text": { "type": "string" }
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

cortex_sdk::export_plugin!(DevPlugin);
```

## Runtime Context

Tools that need session metadata, actor identity, transport/source, execution
scope, progress updates, or observer text should override
`Tool::execute_with_runtime`:

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    ctx: InvocationContext,
    runtime: &dyn ToolRuntime,
) -> Result<ToolResult, ToolError> {
    runtime.progress("working");
    runtime.observer_text("tool", &format!("session={}", ctx.session_id));
    self.execute(input)
}
```

Use runtime callbacks for operator-visible progress only. Tool return values
remain the authoritative data passed back to the model.

## Effects and Policy

Declare side effects so Cortex can apply policy and permission gates before a
tool runs:

```rust
fn effects(&self, input: &serde_json::Value) -> Vec<ToolEffect> {
    vec![ToolEffect::file_read(input["path"].as_str().unwrap_or_default())]
}
```

Good effect declarations should be specific enough for the operator to
understand the blast radius: path, network host, process command, or stateful
operation. Do not hide side effects in generic descriptions.

## Media Results

Return files through structured attachments instead of calling transport APIs:

```rust
Ok(ToolResult::success("generated image").with_media(Attachment {
    media_type: "image".into(),
    mime_type: "image/png".into(),
    url: "/tmp/output.png".into(),
    caption: Some("Generated image".into()),
    size: None,
}))
```

Cortex is responsible for delivering attachments through the active transport.

## Native ABI

The runtime loads a native plugin with `dlopen`, calls
`cortex_plugin_init`, and receives a `CortexPluginApi` function table. Rust
trait objects never cross the dynamic-library boundary. Structured values move
through UTF-8 JSON buffers allocated by the plugin and released through the SDK
ABI helpers.

`NATIVE_ABI_VERSION` is the ABI compatibility marker. Plugin manifests must set
`[native].abi_version` to the ABI version they target.

## Manifest

Native plugins also need a Cortex manifest:

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
library = "lib/libexample.so"
isolation = "trusted_in_process"
abi_version = 1
```

## Release Checklist

1. Build the shared library with `cargo build --release`.
2. Copy the library into the manifest-declared `lib/` path.
3. Run `cortex plugin review .`.
4. Run `cortex plugin test .`.
5. Sign with a private key stored outside the repository.
6. Pack with `cortex plugin pack .`.
7. Publish the `.cpx` archive and checksum as release assets.

## Documentation

- Runtime and plugin guide: <https://github.com/by-scott/cortex/blob/main/docs/plugin-development.md>
- SDK API docs: <https://docs.rs/cortex-sdk/latest/cortex_sdk/>
- Cortex releases: <https://github.com/by-scott/cortex/releases/latest>

## License

MIT.
