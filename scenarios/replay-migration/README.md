# Replay Migration Corpus

This directory contains release-review cases for replay fixture migration,
projection stability, side-effect substitution, replay diffs, and causal audit
coverage. It is an Eval/Scenario artifact, not a guarantee that every historical
journal from every previous release migrates without review.

The current corpus references the checked-in replay fixtures under
`crates/cortex-kernel/tests/fixtures/replay/` and the replay tests that project
those fixtures. It is a release-review surface for migration evidence; it is not
a full historical database archive.

## Files

- `corpus.json`: structured replay migration review cases.

## Case Schema

Each case records:

- `id`: stable case id.
- `fixture_path`: fixture or test surface under review.
- `source_release`: source release or fixture generation scope.
- `target_release`: release under review.
- `projection_surface`: projection or replay behavior being checked.
- `expected_evidence`: evidence the reviewer should see.
- `command`: command that exercises the evidence.
- `migration_risk`: risk class covered by the case.
- `limitation`: what this case still does not prove.
- `release_use`: how to record the case during release evidence review.

## Review Boundary

Attach this corpus to release evidence when making replay compatibility or
migration claims. Do not mark historical migration as passed unless historical
fixtures, journals, or database snapshots for the candidate were actually run.
