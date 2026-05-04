# Prompt-Injection Corpus

This directory contains a small release-review corpus for hostile instructions
arriving through evidence surfaces. It is an Eval/Scenario artifact, not a
runtime policy, not sandbox containment, and not a complete prompt-injection
defense.

It is not a complete prompt-injection defense.
It is not a runtime policy.

Use the corpus to review whether external text remains evidence instead of
runtime instruction when it enters through web pages, files, retrieval results,
plugin output, channel messages, or tool-shaped JSON.

## Files

- `corpus.json`: structured cases for release and regression review.

## Case Schema

Each case records:

- `id`: stable case id.
- `surface`: the ingress surface under review.
- `source_kind`: more specific source category.
- `actor`: actor scope used by the fixture.
- `attack_class`: hostile behavior being attempted.
- `payload`: the hostile evidence text or tool-shaped payload.
- `expected_handling`: how Cortex should treat the input.
- `forbidden_outcome`: behavior that must not happen.
- `evidence_boundary`: the invariant being reviewed.
- `release_use`: how to record the case during release evidence review.

## Review Boundary

Passing this corpus means the reviewed release evidence includes these hostile
evidence classes. It does not prove complete prompt-injection resistance and
does not create OS/container sandbox isolation.
