# Changelog

## Unreleased

### Security and Trust Boundaries

- Unknown plugin and MCP tools now default to conservative risk scoring and require confirmation unless an explicit `[risk.tools.<name>]` policy lowers the risk.
- Added configurable per-tool risk policy overrides for `tool_risk`, `file_sensitivity`, `blast_radius`, `irreversibility`, `require_confirmation`, `block`, and `allow_background`.
- Added `risk.allow` and `risk.deny` tool-name patterns. Deny patterns and allowlist misses block matching tools before execution.
- `Review` risk decisions now require confirmation instead of being approved automatically by the default and interactive gates.
- Added `[risk].auto_approve_up_to` and `[risk].confirmation_timeout_secs`; daemon and channel turns now use the configured permission gate instead of unconditional auto-approval.
- Channel users can resolve pending tool confirmations with `/approve <id>` or `/deny <id>`.
- Pending tool confirmations are emitted through both the session broadcast bus and the active turn stream so synchronous channel replies and streaming transports can render the same prompt.
- Background tool execution now requires either tool-declared `background_safe` capability or explicit `[risk.tools.<name>].allow_background = true`.
- Guardrail findings are now structured by category: prompt injection, system-prompt leakage, role override, and exfiltration.
- Guardrail hits now emit a structured `GuardrailTriggered` journal event in addition to the emergency attention event.
- Added `SourceTrust`/`SourceProvenance` types and `ExternalInputObserved` journal events; successful tool outputs are now wrapped as untrusted evidence before entering LLM history.
- Process-isolated plugin tools can now be declared in manifest `[[native.tools]]` and executed as child processes through a JSON stdin/stdout protocol instead of `dlopen`.
- In-process native plugin manifests can declare `[native].sdk_version` and `abi_revision`; incompatible SDK major/minor versions or ABI revisions are rejected before loading.
- Long-term memories now carry `owner_actor`; memory save/search tools scope saved and recalled memories by runtime actor while preserving `local:default` as the local administrator.
- Plugin directory changes are now detected by the hot-reload watcher. Process-isolated manifest/tool-set changes hot-replace proxy tools, and command implementation changes take effect on the next invocation; in-process library changes remain restart-required.
- Process-isolated plugin execution now uses controlled working directories, environment allowlists/overrides, real child-process timeouts, and output-size limits.
- Memory store APIs now enforce actor-scoped list/load/delete operations for non-admin actors instead of relying only on caller-side filtering.
- Added adversarial guardrail corpus tests for web/file/plugin/channel-style external prompt injection and output leakage cases.
- Process-isolated plugin manifests now reject command/working-directory host-path escapes by default, support explicit `allow_host_paths`, and can apply Unix CPU/memory rlimits.
- Suspicious prompt-injection text inside mutating tool inputs now upgrades the tool call to `RequireConfirmation`; suspicious tool outputs are journaled as guardrail hits.
- Session, task, and audit stores now expose actor-scoped list/load/history/delete/claim/query APIs for non-admin callers; embedding vectors inherit ownership through memory ids.
- Added `cortex --new-process-plugin <name>` to scaffold process-isolated plugins on the recommended JSON protocol boundary while preserving `--new-plugin` for trusted in-process Rust plugins.

### Replay

- Fixed deterministic replay side-effect substitution so provider-supplied values are projected instead of the originally recorded value.
- Added journal-backed replay coverage for recorded side effects loaded from SQLite and substituted through a provider.
- External I/O side-effect keys now include turn id and tool-call id instead of only tool name, avoiding collisions between repeated calls.
- Added `replay_determinism_digest` to compare equivalent replay projections after side-effect substitution while excluding event ids and timestamps.
- Added a runnable soak/fault integration harness for journal reopen replay determinism, process-plugin failure containment, process-plugin reload recovery, and actor-scoped memory persistence.

### Documentation

- Added maturity and production notes in English and Chinese.
- Clarified that Cortex uses cognitive-science-inspired engineering approximations, not formal cognitive-science implementations.
- Added explicit "not yet" and threat-model notes for production hardening.
