# Compatibility Policy

Cortex is still an early local runtime. This document defines which surfaces are treated as compatibility boundaries now, which ones are only best-effort, and which ones remain intentionally unstable while the runtime is still hardening.

The goal is straightforward: make operator trust and extension contracts explicit before Cortex claims a broader ecosystem.

## Compatibility Classes

### Stable Enough To Rely On

These surfaces are treated as operator-facing compatibility boundaries and should not drift casually:

- persisted event replay semantics
- `TurnState` lifecycle surface
- install-time and runtime permission modes: `strict`, `balanced`, `open`
- process-plugin manifest surface documented in `docs/plugins.md`
- trusted native ABI entrypoint name: `cortex_plugin_init`
- documented channel/operator commands in shipped CLI/docs

Changes here require:

- explicit documentation updates
- regression coverage
- release-note callout when the operator-facing behavior changes

### Versioned Contract Surface

These surfaces are expected to evolve, but only behind an explicit version or contract boundary:

- trusted native ABI (`abi_version`)
- process-plugin manifest fields and execution rules
- replay execution version stamped into events
- SDK DTO and media/tool-result surface

Changes here require:

- a version bump or explicit compatibility note
- conformance tests
- migration or rejection behavior that fails clearly

### Best-Effort Surface

These behaviors should stay coherent, but Cortex does not yet promise long-term compatibility beyond the current release line:

- internal prompt composition details
- metacognitive heuristics and thresholds
- skill utility heuristics
- status presentation formatting
- hot-reload implementation details behind documented behavior

These can change when runtime quality improves, but the externally documented behavior should remain readable and operator-safe.

## Event and Replay Compatibility

The event journal is the source of truth. Cortex treats replay semantics as a compatibility surface, not only a debugging convenience.

Current policy:

- new events may be added
- existing persisted fields should not silently change meaning
- replay must continue to understand previously written `execution_version` values that remain within the supported release line
- compaction boundaries, side-effect substitution, and replay digest semantics must stay documented and tested

If replay semantics change in a way an operator would notice, the release notes should call it out explicitly.

## Process Plugin Compatibility

Process JSON is the default external plugin boundary.

Current policy:

- documented manifest fields are treated as public contract surface
- undocumented fields are not supported
- path rules, environment inheritance rules, timeout behavior, and output limits are compatibility-relevant behavior
- invalid manifests should fail clearly rather than degrade silently

When the manifest surface changes, Cortex should prefer:

1. additive fields
2. explicit rejection of unsupported old/new forms
3. release notes and docs updates in the same change

## Trusted Native ABI Compatibility

Trusted native ABI is a versioned extension boundary, not a sandbox.

Current policy:

- the runtime only loads `cortex_plugin_init`
- ABI compatibility is controlled through `abi_version`
- old trait-object loading symbols are not part of the supported surface
- failure reporting for invalid descriptors, invalid input, and ABI mismatch should stay deterministic

Compatibility here means "clear versioned failure or successful load", not "all old binaries keep loading forever".

## Documentation Compatibility

Published docs are part of the operator contract. Cortex should not present docs as normative while letting them drift from the shipped runtime.

Current checks already cover:

- README event count
- turn-state surface
- permission-mode guidance
- plugin boundary wording
- replay/compaction wording
- key bilingual README/runtime surface statements

The direction is to grow that into generated or contract-tested operator docs, not to rely on manual memory.

## Release Expectations

Any release that changes a compatibility boundary should include:

- what changed
- whether the change is additive, breaking, or rejection-only
- whether restart, reinstall, or plugin rebuild is required
- whether persisted data or replay behavior is affected

If a change does not meet that bar yet, it should stay in the "early/runtime hardening" bucket rather than being presented as stable platform behavior.
