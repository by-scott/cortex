# Cortex SDK

The official Rust SDK for building native Cortex plugins.

`cortex-sdk` is intentionally small: it exposes the public plugin surface, tool traits, runtime metadata, progress hooks, and structured media attachments without depending on Cortex internal crates. A plugin built with this crate can run as a trusted in-process shared library; plugins can also declare process-isolated tools in `manifest.toml` that use Cortex's JSON stdin/stdout protocol without exchanging Rust trait objects.

In-process native plugins are trusted code. They run inside the daemon process and share the Rust trait-object ABI boundary with Cortex. The SDK is a source-level compatibility boundary for plugin authors; in-process manifests should declare `sdk_version` and `abi_revision`, and Cortex rejects incompatible values before loading. Process-isolated tools are child processes with a restricted environment, timeout, working directory, and output limit configured from the manifest.

## What You Build

A Cortex plugin can contribute:

- **Tools** that the model can call during a turn.
- **Skills** stored as `SKILL.md` files.
- **Prompt layers** loaded from a plugin `prompts/` directory.
- **Media attachments** returned from tools as image, audio, video, or file outputs.

The SDK covers the Rust side: plugin entry point, tool definitions, execution results, runtime context, and media DTOs. Packaging and installation are handled by the `cortex` CLI.

## From Zero To Plugin

### 1. Install Cortex

Install Cortex first, because the CLI provides the scaffold, packer, installer, and local test surface:

```bash
curl -sSf https://raw.githubusercontent.com/by-scott/cortex/main/scripts/cortex.sh | bash -s -- install
```

Check the binary:

```bash
cortex --version
```

### 2. Scaffold A Plugin

Use the built-in scaffold command:

```bash
cortex scaffold hello
cd cortex-plugin-hello
```

The generated project contains:

```text
cortex-plugin-hello/
├── Cargo.toml
├── manifest.toml
├── src/
│   └── lib.rs
├── skills/
├── prompts/
└── README.md
```

The scaffold is deliberately minimal. Keep the layout, then replace the example tool with your domain-specific tools.

### 3. Understand `Cargo.toml`

A native plugin is a Rust `cdylib`:

```toml
[package]
name = "cortex-plugin-hello"
version = "0.1.0"
edition = "2024"

[lib]
crate-type = ["cdylib"]

[dependencies]
cortex-sdk = "1.0"
serde_json = "1"
```

Only depend on `cortex-sdk` for Cortex integration. Do not depend on Cortex internal crates; that would couple your plugin to runtime internals and break source-level distribution stability.

### 4. Understand `manifest.toml`

Every plugin ships a manifest:

```toml
name = "hello"
version = "0.1.0"
description = "Example Cortex plugin"
cortex_version = "1.1.0"

[capabilities]
provides = ["tools", "skills"]

[native]
library = "lib/libcortex_plugin_hello.so"
entry = "cortex_plugin_create_multi"
sdk_version = "1.1.0"
abi_revision = 1
```

Rules:

- The repository is usually named `cortex-plugin-{name}`.
- The manifest `name` is `{name}` without the `cortex-plugin-` prefix.
- `Cargo.toml` version and `manifest.toml` version should match.
- `[native].library` is the path inside the installed plugin directory.
- `[native].sdk_version` should match the SDK major/minor used to build an in-process plugin.
- `[native].abi_revision` must match the daemon's in-process ABI revision.

For a process-isolated tool plugin, declare tools directly:

```toml
name = "hello-process"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.1.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "word_count"
description = "Count words in text using a child process."
command = "bin/word-count"
inherit_env = ["PATH"]
timeout_secs = 5
max_output_bytes = 1048576
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

Cortex sends `{"tool":"word_count","input":{...}}` on stdin. The command returns either a JSON string or `{"output":"...","is_error":false}` on stdout.

## Minimal Tool

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct HelloPlugin;

impl MultiToolPlugin for HelloPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "hello".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example Cortex plugin".into(),
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
        "Count words in a text string. Use when the user asks for word counts or text length."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": {
                    "type": "string",
                    "description": "Text to count words in"
                }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing 'text'".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(HelloPlugin);
```

## Runtime-Aware Tools

Use `execute_with_runtime` when a tool needs session metadata, progress events, observer text, or foreground/background scope:

