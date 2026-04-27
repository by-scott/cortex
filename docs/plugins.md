# Plugin Development Guide

Cortex plugins are governed packages. A plugin does not only add a tool name; it declares the capabilities, effects, trust tier, sandbox expectation, package metadata, and conformance state that the runtime and operator can review before use.

This guide covers the two public plugin boundaries:

- **Process JSON**: the default external boundary for process-isolated JSON tools. Cortex starts a manifest-declared command for each tool call, sends JSON over stdin, and reads a JSON result from stdout.
- **Trusted native ABI**: the trusted native ABI boundary for low-latency in-process local code built against `cortex-sdk`. Native plugins export `cortex_plugin_init` and return a C-compatible function table. Cortex does not load Rust trait-object symbols.

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

Replace `bin/example-tool` with your implementation. Keep the manifest command path inside the plugin directory unless the operator explicitly accepts `allow_host_paths = true`.

## Manifest

Every plugin ships `manifest.toml`. The manifest is the package contract: identity, compatibility, capability request, sandbox profile, package metadata, and tool declarations.

```toml
name = "example"
version = "0.1.0"
description = "Example process-isolated Cortex plugin"
cortex_version = "1.5.5"
trust = "reviewed_process"

[capabilities]
provides = ["tools", "skills"]
file_read = ["project/**"]
file_write = ["project/src/**"]
network = ["api.github.com"]
process = true
secrets = false
background = false

[sandbox]
level = "child_process"
network = "allowlist"
filesystem = "declared_paths"
writable_paths = ["project/src"]
seccomp = ""
uid_drop = false
memory_mb = 256
cpu_seconds = 10

[package]
publisher_id = "example.dev"
manifest_sha256 = ""
binary_sha256 = ""
signature = ""
sbom = "sbom.spdx.json"
risk_profile = "risk.toml"

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

[[native.tools.effects]]
kind = "write_file"
target = "project/src/**"
reversibility = "partially_reversible"
confirmation = "always"
dry_run = "supported"
```

Rules:

- `cortex_version` is required and checked before library probing.
- `trust = "reviewed_process"` is the normal tier for reviewed process plugins.
- `trust = "trusted_native"` is required for `trusted_in_process` native plugins.
- `trust = "disabled"` or `trust = "quarantined"` prevents loading.
- Unreviewed plugins cannot request `secrets = true`.
- The runtime rejects manifest claims for sandbox enforcement it does not provide: `sandbox.level = "uid_no_network"`, `system_sandbox`, `container_vm`, `remote_worker`, `sandbox.network = "none"`, non-empty `sandbox.seccomp`, and `sandbox.uid_drop = true`.
- Tool-level `effects` are added to package-level capability effects and feed risk scoring.

## Capability Review

Run the review command before installing a local plugin:

```bash
cortex plugin review ./cortex-plugin-example
```

Command form: `cortex plugin review <dir>`.

The review shows:

- requested file, network, process, secret, and background capabilities;
- package signature state;
- conformance state;
- sandbox profile;
- recommended `[risk.tools.<name>]` policy lines;
- governance warnings such as missing SBOM, missing signature, or missing conformance certificate.

Directory installs print the same review before copying files.

After enabling plugins for an instance, run `cortex policy lint` to check the combined config/plugin posture. It catches enabled plugin manifests that are missing, unreviewed plugins under open permission mode, native/process plugin tools without explicit risk profiles, secret access without confirmation/block policy, and unsafe background execution policy.

## Conformance Kit

Plugin authors and operators can run:

```bash
cortex plugin test ./cortex-plugin-example
```

Command form: `cortex plugin test <dir>`.

The local conformance kit checks:

- manifest shape and compatibility;
- governance constraints;
- process command and working directory path boundaries;
- command existence;
- timeout and output-limit values;
- secret-like environment inheritance;
- native ABI declaration for trusted native plugins;
- declared capability/effect visibility.

A failed conformance report is an install and release blocker for governed plugin packages.

## Process JSON Protocol

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

Set `is_error = true` when the command completed but the tool result should be treated as a failed tool call. If the process exits non-zero, Cortex surfaces stderr as the tool error when stderr is present; otherwise it reports the exit status. If stdout is not valid JSON, Cortex rejects the tool result.

## Packaging

From the plugin directory:

```bash
cortex plugin test .
cortex plugin pack .
cortex plugin install ./cortex-plugin-example-v0.1.0-linux-amd64.cpx
```

Folder installs are supported too:

```bash
cortex plugin install ./cortex-plugin-example/
```

Local installs and `.cpx` archives include only supported assets:

- `manifest.toml`
- `package.toml`
- `sbom.spdx.json`
- `risk.toml`
- `conformance.toml`
- `lib/`
- `skills/`
- `prompts/`

Hidden entries, backup directories, and unsupported extra files are ignored. If the manifest declares `[native].library` and `lib/` is missing, Cortex automatically copies the built shared library from `target/release/` or `target/debug/` into the installed plugin `lib/` directory.

## Hot Reload

Process-isolated command implementation changes apply on the next tool invocation. Manifest, schema, and tool-set changes are detected by the hot-reload watcher; Cortex unregisters the previous proxy tools and registers the new manifest-declared set.

Installing or replacing a trusted native shared library requires a daemon restart to load the new code. Trusted native shared-library code changes still require `cortex restart`, because the daemon keeps loaded shared libraries mapped for process lifetime.

## Trusted Native ABI

Trusted native plugins are shared libraries built against `cortex-sdk`. They are not sandboxed and should only be used for code the operator trusts at the daemon-process level.

```toml
name = "dev"
version = "1.5.5"
description = "Trusted native development tools"
cortex_version = "1.5.5"
trust = "trusted_native"

[capabilities]
provides = ["tools", "skills"]

[sandbox]
level = "trusted_in_process"

[native]
library = "lib/libcortex_plugin_dev.so"
isolation = "trusted_in_process"
abi_version = 1
```

Rules:

- Native plugins must export `cortex_plugin_init`.
- The runtime requires `abi_version = 1`.
- Legacy symbols such as `cortex_plugin_create` and `cortex_plugin_create_multi` are rejected.
- A native plugin crash or undefined behavior can affect the daemon.

## Skills And Prompts

Place optional skills under `skills/<skill-name>/SKILL.md` and optional prompt fragments under `prompts/`. They are packaged with the plugin and loaded with the plugin manifest.
