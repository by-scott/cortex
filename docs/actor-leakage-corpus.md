# Actor Leakage Corpus

Cortex treats actor ownership as a runtime boundary for sessions, memory, tasks,
goals, retrieval evidence, channels, transports, and operator surfaces. The
actor leakage corpus is a small release-review fixture for checking that a
requester cannot observe or mutate another actor's state through normal
interfaces.

The corpus lives at:

```text
scenarios/actor-leakage/corpus.json
```

This is an Eval/Scenario artifact. It is not sandbox containment, not hostile
multi-tenant hardening, and not proof of complete actor isolation across every
transport, plugin, or deployment mode.

## Review Path

For release candidates, run the normal Docker gate and behavior evidence
commands, then attach the corpus review in the release evidence template:

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

Focused actor-boundary evidence currently comes from the runtime and memory
contract suites:

```bash
docker compose run --rm dev cargo test -p cortex-runtime daemon_sessions --all-features
docker compose run --rm dev cargo test -p cortex-runtime rpc_sessions --all-features
docker compose run --rm dev cargo test -p cortex-runtime rpc_memory --all-features
docker compose run --rm dev cargo test -p cortex-turn --test memory_tools --all-features
```

The corpus itself is checked for parseability and documentation coverage by the
`actor_leakage_corpus_is_parseable_and_documented` contract test.

## Required Review Notes

For each case, record:

- which actor attempted access and which actor owned the target state;
- whether list/get/search/update/cancel routes filtered or rejected hidden ids;
- whether transport rebinding preserved historical ownership;
- whether non-local actors were blocked from local-operator surfaces;
- limitations when a case was reviewed by static evidence only.

Do not mark the corpus as passed unless the release reviewer checked the cases
against the candidate. Missing active exploit runs must stay visible as
limitations.
