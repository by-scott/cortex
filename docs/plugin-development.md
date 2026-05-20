# Cortex Plugin Development Guide

Language: English | [简体中文](zh_CN/plugin-development.md)

This guide assumes you have never built a Cortex plugin before. It starts with
the vocabulary, builds a minimal process-isolated plugin, explains every file
you touch, then covers review, installation, signing, packing, publishing, and
trusted native plugins.

Related documents: [README](../README.md), [Usage Guide](usage.md)

## What A Cortex Plugin Is

A Cortex plugin is a directory with a `manifest.toml` at its root. The manifest
declares what the plugin is, which Cortex version it needs, what capabilities it
requests, and how Cortex should load it.

A plugin may provide three kinds of things:

- **Tools**: callable actions the model can invoke during a turn.
- **Skills**: `SKILL.md` procedures Cortex can inject into context or invoke
  when a task matches their activation rules.
- **Prompts**: prompt fragments packaged with the plugin.

There are two tool execution boundaries:

- **Process-isolated plugin tools**: Cortex starts a child process for each
  tool call, sends JSON to stdin, and reads JSON from stdout. Start here. You
  can write the tool in shell, Python, Rust, Go, Node.js, or anything else that
  can be executed.
- **Trusted native plugins**: Cortex loads a Rust shared library into the daemon
  process through `cortex-sdk`. Use this only when you need in-process
  performance or tight SDK integration and are ready to treat the plugin as
  trusted code.

For a first plugin, build a process-isolated plugin. You do not need Rust.

## Before You Start

Install Cortex and make sure the daemon is healthy:

```sh
cortex doctor
cortex plugin list
```

You need:

- A shell.
- A working `cortex` binary.
- A directory where you can create a plugin project.
- Rust only if you are building a trusted native plugin.

Useful terms:

- **Source directory**: your plugin project directory.
- **Installed plugin directory**: the copy under Cortex's plugin store.
- **`.cpx` package**: the archive format produced by `cortex plugin pack`.
- **Publisher key**: an Ed25519 private key used to sign package metadata.
- **SBOM**: software bill of materials metadata, usually `sbom.spdx.json`.
- **Risk profile**: package governance metadata, usually `risk.toml`.

## Build Your First Plugin

Create a process plugin scaffold:

```sh
cortex --new-process-plugin hello
cd cortex-plugin-hello
```

The scaffold creates:

```text
cortex-plugin-hello/
├── README.md
├── bin/
│   └── hello-tool
├── manifest.toml
├── prompts/
└── skills/
```

What these files mean:

- `manifest.toml` is the runtime contract. Cortex refuses malformed or unsafe
  manifests.
- `bin/hello-tool` is the executable process tool.
- `skills/` is where plugin skills go.
- `prompts/` is where packaged prompt fragments go.
- `README.md` is local documentation for the plugin project.

Review the generated plugin:

```sh
cortex plugin review .
```

Run the local conformance checks:

```sh
cortex plugin test .
```

Install it into the current Cortex instance:

```sh
cortex plugin install .
```

Directory installs are developer installs: Cortex reviews the directory, copies
it into the plugin store, and enables it for the selected instance. If the
daemon is running, process tool visibility hot-reloads shortly. Restart when in
doubt:

```sh
cortex restart
cortex plugin list
```

Try it from Cortex:

```sh
cortex "Use the hello tool with input 'world' and tell me the result."
```

The model decides whether to call the tool. Clear tool names, descriptions, and
schemas make that decision much more reliable.

## The Process Tool Protocol

For each process tool call, Cortex starts the manifest-declared command and
writes one JSON object to stdin:

```json
{
  "tool": "hello",
  "input": {
    "input": "world"
  }
}
```

The `input` object follows the tool's `input_schema` in `manifest.toml`.

The tool must write one JSON value to stdout. The simplest valid output is a
JSON string:

```json
"hello world"
```

The preferred output is an object:

```json
{
  "output": "hello world",
  "is_error": false
}
```

Rules:

