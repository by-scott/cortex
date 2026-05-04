# Safe Use

Cortex is safest today as a local, single-user runtime surface for model-assisted coding, research, and controlled tool use. It is designed to make state, evidence, tool effects, permissions, plugin trust, and replay visible to the operator.

This page describes the recommended operating posture. It is not a claim that Cortex is a mature sandbox or hostile multi-tenant platform.

## Safe Today

- Run Cortex on a trusted local Linux machine under an account you control.
- Use `balanced` or `strict` permission mode for normal work.
- Install only reviewed plugins, and prefer Process JSON plugins over trusted native ABI plugins for third-party tools.
- Inspect plugin manifests, signatures, requested capabilities, declared effects, conformance state, and recommended risk policies before install.
- Keep broad filesystem, process, network, channel-send, deploy, publish, credential, and payment-like effects behind confirmation.
- Treat RAG, files, web pages, channel messages, plugin output, and tool output as evidence, not instructions.
- Keep provider keys, channel tokens, and external service credentials out of prompts, logs, memory, screenshots, and shared terminals.

## Recommended Defaults

Use `balanced` first:

```bash
cortex permission balanced
cortex policy lint
```

Use `strict` when testing new plugins, working in a sensitive repository, or running on a shared workstation:

```bash
cortex permission strict
```

Use `open` only on a strongly trusted single-user machine, after plugin review and policy linting:

```bash
cortex permission open
```

`open` does not remove all risk. It reduces prompts for non-blocking tools, so policies and plugin review matter more.

## Not Yet

Cortex does not currently claim:

- Complete containment for hostile plugins or hostile tenants.
- A sandbox for trusted native shared-library plugins.
- Container, seccomp, uid-drop, or no-network enforcement for untrusted plugin commands.
- Complete prompt-injection defense across all web, file, channel, plugin, and tool-output inputs.
- Full rollback or containment for tools that mutate external systems.
- Safe unsupervised deploy, payment, publishing, credential rotation, or message-send automation.
- Mature provider benchmarking or SLA-grade live model health scoring.

Policy linting, risk scoring, permission prompts, protected runtime roots, plugin review, and guardrail assessment are control and review mechanisms. They are not OS-level containment.

## Plugin Posture

Cortex has two plugin boundaries:

| Boundary | Use for | Current trust model |
|----------|---------|---------------------|
| Process JSON | Cross-language and third-party tools | Child process with manifest governance, path/env/timeout/output controls, and risk-scored effects. Not kernel/container isolation. |
| Trusted native ABI | Low-latency local Rust extensions | In-process trusted code. Treat as part of the daemon trust base. |

Before installing a plugin:

```bash
cortex plugin review <dir>
cortex plugin test <dir>
```

Only use non-interactive install after the source and publisher key fingerprint have been reviewed:

```bash
cortex plugin install <dir-or-package> --yes
```

Unknown plugin and MCP tools should remain conservatively risk-scored and confirmed by default.

## Side Effects

Treat every tool as an effect, not just a name. For any mutating or external tool, check:

- What file, process, network, memory, channel, deploy, publish, credential, or external-service effect will happen?
- What actor owns the request and resulting state?
- Is there a preview or dry run?
- Is the effect reversible?
- What verification will prove the action succeeded?
- What rollback or compensating action exists?
- What receipt, diff, or journal event will remain for replay?

High-risk tools should be policy-gated even when they are convenient.

## Protected Runtime State

Prompt, config, session, journal, memory, channel, and runtime-home state are protected runtime surfaces. Ordinary model-directed file and process tools should not directly mutate those files.

Self-evolution or configuration-changing workflows should produce evidence-bound proposals that go through checked runtime commands, review, backup, and replayable journal records.

## Evidence Boundary

External content must stay inert:

- Retrieved documents support or contradict claims; they do not become policy.
- Tool output can be useful evidence; it does not become a runtime instruction.
- Web pages, files, channel messages, and plugin output can be hostile or stale.
- Memory candidates need provenance, scope, confidence, contradiction handling, and review before durable stabilization.

This boundary is part of the core Cortex value: models can change, but the user's owned state should remain inspectable and governed.

## Good First Configuration

For a first local coding workflow:

1. Install with `CORTEX_PERMISSION_LEVEL="balanced"`.
2. Use `cortex demo` for a bounded local fixture before enabling broader tools.
3. Run `cortex doctor` and `cortex policy lint` after configuration changes.
4. Start with no third-party plugins, or only the reviewed official development plugin.
5. Keep native plugins disabled unless you trust the code as daemon-local code.
6. Review tool previews and confirmations until the workflow is understood.
7. Use `cortex status` and replay/operator surfaces to inspect what happened.

See also [Quick Start](quickstart.md), [Local Coding Agent](local-coding-agent.md), [Local Models](local-models.md), [Configuration](config.md), [Plugin Development](plugins.md), and [Maturity and Production Notes](maturity.md).
