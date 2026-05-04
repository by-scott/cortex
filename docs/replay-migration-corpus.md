# Replay Migration Corpus

Cortex replay claims depend on journaled events, projection versions, replay
diffs, side-effect substitution, and checked replay fixtures. The replay
migration corpus is a release-review fixture that records which replay surfaces
must be checked before making migration or compatibility claims.

The corpus lives at:

```text
scenarios/replay-migration/corpus.json
```

The current corpus references the checked-in replay fixtures:

```text
crates/cortex-kernel/tests/fixtures/replay/externalized_compaction_boundary.toml
crates/cortex-kernel/tests/fixtures/replay/tool_effect_transaction.toml
```

This is an Eval/Scenario artifact. It is not proof that every historical
journal or database from every previous release migrates without review. When
historical snapshots are not run, release evidence must say so.

## Review Path

For release candidates, run the normal Docker gate and behavior evidence
commands, then attach the corpus review in the release evidence template:

```bash
./scripts/gate.sh --docker --require-clean
./scripts/release-behavior-report.sh --run
./scripts/soak-fault-harness.sh --run
```

Focused replay evidence currently comes from:

```bash
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_fixture_corpus_projects_current_surfaces
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_diff_reports_projection_changes
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay replay_side_effect_substitution_prefers_provider_values
docker compose run --rm dev cargo test -p cortex-kernel --test persistence_replay journal_replay_keeps_guardrail_and_external_input_events_stable
```

The corpus itself is checked for parseability and documentation coverage by the
`replay_migration_corpus_is_parseable_and_documented` contract test.

## Required Review Notes

For each case, record:

- fixture path or replay test surface reviewed;
- source and target release scope;
- projection surface and expected evidence;
- whether replay diff and deterministic digest evidence were attached;
- limitations when only current fixtures were run.

Do not mark historical migration as passed unless historical fixtures, journals,
or database snapshots for the candidate were actually run.