- `output` must be a string.
- `is_error = true` returns a recoverable tool error visible to the model.
- A non-zero process exit becomes a tool error. If stderr has text, Cortex uses
  it as the error message.
- stdout must be valid JSON on success.
- stdout plus stderr is subject to `max_output_bytes`.
- If `timeout_secs` is reached, Cortex kills the child process and returns a
  timeout error.

The generated shell script is deliberately small. For production plugins, use a
real JSON parser in your chosen language instead of parsing JSON with regular
expressions.

## Manifest From Zero

`manifest.toml` is strict: unknown fields are rejected.

Minimal process plugin:

```toml
name = "hello"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
author = "example.dev"
cortex_version = "1.6.14"
trust = "reviewed_process"

[capabilities]
provides = ["tools", "skills"]
file_read = []
file_write = []
network = []
process = false
secrets = false
background = false

[sandbox]
level = "child_process"
network = "inherit"
filesystem = "plugin_only"

[native]
isolation = "process"

[[native.tools]]
name = "hello"
description = "Return a short greeting for the supplied input."
command = "bin/hello-tool"
inherit_env = ["PATH"]
timeout_secs = 5
max_output_bytes = 1048576
max_memory_bytes = 67108864
max_cpu_secs = 2
input_schema = { type = "object", properties = { input = { type = "string" } }, required = ["input"] }
```

Identity fields:

- `name`: plugin name. Keep it short and stable.
- `version`: plugin version.
- `description`: one-line purpose.
- `author`: publisher or maintainer identity.
- `cortex_version`: minimum Cortex runtime version required by the plugin.
  Cortex accepts concrete versions less than or equal to the running runtime.
  Version ranges are rejected.
- `trust`: governance tier. Common values are `reviewed_process` for process
  plugins and `trusted_native` for native plugins.

Capability fields:

- `provides`: must include what the package provides, such as `tools`,
  `skills`, or `prompts`.
- `file_read`: file read globs requested by plugin tools.
- `file_write`: file write globs requested by plugin tools.
- `network`: network hosts requested by plugin tools.
- `process`: whether the plugin tools spawn their own subprocesses.
- `secrets`: whether the plugin requests host secrets or inherited
  credentials.
- `background`: whether the plugin requests background or long-running
  execution.

Declare the smallest truthful capability set. These fields feed review, policy,
and operator trust decisions.

Process tool fields:

- `name`: tool name registered in Cortex. Use lowercase words with underscores.
- `description`: written for the model. Explain what the tool does, when to use
  it, and when not to use it.
- `command`: executable path. Relative paths are resolved inside the plugin
  directory.
- `args`: optional command arguments.
- `working_dir`: optional working directory. Relative paths are resolved inside
  the plugin directory.
- `allow_host_paths`: false by default. Keep it false unless the operator
  intentionally trusts host-level paths.
- `inherit_env`: explicit environment allowlist. If empty, Cortex supplies a
  minimal default containing `PATH`.
- `env`: explicit environment variables set for this tool.
- `timeout_secs`: hard timeout for the child process.
- `max_output_bytes`: maximum stdout plus stderr bytes.
- `max_memory_bytes`: Unix virtual memory limit.
- `max_cpu_secs`: Unix CPU seconds limit.
- `input_schema`: JSON Schema object for the tool input.
- `effects`: optional tool-specific effects. Plugin-level capability effects
  are always added as a floor.

## JSON Schema Basics

The model uses `input_schema` to decide the shape of tool input. Keep schemas
small and precise.

One required string:

