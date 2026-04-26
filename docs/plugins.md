# Plugins

Cortex 1.5 exposes the SDK contract, not a live plugin loader.

## SDK Contract

- `PluginManifest::validate` rejects empty names, empty versions, and ABI
  mismatches.
- `PluginManifest::validate_request` rejects tool requests that require
  capabilities the plugin did not declare.
- `PluginContext::authorize` rejects missing host-granted capabilities.
- Host paths are denied by default unless the host explicitly enables them.
- `ToolResponse::validate_output` enforces output limits.

## ABI

`cortex-sdk` declares `ABI_VERSION`. A plugin manifest must match that value
before the host treats the plugin as conforming.

## Current Boundary

The current release does not load native shared libraries or spawn process
plugins. Those execution paths must be rebuilt with conformance tests before
they are documented as operational features again.
