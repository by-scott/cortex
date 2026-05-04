# Prompt-Injection Corpus

Cortex keeps external text as evidence, not runtime instruction. The
prompt-injection corpus is a small release-review fixture for checking that
boundary across web, file, retrieval, plugin, channel, and tool-output surfaces.

The corpus lives at:

```text
scenarios/prompt-injection/corpus.json
```

This is an Eval/Scenario artifact. It is not a complete prompt-injection
defense, not sandbox containment, and not proof that every hostile document is
blocked. It records the cases a release reviewer should inspect and extend.
It is not a complete prompt-injection defense.

## Review Path

For release candidates, run the normal Docker gate and behavior evidence
commands, then attach the corpus review in the release evidence template:

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

The existing safety tests cover runtime guardrail behavior:

```bash
docker compose run --rm dev cargo test -p cortex-turn --test safety_contracts --all-features
```

The corpus itself is checked for parseability and documentation coverage by the
`prompt_injection_corpus_is_parseable_and_documented` contract test.

## Required Review Notes

For each case, record:

- whether the hostile payload stayed in the evidence/tool-result plane;
- whether actor ownership, protected roots, plugin trust, and permission gates
  remained authoritative;
- whether any candidate-specific hostile evidence was added;
- limitations when a case was reviewed by static evidence only.

Do not mark the corpus as passed unless the release reviewer checked the cases
against the candidate. Missing active exploit runs must stay visible as
limitations.