```toml
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

String enum:

```toml
input_schema = { type = "object", properties = { format = { type = "string", enum = ["text", "json"] } } }
```

Object with two required fields:

```toml
input_schema = { type = "object", properties = { path = { type = "string" }, query = { type = "string" } }, required = ["path", "query"] }
```

Good schemas reduce hallucinated parameters. Bad schemas make tool calls noisy.

## Add A Real Tool

Add a second executable:

```sh
mkdir -p bin
cat > bin/reverse-text <<'SH'
#!/bin/sh
set -eu
request=$(cat)
text=$(printf '%s' "$request" | sed -n 's/.*"text"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p')
output=$(printf '%s' "$text" | rev)
printf '{"output":"%s","is_error":false}\n' "$output"
SH
chmod +x bin/reverse-text
```

Declare it:

```toml
[[native.tools]]
name = "reverse_text"
description = "Reverse a short text string. Use for simple text reversal requests."
command = "bin/reverse-text"
timeout_secs = 5
max_output_bytes = 65536
input_schema = { type = "object", properties = { text = { type = "string" } }, required = ["text"] }
```

Then run:

```sh
cortex plugin review .
cortex plugin test .
cortex plugin install .
cortex restart
```

Ask Cortex to use it:

```sh
cortex "Use the reverse_text tool to reverse 'cortex'."
```

## Add A Skill

Skills are markdown files with YAML frontmatter. They live at
`skills/<name>/SKILL.md`.

```text
skills/refactor-small/
└── SKILL.md
```

Example:

```markdown
---
name: refactor-small
description: Refactor a small code region while preserving behavior.
when_to_use: Use when the user asks for a narrow cleanup or simplification.
required_tools:
  - bash
execution_mode: Inline
timeout_secs: 600
tags:
  - refactor
  - quality
user_invocable: true
agent_invocable: true
version: 0.1.0
activation:
  input_patterns:
    - (?i)(refactor|cleanup|simplify)
---

# Refactor Small

${ARGS}

## Steps

1. Locate the owning module and read nearby code.
2. Make the smallest behavior-preserving edit.
3. Run the relevant check.
4. Report what changed and what was verified.
```

Supported frontmatter fields:

- `name`: required.
- `description`: required.
- `when_to_use`: optional routing guidance.
- `parameters`: optional list of `{ name, description, required }`.
- `required_tools`: optional list of runtime tools the skill expects.
- `execution_mode`: `Inline` or `Fork`.
- `timeout_secs`: optional expected timeout.
- `tags`: optional list.
- `user_invocable`: defaults to true.
- `agent_invocable`: defaults to true.
- `version`: optional skill version.
- `activation.input_patterns`: regex patterns matched against user input.
- `activation.pressure_above`: optional context pressure trigger.
- `activation.alert_kinds`: optional metacognitive alert trigger list.
- `activation.event_kinds`: optional runtime event trigger list.

`${ARGS}` is replaced with invocation arguments when the skill is rendered.

## Review And Conformance

Run review before installing or packing:

```sh
cortex plugin review .
```

Review shows:

- plugin identity
- trust tier
- package signature state
- conformance state
- requested capabilities
- recommended risk profile
- warnings
- checks

Run conformance:

```sh
cortex plugin test .
```

Conformance checks include manifest identity, version, Cortex version target,
capability declaration, native isolation boundary, process tool path safety,
command existence, output limit, timeout, working directory, and inherited
environment risks.

## Package Metadata

A releasable plugin should carry governance metadata:

- `package.toml`: publisher id, public key, hashes, signature, SBOM path, risk
  profile path, and optional conformance certificate.
- `risk.toml`: human-readable package risk profile.
- `sbom.spdx.json`: software bill of materials in SPDX JSON format.
- `conformance.toml`: optional conformance evidence if your release process
  writes one.

`cortex plugin sign` writes `package.toml`. It does not invent a private key,
SBOM, or external release process for you. Keep the signing key outside the
repository.

## Sign And Pack

Create a local Ed25519 signing key:

```sh
cortex plugin keygen ~/.config/cortex/plugin-signing/example-dev.ed25519
```

Sign the plugin:

```sh
cortex plugin sign . \
  --key ~/.config/cortex/plugin-signing/example-dev.ed25519 \
  --publisher example.dev