```rust
fn execute_with_runtime(
    &self,
    input: serde_json::Value,
    runtime: &dyn ToolRuntime,
) -> Result<ToolResult, ToolError> {
    let ctx = runtime.invocation();
    let actor = ctx.actor.clone().unwrap_or_else(|| "unknown".to_string());

    runtime.emit_progress("starting work");
    runtime.emit_observer(Some("hello"), format!("actor={actor}"));

    self.execute(input)
}

fn capabilities(&self) -> ToolCapabilities {
    ToolCapabilities {
        emits_progress: true,
        emits_observer_text: true,
        background_safe: false,
    }
}
```

Stateful tools should namespace state by actor or session:

```rust
fn namespace(ctx: &InvocationContext) -> String {
    ctx.actor
        .clone()
        .or_else(|| ctx.session_id.clone())
        .unwrap_or_else(|| "global".to_string())
}
```

## Returning Media

Tools can return structured media without channel-specific code:

```rust
Ok(ToolResult::success("image ready").with_media(Attachment {
    media_type: "image".into(),
    mime_type: "image/png".into(),
    url: "/absolute/path/to/image.png".into(),
    caption: Some("Generated preview".into()),
    size: None,
}))
```

Cortex delivers attachments through the active client: HTTP, WebSocket, Telegram, QQ, or another transport. Plugins should not call Telegram, QQ, or browser APIs directly.

## Add A Skill

Create `skills/review/SKILL.md`:

```markdown
---
description: Review code changes for correctness and regressions
when_to_use: Use when the user asks for review, audit, or PR feedback.
required_tools:
  - grep
  - read_file
activation:
  input_patterns:
    - (?i)(review|audit|代码审查)
---

# Review

${ARGS}

Find correctness issues first. Report findings with file and line references.
```

Skills are loaded with the plugin when `manifest.toml` declares `skills`.

## Build

From the plugin repository:

```bash
cargo build --release
```

For a Linux release build in Cortex's Docker environment:

```bash
docker compose -f /path/to/cortex/docker-compose.yml run --rm \
  -v "$PWD:/plugin" -w /plugin dev cargo build --release
```

## Local Install

During development, install from the plugin directory:

```bash
cortex plugin install .
cortex restart
cortex plugin list
```

If you staged a plugin directory manually, its shape should be:

```text
my-plugin/
├── manifest.toml
├── lib/
│   └── libcortex_plugin_hello.so
├── skills/
└── prompts/
```

## Package

Use the Cortex packer. Do not hand-roll `.cpx` archives:

```bash
cortex plugin pack .
```

The packer reads `manifest.toml`, resolves the native library from `target/release/`, and includes `skills/` and `prompts/` if present.

The output name is:

```text
{repository}-v{version}-{platform}.cpx
```

Example:

```text
cortex-plugin-hello-v0.1.0-linux-amd64.cpx
```

## Publish A Plugin

Create a GitHub release and attach the `.cpx` asset:

```bash
git tag v0.1.0
git push origin main --tags
gh release create v0.1.0 \
  ./cortex-plugin-hello-v0.1.0-linux-amd64.cpx \
  --title "cortex-plugin-hello v0.1.0" \
  --notes "Initial release."
```

Users can install latest or a pinned version:

```bash
cortex plugin install your-name/cortex-plugin-hello
cortex plugin install your-name/cortex-plugin-hello@1.1.0
```

## Publish The SDK

This section is for Cortex maintainers publishing `cortex-sdk` itself.

Checklist:

```bash
cargo fmt --all -- --check
cargo clippy -p cortex-sdk --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery
cargo test -p cortex-sdk
cargo publish -p cortex-sdk --dry-run
cargo publish -p cortex-sdk
```

Rules:

- The SDK must remain independent from Cortex internal crates.
- Public types should be stable DTOs or traits.
- Avoid exposing runtime internals through the SDK.
- Update this README and `docs/plugins.md` when the public surface changes.

## Reference

Important SDK items:

| Item | Purpose |
|---|---|
| `MultiToolPlugin` | Plugin entry point returning metadata and tools |
| `Tool` | Tool interface: name, description, schema, execute |
| `ToolResult` | Text/error output plus optional media attachments |
| `ToolError` | Invalid input or execution failure |
| `Attachment` | Stable image/audio/video/file DTO |
| `InvocationContext` | Session, actor, source, and execution scope |
| `ToolRuntime` | Progress and observer bridge |
| `ToolCapabilities` | Declares progress, observer, and background safety |
| `PluginInfo` | Name, version, description |
| `export_plugin!` | Exports the FFI entry point |
