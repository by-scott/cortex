# Actor Leakage Corpus

This directory contains release-review cases for actor, session, memory, task,
goal, retrieval, channel, transport, and audit boundary leakage. It is an
Eval/Scenario artifact, not a runtime isolation layer, not sandbox containment,
and not proof of hostile multi-tenant hardening.

Use the corpus to review whether a requester can observe or mutate another
actor's state through ordinary user-visible surfaces. Passing this corpus means
the reviewed release evidence includes these leakage classes. It does not prove
complete actor isolation across every transport, plugin, or deployment mode.

## Files

- `corpus.json`: structured cases for release and regression review.

## Case Schema

Each case records:

- `id`: stable case id.
- `surface`: the surface under review.
- `source_kind`: more specific route or state category.
- `requester_actor`: actor attempting the access.
- `target_actor`: actor whose state must remain scoped.
- `asset`: protected state under review.
- `leakage_class`: type of boundary failure being tested.
- `setup`: fixture state the reviewer should create or inspect.
- `action`: attempted access or mutation.
- `expected_handling`: how Cortex should enforce actor scoping.
- `forbidden_outcome`: behavior that must not happen.
- `evidence_boundary`: invariant being reviewed.
- `release_use`: how to record the case during release evidence review.

## Review Boundary

Attach this corpus to release evidence when making claims about actor-scoped
sessions, memory, tasks, goals, retrieval, channel subscriptions, transport
bindings, or audit/operator surfaces. Missing active exploit runs must stay
visible as release limitations.