```

Pack it:

```sh
cortex plugin pack .
```

The default archive name is:

```text
<directory>-v<version>-<platform>.cpx
```

The archive can include:

- `manifest.toml`
- `package.toml`
- `sbom.spdx.json`
- `risk.toml`
- `conformance.toml`
- `lib/`
- `skills/`
- `prompts/`

Sign after the package contents are final. If you change the manifest, skills,
prompts, or native library after signing, sign again.

## Install Sources

Install from a local package:

```sh
cortex plugin install ./cortex-plugin-hello-v0.1.0-linux-amd64.cpx
cortex restart
cortex plugin list
```

Install from a directory during development:

```sh
cortex plugin install .
cortex restart
```

Install by repository name and version:

```sh
cortex plugin install by-scott/cortex-plugin-dev@1.6.10
```

Names without owner resolve to GitHub repositories named
`by-scott/cortex-plugin-<name>`.

## Trusted Native Plugins

Use `cortex-sdk` when a plugin must run as trusted in-process native Rust code.
Native plugins compile as `cdylib` shared libraries and export the stable
`cortex_plugin_init` ABI through `cortex_sdk::export_plugin!`.

Start with process plugins unless you know why native is required.

`Cargo.toml`:

```toml
[package]
name = "cortex-plugin-native-hello"
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
cortex-sdk = "1.6.14"
serde_json = "1"
```

`src/lib.rs`:

```rust
use cortex_sdk::prelude::*;

#[derive(Default)]
struct NativeHello;

impl MultiToolPlugin for NativeHello {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "native-hello".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example trusted native Cortex plugin".into(),
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
        let text = input["text"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing text".into()))?;
        Ok(ToolResult::success(format!("{} words", text.split_whitespace().count())))
    }
}

cortex_sdk::export_plugin!(NativeHello);
```

Native manifest:

```toml
name = "native-hello"
version = "0.1.0"
description = "Example trusted native Cortex plugin"
author = "example.dev"
cortex_version = "1.6.14"
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

Build and package:

```sh
cargo build --release
mkdir -p lib
cp target/release/libcortex_plugin_native_hello.so lib/
cortex plugin review .
cortex plugin test .
cortex plugin sign . --key ~/.config/cortex/plugin-signing/example-dev.ed25519 --publisher example.dev
cortex plugin pack .
```

Native plugin updates require `cortex restart`.

## Publishing

Publish `.cpx` packages as GitHub Release assets. A release should include the
package and a checksum file generated by your release process:

```sh
sha256sum cortex-plugin-hello-v0.1.0-linux-amd64.cpx > cortex-plugin-hello-v0.1.0-linux-amd64.cpx.sha256
```

Recommended release discipline:

- Keep `manifest.toml`, `package.toml`, `sbom.spdx.json`, and `risk.toml`
  current.
- Run `cortex plugin review .` and `cortex plugin test .` before signing.
- Sign only after the artifact contents are final.
- Do not commit private signing keys, local secrets, generated `target/`
  directories, or temporary archives.
- Bump `cortex_version` only when the plugin requires a newer runtime contract.

## Troubleshooting

| Symptom | Likely cause | Fix |
| --- | --- | --- |
| `manifest.toml` missing | Command was run outside the plugin root | `cd` into the plugin directory |
| `invalid manifest.toml` | Unknown or misspelled field | Compare against the manifest schema in this guide |
| command path fails review | Tool command escapes plugin directory or is missing | Put executables under `bin/` and keep `allow_host_paths = false` |
| command is not executable | Missing executable bit | `chmod +x bin/<tool>` |
| invalid JSON output | Tool printed logs or plain text to stdout | Print only JSON to stdout; send logs to stderr |
| tool is installed but not used | Description/schema is unclear or daemon has not reloaded | Improve tool description, run `cortex plugin list`, restart daemon |
| signature is invalid | Files changed after signing | Re-run review, test, sign, and pack |

## Design Checklist

Before publishing, answer yes to each item:

- The plugin name is short and stable.
- `cortex_version` is the minimum runtime contract actually required.
- The tool boundary is process-isolated unless native is justified.
- Every tool has a narrow JSON Schema.
- Every tool description tells the model when to use it.
- Capabilities are truthful and minimal.
- Secrets are not inherited unless declared and required.
- Tool stdout is valid JSON.
- `cortex plugin review .` is clean enough for release.
- `cortex plugin test .` passes.
- Signing key is outside the repository.
- The package is signed after final build artifacts are in place.
