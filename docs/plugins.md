# Plugin Usage & Development Guide

This guide covers how to install and use plugins, and how to develop and publish your own.

---

## Overview

Plugins extend Cortex with up to 5 capability types:

| Capability | Description |
|------------|-------------|
| **Tools** | New tools available to the LLM during turns |
| **Skills** | Reasoning protocols for structured thinking |
| **Prompts** | Override built-in prompt templates |
| **LLM** | Provider backends for new LLM services |
| **Memory** | Storage backends for memory persistence |

The core runtime ships with 17 tools and 5 skills. Plugins add more. No plugins are installed by default.

---

## Installing Plugins

### Install Sources

| Source | Command |
|--------|---------|
| GitHub | `cortex plugin install owner/repo` |
| URL | `cortex plugin install https://example.com/plugin.cpx` |
| Local .cpx | `cortex plugin install ./my-plugin.cpx` |
| Local directory | `cortex plugin install ./my-plugin/` |

### Enable Per-Instance

After installing, add the plugin name to the enabled list in your instance config:

```toml
# ~/.cortex/<instance>/config.toml
[plugins]
enabled = ["my-plugin"]
```

Restart the daemon for changes to take effect.

### Managing Plugins

```bash
cortex plugin list                        # List installed plugins
cortex plugin uninstall my-plugin         # Remove but keep files
cortex plugin uninstall my-plugin --purge # Remove everything
```

### Official Plugin: cortex-plugin-dev

The [cortex-plugin-dev](https://github.com/by-scott/cortex-plugin-dev) plugin is the official development toolkit, providing tools for code navigation (tree-sitter), git operations, docker management, task tracking, HTTP, SQL, LSP, and workflow skills. Install it with:

```bash
cortex plugin install by-scott/cortex-plugin-dev
```

---

## Plugin Storage

Plugins are installed globally to `~/.cortex/plugins/<name>/` and enabled per-instance through `config.toml`.

```
~/.cortex/plugins/<name>/
  manifest.toml                   # Required: metadata and capabilities
  skills/
    <skill-name>/SKILL.md         # Skill definitions
  prompts/
    <template>.md                 # Prompt template overrides
  lib/
    lib<name>.so                  # Native shared library (optional)
```

---

## .cpx Archive Format

A `.cpx` file is a gzip-compressed tar archive containing the plugin directory structure. Create one with:

```bash
cortex plugin pack ./my-plugin/               # Output: my-plugin.cpx
cortex plugin pack ./my-plugin/ custom.cpx    # Custom output name
```

The archive must contain a `manifest.toml` at the root. All other directories (`lib/`, `skills/`, `prompts/`) are optional.

For GitHub distribution, create a release and attach the `.cpx` as a release asset. Users install with:

```bash
cortex plugin install owner/repo
```

---

## Skill Loading: 3-Tier Hierarchy

Skills are loaded through a 3-tier priority system. Higher tiers override lower ones when skill names collide:

```
system (versioned with core) < plugin (from enabled plugins) < instance (user-created + evolved)
```

| Tier | Source | Location |
|------|--------|----------|
| System | Built into the core | `skills/system/` in instance directory |
| Plugin | Loaded from enabled plugins | `~/.cortex/plugins/<name>/skills/` |
| Instance | User-created or evolved | `skills/` in instance directory (non-system) |

Plugin skills are tagged with `SkillSource::Plugin` for provenance tracking.

---

## Developing Plugins

### Step 1: Create manifest.toml

Every plugin requires a manifest declaring its metadata and capabilities:

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "What this plugin does"
author = "Your Name"
cortex_version = "0.8"

[capabilities]
provides = ["tools", "skills"]
```

#### Manifest Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique plugin identifier (used in `[plugins].enabled`) |
| `version` | Yes | Semantic version of the plugin |
| `description` | Yes | Human-readable summary |
| `author` | Yes | Author name or organization |
| `cortex_version` | Yes | Minimum Cortex version required (major.minor) |
| `provides` | Yes | Array of capability types: `tools`, `skills`, `prompts`, `llm`, `memory` |

### Step 2: Add Skills (Optional)

Create `skills/<skill-name>/SKILL.md` with YAML frontmatter:

```markdown
---
description: Short description of what this skill does
when_to_use: When the LLM should activate this skill
required_tools:
  - bash
  - read
tags:
  - analysis
  - debugging
activation:
  input_patterns:
    - "debug.*crash"
    - "why is.*failing"
  alert_kinds:
    - error_spike
  event_kinds:
    - tool_failure
parameters:
  max_depth: 5
  verbose: false
execution_mode: interactive
---

# Skill Name

Skill execution instructions go here. This is the prompt template
that guides the LLM through the reasoning protocol.
```

#### Skill Frontmatter Fields

| Field | Description |
|-------|-------------|
| `description` | Short summary for listing and discovery |
| `when_to_use` | Natural language guidance for activation |
| `required_tools` | Tools that must be available for the skill to work |
| `tags` | Categorization tags |
| `activation.input_patterns` | Regex patterns that trigger skill suggestion |
| `activation.alert_kinds` | Metacognition alert types that trigger suggestion |
| `activation.event_kinds` | System events that trigger suggestion |
| `parameters` | Default parameter values passed to the skill |
| `execution_mode` | How the skill runs (e.g., `interactive`, `autonomous`) |

### Step 3: Add Prompt Overrides (Optional)

Place Markdown files in `prompts/` to override any of the 18 built-in system templates. The filename must match the template being overridden:

`bootstrap.md`, `bootstrap-init.md`, `self-update.md`, `memory-extract.md`, `memory-consolidate.md`, `entity-extract.md`, `context-compress.md`, `context-summarize.md`, `causal-analyze.md`, `agent-readonly.md`, `agent-full.md`, `agent-teammate.md`, `hint-doom-loop.md`, `hint-fatigue.md`, `hint-frame-anchoring.md`, `hint-exploration.md`, `batch-analysis.md`, `summarize-system.md`

### Step 4: Implement Native Tools (Optional)

For plugins that provide tools, implement the `MultiToolPlugin` FFI interface. This is a core interface defined in `cortex-sdk` that allows a single shared library to expose multiple tools through one entry point.

#### cortex-sdk

The `cortex-sdk` crate is the official SDK for plugin development. It is a **standalone crate with zero dependencies on internal Cortex crates** — it defines `Tool`, `ToolResult`, `ToolError`, `MultiToolPlugin`, and `PluginInfo` traits/types from scratch (~150 lines, pure interface). It is **published to crates.io independently**, so plugin authors never need access to closed-source internal crates. The internal `cortex-turn` crate depends on `cortex-sdk` and re-exports its traits; plugins depend **only** on `cortex-sdk`.

#### The MultiToolPlugin Trait

```rust
use cortex_sdk::prelude::*;

pub trait MultiToolPlugin: Send + Sync {
    fn plugin_info(&self) -> PluginInfo;
    fn create_tools(&self) -> Vec<Box<dyn Tool>>;
}
```

Each tool implements the standard `Tool` trait:

```rust
use cortex_sdk::prelude::*;

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    fn input_schema(&self) -> serde_json::Value;
    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError>;
    fn timeout_secs(&self) -> Option<u64> { None }  // optional override
}
```

#### FFI Entry Point

Use the `export_plugin!` macro from `cortex-sdk` to generate the FFI entry point:

```rust
use cortex_sdk::prelude::*;

