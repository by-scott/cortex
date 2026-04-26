# cortex-sdk

Plugin SDK for Cortex 1.5.

The 1.5 SDK is capability-first: plugins receive explicit tenant, actor,
session, resource, and policy context from the host. Unknown capabilities are
denied by default.

Current contract surface:

- `ToolRequest` declares required capabilities and host paths.
- `PluginContext::authorize` denies missing capabilities and host paths unless
  host access is explicitly enabled.
- `ToolResponse::validate_output` rejects output that exceeds the resource
  limit.
- `PluginManifest::validate` rejects empty manifests and ABI mismatches.
- `PluginManifest::validate_request` rejects requests that need capabilities
  the plugin did not declare.
- `crates/cortex-sdk/tests/plugin_contract.rs` is the conformance floor for
  SDK releases.
