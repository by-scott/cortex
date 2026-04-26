# Plugins

Cortex 1.5 exposes the SDK contract and a runtime tool execution boundary. It
does not yet expose native shared-library loading or process plugin spawning.

## SDK Contract

- `PluginManifest::validate` rejects empty names, empty versions, and ABI
  mismatches.
- `PluginManifest::validate_request` rejects tool requests that require
  capabilities the plugin did not declare.
- `PluginContext::authorize` rejects missing host-granted capabilities.
- Host paths are denied by default unless the host explicitly enables them.
- `ToolResponse::validate_output` enforces output limits.

## Runtime Execution

`CortexRuntime::execute_tool` validates the manifest, validates the tool
request, builds a host-granted `PluginContext`, rejects unauthorized host paths
before creating side effects, records `SideEffectIntended`, executes the tool
through the runtime `ToolExecutor`, validates output size, and records
`SideEffectRecorded`.

Execution failures are durable records, not silent host errors. Successful
records carry an output digest. Tool side effects are private to the invoking
client unless a future boundary explicitly widens that scope.

## ABI

`cortex-sdk` declares `ABI_VERSION`. A plugin manifest must match that value
before the host treats the plugin as conforming.

## Current Boundary

The current release does not load native shared libraries or spawn process
plugins. Those loader paths must be rebuilt with conformance tests before they
are documented as operational features again.
