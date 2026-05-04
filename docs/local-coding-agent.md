# Local Coding Agent

This path gives a new user a bounded local coding workflow before they enable broad tools, plugins, or channels. It does not add sandbox containment. It uses ordinary Cortex policy, permission, journal, replay, memory, RAG, tool-effect, plugin-governance, and protected-root rules.

## Create The Fixture

```bash
cortex demo
```

The command creates:

- instance `demo` under the current Cortex home;
- config for `ollama` and `qwen2.5-coder:7b`;
- `balanced` permission posture through `risk.auto_approve_up_to = "Review"`;
- no enabled plugins;
- a `local-coding-demo` skill;
- a workspace under `~/.cortex/workspaces/demo`, outside the protected runtime root.

Use `--id NAME` for a different instance id, `--home PATH` for a different Cortex home, and `--force` to refresh demo-owned files.

## Check Readiness

```bash
ollama pull qwen2.5-coder:7b
cortex doctor --id demo
cortex doctor --id demo --json
cortex policy lint --id demo
```

`cortex doctor` is a read-only readiness and policy-posture report. The `--json` form is useful for scripts and issue reports because it includes machine-readable findings and remediation hints. It does not contact providers by default, does not start the daemon, and does not claim sandbox containment. See [Local Models](local-models.md) for Ollama and vLLM configuration details.

## Run The Demo

```bash
cortex install --id demo
cortex --id demo
```

Then ask:

```text
Use the local-coding-demo skill on ~/.cortex/workspaces/demo. Fix the formatter test and verify it with python3 -m unittest discover -s tests.
```

The generated workspace is intentionally small. The expected loop is read, plan, edit, verify, and report changed files, tests, risks, and next steps.

## Boundaries

- The workspace is project data; the instance home is protected runtime state.
- Policy and risk gates are review controls, not OS/container isolation.
- No plugin is enabled by the fixture.
- Native plugins remain trusted in-process code if the operator enables them later.
- Retrieved files, tool output, and test output are evidence, not runtime instructions.