export_plugin!(MyPlugin);
```

This expands to the required `cortex_plugin_create_multi` C function. If you prefer manual control, the equivalent is:

```rust
#[unsafe(no_mangle)]
pub extern "C" fn cortex_plugin_create_multi() -> *mut dyn MultiToolPlugin {
    Box::into_raw(Box::new(MyPlugin::default()))
}
```

The runtime calls `cortex_plugin_create_multi()`, receives the `MultiToolPlugin` trait object, then calls `create_tools()` to register each tool into the global tool registry.

#### Cargo.toml Setup

Your plugin crate must be a `cdylib`:

```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
cortex-sdk = "1.0"
serde_json = "1"
```

#### Building the Library

```bash
cargo build --release
mkdir -p my-plugin/lib
cp target/release/libmy_plugin.so my-plugin/lib/
```

Place the compiled `.so` in the `lib/` directory of your plugin.

### Step 5: Plugin Library Lifecycle

Understanding the lifecycle is important for native plugin authors:

1. **Load**: Plugin shared libraries are loaded at daemon startup via `dlopen`
2. **Create**: The runtime calls `cortex_plugin_create_multi()` to get a `MultiToolPlugin` instance
3. **Register**: `create_tools()` is called and each tool is registered in the global tool registry
4. **Retain**: The library handle is kept alive in `runtime.plugin_libraries` for the entire daemon session
5. **Never unloaded**: Libraries are never unloaded during a daemon session -- this ensures function pointers remain valid

This means:

- Your plugin's `Drop` implementation (if any) runs only when the daemon shuts down
- Static state in the library persists across all tool invocations
- You can safely use `lazy_static` or `once_cell` for initialization

### Step 6: Test Locally

```bash
# Install from your development directory
cortex plugin install ./my-plugin/

