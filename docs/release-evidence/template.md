# Release Evidence Template

Use this template for a release-candidate review attachment. It is a checklist
for evidence, not a release note. Do not mark an item as passed unless the
listed command was run for the candidate and the output was reviewed.

## Candidate

```text
Release target:
Git revision:
Generated at:
Reviewed by:
Docker gate image:
```

## Required Attachments

| Evidence | Status | Command or file | Notes |
|----------|--------|-----------------|-------|
| Strict Docker gate | not run | `./scripts/gate.sh --docker --require-clean` | Required before release-ready claims. |
| Release behavior report | not run | `./scripts/release-behavior-report.sh --run` | Covers memory, retrieval/RAG, tools, safety, operator timeline, recovery, replay, and soak posture. |
| Bounded soak/fault report | not run | `./scripts/soak-fault-harness.sh --run` | Covers bounded provider, channel, SQLite, plugin crash, disk/config, rate-limit/backpressure, replay determinism, and reconnect evidence. |
| Long daemon soak report | not run | `./scripts/daemon-soak.sh --run --duration 24h --interval 60s` | Required before claiming 24h daemon soak evidence; shorter runs are smoke only. |
| Release audit table | not checked | `docs/release-audit-<version>.md` | Every partial row needs an explicit limitation or acceptance evidence. |
| First-run readiness output | not run | `cortex doctor --json` | Read-only diagnostics; this is not sandbox or provider reachability proof. |
| Policy posture | not run | `cortex policy lint` | Record warnings/errors and remediation decisions. |
| Plugin conformance attachment | not run | `docs/plugin-conformance-template.md` | Complete one copy for each plugin relied on by the candidate. |
| Prompt-injection corpus review | not run | `scenarios/prompt-injection/corpus.json` | Review hostile evidence cases and attach candidate-specific additions or limitations. |
| Actor leakage corpus review | not run | `scenarios/actor-leakage/corpus.json` | Review cross-actor state access cases and attach candidate-specific additions or limitations. |
| Replay migration corpus review | not run | `scenarios/replay-migration/corpus.json` | Review replay fixtures, projection diffs, side-effect substitution, and historical limitations. |

Status values:

```text
pass
fail
not run
not applicable
blocked
```

## Safety Boundary Review

Confirm these boundaries remain true for the release claim:

- Policy/risk gates are described as review/control mechanisms, not sandbox containment.
- Native plugins are described as trusted in-process code, not sandboxed code.
- Retrieved evidence, files, web content, channel messages, plugin output, and tool output stay evidence, not runtime instructions.
- Ordinary tools do not directly mutate prompts, config, sessions, journal, memory, channel state, or protected runtime roots.
- Demo and first-run paths do not broaden permissions, enable plugins, or start external side effects without operator action.
- Missing 24h/72h/7d soak evidence is called out as a limitation when it has not run for the candidate.
- Prompt-injection corpus review is evidence coverage, not a complete defense claim.
- Actor leakage corpus review is evidence coverage, not hostile multi-tenant hardening or sandbox containment.
- Replay migration corpus review is evidence coverage, not proof that every historical journal/database migrates.

## Behavior Evidence Summary

| Area | Evidence source | Result | Limitation |
|------|-----------------|--------|------------|
| Memory ownership and memory tools |  |  |  |
| Retrieval/RAG and support verification |  |  |  |
| Tool risk and permission behavior |  |  |  |
| Prompt injection and hostile evidence corpus |  |  |  |
| Actor/session isolation |  |  |  |
| Replay and journal recovery |  |  |  |
| Plugin governance and conformance |  |  |  |
| Operator status/timeline observability |  |  |  |
| Bounded soak/fault harness |  |  |  |
| Long soak evidence |  |  |  |

## Known Limitations

List limitations that must remain public and must not be promoted into release
claims:

```text
- 
```

## Decision

```text
Release decision: pass / fail / hold
Reason:
Follow-up issues:
```
