# Roadmap Review

This document defines the next Cortex release line after `v1.4.0`. It is not a
date promise. It is the engineering contract for the `v1.5.0` planning target.

The rule for `v1.5.0` is deliberately narrow: do not replace the mature `v1.4.0`
runtime with a smaller rewrite. Build on the released `v1.4.0` baseline and turn
the existing cognitive approximations into stronger runtime contracts:
evidence-backed, typed, calibrated, replayable, auditable, and evaluable.

## Release Target

The current planning target is `1.5.0`. The release should upgrade mechanisms,
not merely rename concepts. A feature is in scope only when it strengthens one of
these properties:

- **Evidence**: runtime claims can point to support, contradiction, provenance,
  and outcomes.
- **Types**: ownership, effects, evidence, policy, and authority are represented
  explicitly instead of carried as prose.
- **Calibration**: confidence, retrieval support, skill utility, and model
  routing are checked against results.
- **Replay**: important behavior can be reconstructed, diffed, migrated, and
  explained from the journal.
- **Evaluation**: release quality includes behavior, safety, retrieval, memory,
  tool, and soak metrics, not only unit tests.

## Non-Negotiable Gates

`v1.5.0` must continue the strict project gate:

- `cargo fmt --all --check` has no diff.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery` has zero warnings.
- `cargo test --workspace --all-features` passes.
- Docker Compose must be used through repository entrypoints. `./scripts/gate.sh --docker` runs the `dev` service from `docker-compose.yml`, built from the repository `Dockerfile`; that repository Docker Compose environment remains the only release-authoritative gate.
- Warning suppression attributes and compiler warning-suppression flags are not introduced.
- A failed check blocks the release until the underlying code is fixed.

## Scope Matrix

The table is the release tracking surface. Every row maps to a required planning
area for `v1.5.0`; none of these areas may disappear from implementation,
documentation, or acceptance review.

| Area | Upgrade | Required work | Acceptance signal |
|------|---------|---------------|-------------------|
| Memory | From memory entries to evidence-backed beliefs | Add claim, evidence, scope, confidence, contradiction, supersession, validity window, risk-if-wrong, user confirmation, and usage outcomes. Stabilization must consider evidence quality, user confirmation, consistency, conflict, usage result, and risk. | A stabilized memory can explain why it is believed, which events support it, what scope it applies to, what conflicts exist, whether it was refuted, and whether using it improved a task. |
| Retrieval / RAG | From relevant text retrieval to evidence adjudication | Add evidence roles, answer-claim support verification, negative evidence retrieval, corpus trust policy, and a hard invariant that query artifacts such as HyDE are never evidence. | An answer can emit a support report listing supported, contradicted, and unsupported claims. |
| Workspace / Context | From prompt assembly to controlled workspace admission | Add typed roles, trust, utility, risk, volatility, binding, eviction reasons, marginal-utility admission, evidence/memory/policy lanes, and contamination barriers. | A frame can explain why each item entered or was evicted and why external or tool text cannot become instruction, identity, permission, or tool policy. |
| Control / Decision | From heuristic action choice to explainable control policy | Record candidate actions, benefit, cost, risk, confidence, reversibility, selected action, rejected alternatives, blocking uncertainty, required evidence, and fallback plan. Use risk-sensitive thresholds and the observe-retrieve-evaluate-conflict-decide-verify-commit loop. | A permission prompt explains the available actions, evidence support, risk boundary, and why confirmation is required. |
| Metacognition | From alerts to self-calibrating control | Add typed alerts for goal conflict, evidence insufficiency/conflict, tool loop, low progress, high uncertainty, instruction conflict, context overload, calibration drift, and user dissatisfaction. Map alerts to interventions and record outcomes. | Every alert has trigger, severity, recommended action, action taken, outcome, and threshold update, and alerts affect control flow. |
| Attention / Scheduler | From three channels to resource governance | Add maintenance debt, emergency debounce, actor fairness, per-actor budgets, deadlines, cost, risk, priority inheritance, and operator override. | The scheduler can explain foreground, maintenance, and emergency decisions, including deferred work and per-actor budget pressure. |
| Risk / Permission | From tool-name scoring to an effect type system | Introduce effects such as read file, write file, delete file, run process, network request, send message, spend money, deploy, modify credential, persist memory, and publish content. Tools declare effects, reversibility, confirmation conditions, dry-run support, paths, domains, and actors. Policy can target effects, not only tool names. | Operator prompts describe actual effects, affected paths/domains, reversibility, dry-run status, policy reason, approval actor, and rollback path. |
| Guardrails | From pattern blocking to adversarial input governance | Add taint propagation, structured injection-intent classes, cross-turn hostile-source memory, and safe transformations such as summary-only, quote-only, or metadata-only evidence. | Malicious web/file/plugin/channel input is downgraded to hostile evidence, cannot alter policy/identity/permissions/memory, and leaves a journaled guardrail event. |
| Plugin System | From runnable plugins to governed packages | Add capability manifests, signed packages, publisher identity, manifest/binary hash, SBOM, declared capabilities, risk profile, conformance certificate, and trust tiers: trusted native, reviewed process, unreviewed process, disabled, quarantined. | Before install, the operator sees requested capabilities, missing secret access, signature state, conformance result, and recommended risk profile. |
| Sandbox / Containment | From child-process controls to real isolation | Add sandbox profiles from trusted in-process through child process, uid/gid drop, no-network profile, bubblewrap/firejail/seccomp, container/VM, and remote worker. Add a side-effect broker for high-risk external actions. | An untrusted plugin cannot read private keys, use arbitrary network, bypass output limits, modify host config, steal provider keys, or persist a background process. |
| Replay / Journal | From event replay to causal audit | Add causal edges, dependency/invalidation edges, projection versioning, replay diff, external idempotency keys, dry-run hashes, pre/post-state hashes, rollback actions, and external receipts. | A file change or external mutation can be traced from user request through evidence, decision, permission, tool call, diff, verification, memory update, and every side effect. |
| Actor / Ownership | From scoped APIs to information-flow typing | Add scope objects, ban load-before-auth, require scoped queries before private data materialization, and record flow-sensitive audit when private data enters workspace, transport, or tool inputs. | No API can use a bare memory/session id to bypass actor checks, and replay/projection cannot materialize private content without scope proof. |
| Prompt / Executive | From prompt files to compiled operating protocol | Add prompt parsing, section checks, forbidden-claim checks, runtime/schema consistency checks, layer compilation, version hashes, prompt linter, prompt diff approval, evidence-backed self-edits, and rollback copies. Runtime schema always wins. | A prompt cannot grant capabilities, override runtime policy, fossilize temporary state, or claim nonexistent tools. |
| Skills / Repertoire | From scripts to evaluable procedures | Add skill manifests with preconditions, inputs, outputs, effects, risk, expected duration, success criteria, fallback, and observability. Record skill execution traces and quarantine skill candidates before activation. | A skill is a runtime unit with permissions, trace, tests, user feedback, and historical utility, not only a document. |
| Tool Execution | From calls to transactional actions | Mutating tools follow plan, preview, permission, execute, verify, commit, rollback, record. Add dry-run-first support, rollback handles, structured result schemas, artifacts, diffs, receipts, warnings, and verification outputs. | For any high-risk action, Cortex can say what changed, how it was verified, how to roll back, and who approved it. |
| Model / Provider Routing | From configured model to capability routing | Add a model capability registry for coding, long context, vision, tool calling, JSON reliability, latency, cost, safety, and reasoning depth. Add routing, fallback, and escalation when confidence is low, risk is high, provider fails, or schema output is invalid. | Cortex can explain why a model was chosen over alternatives and what tradeoff was accepted. |
| Evaluation | From tests passing to behavior evaluation | Add memory precision/recall, false stabilization, contradiction resolution, harmful memory usage, retrieval recall/MRR/citation accuracy/unsupported claims/poison resistance, tool success/retry/bad selection/permission/rollback metrics, long-task recovery, and safety bypass/leakage metrics. | A release report includes behavior metrics, safety corpus results, and soak outcomes, not only cargo test output. |
| Observability | From status and logs to operator timeline | Add turn timeline, workspace frame view, retrieval/memory/control/tool/guardrail views, risk ledger, memory change review, token/cost usage, actor/session map, plugin health, and provider health. | An operator can understand why an answer was produced, why a tool ran, why memory changed, and why confirmation was requested without reading raw logs. |
| Configuration / Policy | From configuration to policy-as-code | Add policy profiles, schema validation, static policy lint, policy simulation, and explanations for tools, actors, and effects. Detect dangerous combinations at startup. | Misconfiguration such as open permissions with unknown plugins, native plugin without risk profile, network evidence auto-memory, or deploy tools with background execution is reported before use. |
| Operations / Soak | From install success to long-run reliability | Add fault injection for provider timeout, invalid schema, SQLite lock/WAL corruption, network reconnect, Telegram retry, QQ duplicate callback, plugin crash, native panic, large payload externalization, replay after upgrade, disk full, and rate limits. Run 24h/72h/7d daemon soak. | Faults do not lose ownership, pending permissions, replay consistency, state recovery, or channel session binding. |
| Multimodal / Media | From attachments to media evidence governance | Add media id, hash, MIME, actor, source URI, visibility, extracted text, detected objects, generated/edited flag, license, taint, media provenance, media-derived evidence, and external-recipient safety policy. | Media-derived OCR or vision observations are cited as derived evidence with confidence and are not silently written to memory or published. |
| Delegation / Multi-agent | From sub-agent calls to controlled delegation | Add delegation contracts with task, scope, allowed and forbidden tools, budgets, allowed evidence, expected artifact, review requirement, merge verifier, and minimal authority inheritance. | Cortex can explain who was delegated, what they could see, what they could do, how their output was verified, and whether it affected memory or external state. |
| Security / Secrets | From sensitive path rules to secret dataflow control | Add ingress secret scanning, secret source/sink tracking, allowed-use rules, sink policy, redaction handles, and brokered runtime injection for tools that need secrets. | The model can know that a secret exists but cannot see the value; secrets cannot flow to providers, web requests, plugin output, channels, memory, or logs without explicit policy. |
| Data Model / Schema | From fields to versioned semantics | Add schema version, semantic version, migration, rejection behavior, compatibility tests, generated runtime specs, and a release fixture corpus for journals, memory, plugin manifests, actor mappings, retrieval evidence, and daemon state. | `cortex compat test fixtures/releases/*` proves historical data still migrates, replays, and rejects correctly. |
| Human Feedback | From feedback text to training signal | Add feedback types for correction, preference, approval, rejection, style, factual correction, safety boundary, task success, and task failure. Attribute feedback to answer style, fact, tool choice, memory, evidence, or permission judgment; gate durable feedback into memory/policy candidates; replay corrections. | A correction changes the right runtime object and future similar tasks can prove the correction was applied. |

## Priority Order

### P0: Release-Blocking Trust Work

- Memory evidence / contradiction / usage-outcome tracking.
- Guardrail taint propagation plus adversarial harness across web, file, plugin,
  and channel inputs.
- Tool effect system plus transactional side-effect execution.
- Plugin capability governance, sandbox profiles, signatures, and conformance.
- Replay causal graph plus migration corpus.
- Policy linting and simulation.

### P1: Intelligence and Explainability Work

- RAG support verifier and negative evidence.
- Workspace marginal-utility admission.
- Metacognition outcome calibration.
- Skill manifests and execution traces.
- Model capability routing.
- Operator dashboard and turn timeline.

### P2: Expansion Work, Not Release Claims

These areas remain tracked but must not outrank P0/P1 or become marketing claims
before the core boundaries are stronger:

- Complex multi-agent protocols beyond controlled delegation contracts.
- Formal cognitive-architecture claims beyond implemented runtime contracts.
- Large third-party plugin ecosystem before conformance and sandboxing mature.
- Mature hostile multi-tenant platform claims.
- Fully automatic self-evolution without review, verification, and rollback.

## Ten Design Rules For `v1.5.0`

1. Memory must have evidence, scope, conflict handling, and usage outcomes.
2. Retrieved material is evidence, never instruction.
3. Context is typed workspace admission, not prompt concatenation.
4. Control decisions record alternatives, risk, confidence, and reason.
5. Metacognition must change control flow or it is only logging.
6. Tools declare effects, not only names.
7. High-risk actions require preview, confirmation, verification, and rollback.
8. Plugins require capabilities, sandbox posture, signatures, and conformance.
9. Replay must become causal audit, not only event playback.
10. Tests must expand into behavior evaluation and long-run fault injection.

## Exit Criteria

`v1.5.0` should not ship until the P0 work is implemented, documented, and
covered by tests, and every matrix area has one of these explicit statuses:
implemented, partially implemented with a listed limitation, or intentionally
deferred as a non-release claim. Silent omission is a release blocker.