# Enable it in config.toml
# Add "my-plugin" to [plugins].enabled

# Restart to load
cortex restart

# Verify it appears
cortex plugin list
```

### Step 7: Pack and Distribute

```bash
# Create the .cpx archive
cortex plugin pack ./my-plugin/

# Others can install directly
cortex plugin install ./my-plugin.cpx
```

For GitHub distribution:

1. Create a repository for your plugin
2. Build and pack the `.cpx` archive
3. Create a GitHub release and attach the `.cpx` as a release asset
4. Users install with `cortex plugin install owner/repo`

---

## Complete Plugin Example

A minimal plugin with one skill and one native tool:

```
my-plugin/
  manifest.toml
  skills/
    my-skill/
      SKILL.md
  lib/
    libmy_plugin.so
```

**manifest.toml:**

```toml
[plugin]
name = "my-plugin"
version = "0.1.0"
description = "Example plugin with one tool and one skill"
author = "Your Name"
cortex_version = "0.8"

[capabilities]
provides = ["tools", "skills"]
```

**skills/my-skill/SKILL.md:**

```markdown
---
description: Analyze code complexity metrics
when_to_use: When the user asks about code complexity or wants metrics
required_tools: [bash, read]
tags: [analysis, metrics]
activation:
  input_patterns:
    - "complexity"
    - "cyclomatic"
    - "code metrics"
---

# Complexity Analysis

1. Identify the target files or directory
2. Run static analysis tools to gather metrics
3. Summarize findings with actionable recommendations
```

**src/lib.rs:**

```rust
use cortex_sdk::prelude::*;
use serde_json::{json, Value};

#[derive(Default)]
pub struct MyPlugin;

impl MultiToolPlugin for MyPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "my-plugin".into(),
            version: env!("CARGO_PKG_VERSION").into(),
            description: "Example plugin with one tool".into(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(MyTool)]
    }
}

struct MyTool;

impl Tool for MyTool {
    fn name(&self) -> &'static str { "my_tool" }
    fn description(&self) -> &'static str { "Process text input" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "input": { "type": "string", "description": "The input to process" }
            },
            "required": ["input"]
        })
    }
    fn execute(&self, input: Value) -> Result<ToolResult, ToolError> {
        let text = input["input"]
            .as_str()
            .ok_or_else(|| ToolError::InvalidInput("missing 'input'".into()))?;
        Ok(ToolResult::success(format!("Processed: {text}")))
    }
}

cortex_sdk::export_plugin!(MyPlugin);
```

---

## Version Compatibility

Plugin compatibility follows these rules:

- **Major version**: must match the running Cortex major version
- **Minor version**: Cortex minor must be >= the plugin's declared minimum
- **Patch version**: ignored for compatibility checks

Example: a plugin declaring `cortex_version = "0.8"` works with Cortex 0.8.x and 0.9.x, but not with 1.0.0.

If a plugin fails to load with a version error, check `cortex_version` in `manifest.toml` against the output of `cortex --version`.

---

## Troubleshooting

### Plugin Not Appearing

1. Verify installation: `cortex plugin list` should show it
2. Check it is enabled in `config.toml` under `[plugins].enabled`
3. Restart the daemon after enabling
4. Check logs for load errors: `journalctl --user -u cortex`

### Skill Not Activating

1. Verify the skill file is at `skills/<name>/SKILL.md` inside the plugin directory
2. Check that `activation.input_patterns` match your test input
3. Ensure `required_tools` are all available in the current instance
4. Check `cortex plugin list` confirms the skill is registered

### Native Plugin Crashes

1. Rebuild with debug symbols: `cargo build` (without --release)
2. Check for memory safety issues (null pointers, use-after-free)
3. Verify the `cortex_plugin_create_multi` function returns a valid `MultiToolPlugin`
4. Ensure the library is built as `cdylib` in Cargo.toml
5. Check logs for dlopen errors or symbol resolution failures

### Version Mismatch

1. Check the `cortex_version` field in `manifest.toml`
2. Run `cortex --version` to see the current Cortex version
3. Update the plugin's `cortex_version` to match your major.minor
4. Rebuild and reinstall the plugin
