# Roadmap Review

This document defines the next Cortex release line after `v1.4.0`. It is not a
date promise. It is the engineering contract for the `v1.5.9` planning target.

The rule for `v1.5.9` is deliberately narrow: do not replace the mature `v1.4.0`
baseline with a smaller rewrite. Cortex is now positioned as a language-model
harness: a controlled surface for driving, observing, replaying, evaluating, and
hardening model behavior. The next release should turn the existing cognitive
approximations into stronger harness contracts: evidence-backed, typed,
calibrated, replayable, auditable, and evaluable.

## Release Target

The current planning target is `1.5.9`. It is a patch target on the same 1.5
release line, not a parallel roadmap and not a reason to weaken the release
contract established after `v1.4.0`. The release should upgrade mechanisms, not
merely rename concepts. A feature is in scope only when it strengthens one of
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

## Harness Contract

Cortex should use the term `harness` in the same engineering sense used by
serious test, eval, and runtime-control systems: the harness surrounds a system
under test with controlled inputs, adapters, instrumentation, oracles,
evaluation, replay, and reporting. It is not the actor. It is the mechanism that
makes model behavior operable and measurable.

The product surface should therefore develop around these objects:

- **Scenario**: the task, actor, policy, data, tools, channels, and success
  criteria being exercised.
- **Fixture**: stable state used to run a scenario, including journals, memory,
  retrieval corpora, plugin manifests, policy profiles, and channel bindings.
- **Driver**: the component that feeds turns, tool results, channel events,
  faults, and operator decisions into the runtime.
- **Adapter**: the boundary layer for providers, tools, plugins, transports,
  corpora, and external systems.
- **Oracle**: explicit expectations for correctness, safety, ownership,
  citations, side effects, permissions, and recovery.
- **Evaluator**: metric code that scores outputs, traces, tool choices, memory
  changes, retrieval support, and safety behavior.
- **Trace**: a typed, queryable record of the actual run, not only logs.
- **Replay**: deterministic or differential reconstruction of a run from
  journaled inputs, fixtures, external receipts, and projection versions.
- **Report**: a release or scenario result that explains pass/fail status,
  regressions, risks, and the evidence behind each conclusion.

This contract is the direction for future work. New features should explain
which harness object they strengthen. Features that only make Cortex appear more
autonomous, without improving control, measurement, replay, or hardening, are
out of scope for the `v1.5.9` release claim.

## Source Basis

This plan is not a loose feature wishlist. It is the release contract distilled
from the project research corpus and the primary works behind it. The research
notes themselves stay outside the public repository; the public contract records
the engineering obligations, not private study notes.

| Source family | Primary works and engineering references | Planning impact |
|---------------|------------------------------------------|-----------------|
| Global workspace and cognitive cycles | Baars' Global Workspace Theory, Franklin's LIDA architecture, Dehaene and Naccache's global neuronal workspace, and CoALA's language-agent architecture framing | Foreground attention, limited workspace admission, broadcast through the journal, explicit turn stages, and separation between internal actions and external side effects. |
| Working memory and cognitive load | Baddeley working memory, Cowan focus of attention, Miller chunking, and Sweller cognitive load theory | Typed workspace lanes, limited focus, chunked context, marginal-utility admission, pressure-aware compaction, and eviction explanations. |
| Memory consolidation and reconsolidation | McClelland/McNaughton/O'Reilly complementary learning systems, Kumaran learning-systems review, Nader reconsolidation, and sleep/memory consolidation literature | Captured -> Materialized -> Stabilized memory, evidence-backed beliefs, source trust, contradiction handling, validity windows, and usage-outcome tracking. |
| Metacognition and conflict monitoring | Flavell metacognition, Nelson/Narens monitoring-control, Botvinick conflict monitoring, Shenhav expected value of control, frame-anchoring and calibration research | Typed alerts, alert-to-intervention mapping, confidence/outcome calibration, goal and instruction conflict detection, and control-flow-changing metacognition. |
| Decision under uncertainty | Ratcliff diffusion decision model, Gold/Shadlen decision neuroscience, Bogacz speed-accuracy tradeoff, Fleming confidence research, and precision-weighting critiques | Evidence accumulation, risk-sensitive thresholds, confidence traces, reversible vs irreversible action policy, and escalation when confidence is low or stakes are high. |
| Event sourcing and durable execution | Fowler event sourcing, CQRS/event-sourced architecture literature, Temporal-style durable execution, Durable Functions/Step Functions patterns | Append-only journal, command/event separation, intent-before-execution, side-effect recording, projection versioning, replay diff, idempotency keys, and recovery from recorded facts. |
| SQLite and daemon operations | SQLite WAL documentation, online backup/checkpoint guidance, single-writer discipline, and daemon operations practice | WAL posture, DbWriter single-writer model, checkpoint observability, online backups, corruption/fault tests, and deterministic recovery. |
| Security, policy, and plugin governance | prompt-injection and tool-use security research, process isolation practice, capability manifests, signed package patterns, SBOM/conformance practice, and approval-system design | Taint propagation, hostile-source tracking, effect policies, sandbox levels, side-effect broker, plugin signatures, conformance kits, and deny-by-default ownership. |
| Skills and capability systems | Function calling schemas, MCP capability negotiation, Kubernetes/VSCode/Emacs extension discovery, ACT-R/Fitts-Posner skill learning, and modern coding-assistant skill patterns | Skill manifests, progressive discovery, trigger provenance, execution traces, quarantine before activation, utility scoring, and schema-as-contract behavior. |
| Prior operational failures | The prior Cortex postmortem, continuity failure analysis, and long-running session failure observations | No natural-language IPC as authority, no session-as-truth, journal-derived resume packets, explicit phase/frontier state, frame checks, rollback lifecycle events, and soak/fault harnesses. |
| Cognition and wisdom formation | Friston's predictive-processing/free-energy framing, Damasio-style value and affect constraints, Baltes/Staudinger wisdom research, Sternberg's balance theory of wisdom, and Grossmann-style wise reasoning research | Cortex must not claim biological wisdom. The harness should instead create the engineering conditions for better judgment: grounded observation, value/policy weighting, long-horizon outcome feedback, calibrated uncertainty, metacognitive humility, social/operator correction, and memory consolidation. |

Any `v1.5.9` design or implementation that conflicts with these sources must
document the reason, the risk, and the test that proves the deviation is safer
for Cortex.

## Cognition Boundary

The project research treats cognition as an interacting loop rather than a
single module: perception predicts the world, attention selects a limited
workspace, working memory holds task state, memory consolidation turns episodes
into durable structure, valuation ranks possible actions, and metacognition
adjusts control when uncertainty, conflict, or failure appears. Wisdom is the
long-horizon integration of those mechanisms with value judgment, social
feedback, self-restraint, and correction under uncertainty.

For `v1.5.9`, this is a boundary condition, not a marketing claim. Cortex should
not say it implements biological cognition or wisdom. It should implement the
runtime contracts that make wisdom-like behavior auditable: evidence-backed
beliefs, policy/value constraints, closed-loop feedback, calibrated confidence,
operator correction, replayable decisions, and durable memory revision.

## Review Coverage Contract

The review that defines `v1.5.9` has twenty-five required areas. The scope
matrix below is the authoritative coverage surface for all of them:

1. Memory.
2. Retrieval / RAG.
3. Workspace / Context.
4. Control / Decision.
5. Metacognition.
6. Attention / Scheduler.
7. Risk / Permission.
8. Guardrails.
9. Plugin System.
10. Sandbox / Containment.
11. Replay / Journal.
12. Actor / Ownership.
13. Prompt / Executive.
14. Skills / Repertoire.
15. Tool Execution.
16. Model / Provider Routing.
17. Evaluation.
18. Observability.
19. Configuration / Policy.
20. Operations / Soak.
21. Multimodal / Media.
22. Delegation / Multi-worker, covering the review's multi-agent requirement
    under the new worker/harness product vocabulary.
23. Security / Secrets.
24. Data Model / Schema.
25. Human Feedback.

No row may be silently removed, renamed away from its intent, or treated as
marketing copy. At release review, each row must have implementation evidence,
tests, docs, and a known-limitations statement.

## Non-Negotiable Gates

`v1.5.9` must continue the strict project gate:

- `cargo fmt --all --check` has no diff.
- `cargo clippy --workspace --all-targets --all-features -- -D warnings -W clippy::pedantic -W clippy::nursery` has zero warnings.
- `cargo test --workspace --all-features` passes.
- Docker Compose must be used through repository entrypoints. `./scripts/gate.sh --docker` runs the `dev` service from `docker-compose.yml`, built from the repository `Dockerfile`; that repository Docker Compose environment remains the only release-authoritative gate.
- Warning suppression attributes and compiler warning-suppression flags are not introduced.
- A failed check blocks the release until the underlying code is fixed.

## Scope Matrix

The table is the release tracking surface. Every row maps to a required planning
area for `v1.5.9`; none of these areas may disappear from implementation,
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
| Delegation / Multi-worker | From delegated worker calls to controlled delegation | Add delegation contracts with task, scope, allowed and forbidden tools, budgets, allowed evidence, expected artifact, review requirement, merge verifier, and minimal authority inheritance. | Cortex can explain what was delegated, what the worker could see, what it could do, how its output was verified, and whether it affected memory or external state. |
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

Current implementation checkpoints for this release line:

These checkpoints are not release claims until the implementation, tests, docs,
and known limitations are reviewed together. `v1.5.9` must verify each point
against code-level evidence rather than treating earlier `1.5.x` notes as
accepted truth.

- Memory entries carry evidence, claim/scope fields, contradiction and supersession links, validity windows, user confirmation, risk-if-wrong, and usage outcomes.
- Guardrails now propagate taint across web, file, plugin, channel, and tool-shaped inputs with safe transformations and hostile-source memory handling.
- Tools declare typed effect surfaces and mutating execution records preview, verification, and commit events for transactional audit.
- Plugin manifests carry trust tiers, sandbox profiles, package metadata, conformance state, and capability-derived effects; install/review/test paths expose those governance fields.
- Replay exposes projection versions, causal audit graph edges, replay diffs, deterministic side-effect substitution, and a migration fixture corpus for legacy replay shapes.
- Policy-as-code exposes `cortex policy lint`, `cortex policy simulate`, and daemon startup findings for dangerous config/plugin/tool combinations.
- RAG evidence now carries explicit roles, and answer claims can be verified into supported, contradicted, unsupported, or insufficient support reports; negative evidence overrides stale support.
- Workspace frames now expose lane, utility, risk, volatility, taint, budget-aware marginal utility, admission outcomes, contamination barriers, and eviction records.
- Metacognitive adaptive thresholds now record rich alert feedback: outcome, intervention, confidence delta, intervention success rate, precision, and threshold snapshots.
- Skills now expose manifests with preconditions, inputs, outputs, effects, required tools, risk, expected duration, success criteria, fallback, and observability; executions record bounded traces.
- Model routing now uses a capability registry derived from `[llm_groups.*]` and provider metadata, covering coding, long context, vision, tool calling, JSON reliability, latency, cost, safety, and reasoning depth. Route decisions explain selected group, fallback reasons, rejected failed targets, schema-invalid fallback, and low-confidence/high-risk escalation.
- Operator dashboard now exposes local-operator state, metrics, session and binding summaries, backlog, provider model profiles, and a bounded journal timeline normalized by runtime category.

### P1: Intelligence and Explainability Work

P1 work remains in scope for `v1.5.9` when it is backed by code-level
acceptance tests and does not weaken the release gate. Existing claims must be
revalidated, especially RAG support verification, workspace admission,
metacognitive calibration, skill traces, model routing, and operator
observability.

### P2: Expansion Work, Not Release Claims

These areas remain tracked but must not outrank P0/P1 or become marketing claims
before the core boundaries are stronger:

- Complex multi-worker orchestration protocols beyond controlled delegation contracts.
- Formal cognitive-architecture claims beyond implemented runtime contracts.
- Large third-party plugin ecosystem before conformance and sandboxing mature.
- Mature hostile multi-tenant platform claims.
- Fully automatic self-evolution without review, verification, and rollback.

## Execution Order

`v1.5.9` should be implemented in this order. The order follows the research
basis: cognition depends on grounded observation, limited workspace admission,
memory consolidation, value-weighted action, feedback, and metacognitive
control. In engineering terms, the harness must first know what it believes and
why, then constrain what it can do, then prove what happened.

1. **Release audit and truth table**: build a row-by-row review table for all
   twenty-five areas. Each row records current code evidence, missing runtime
   contracts, tests, docs, and known limitations. If a claim has no test, it is
   treated as unproven.
2. **Evidence and cognition core**: finish Memory, Retrieval / RAG, Workspace /
   Context, Control / Decision, Metacognition, Human Feedback, and Model /
   Provider Routing as one loop. This is the harness equivalent of perception,
   working memory, consolidation, confidence, and correction.
3. **Action and containment core**: finish Risk / Permission, Tool Execution,
   Guardrails, Security / Secrets, Plugin System, Sandbox / Containment, and
   Delegation / Multi-worker. No external action should bypass effect typing,
   preview, confirmation, verification, rollback, taint, or scope.
4. **Durability and authority core**: finish Replay / Journal, Actor /
   Ownership, Data Model / Schema, Prompt / Executive, Configuration / Policy,
   and Skills / Repertoire. The journal remains the source of truth; prompts and
   skills cannot grant authority; migrations and projections must be testable.
5. **Operations and evaluation core**: finish Attention / Scheduler,
   Evaluation, Observability, Operations / Soak, and Multimodal / Media. Release
   confidence must come from behavior metrics, safety corpora, replay fixtures,
   and daemon fault tests, not only from passing unit tests.
6. **Release cut**: update versions, generated docs, README surfaces,
   changelog, release notes, package metadata, and binary packaging only after
   the full Docker Compose gate passes with zero warnings, zero errors, no
   suppressions, and a clean release review table.

No step may hide unfinished work behind new terminology. If implementation
deviates from the research basis or review opinion, the deviation must be
explicitly recorded with its risk and the test that makes the deviation safer.

## Ten Design Rules For `v1.5.9`

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

`v1.5.9` should not ship until the P0 work is implemented, documented, and
covered by tests, and every matrix area has one of these explicit statuses:
implemented, partially implemented with a listed limitation, or intentionally
deferred as a non-release claim. Silent omission is a release blocker.

The working release audit is tracked in
[`release-audit-1.5.9.md`](release-audit-1.5.9.md). That audit table is the
handoff surface between planning and implementation; release notes must not
claim completion for a row that remains partial or blocked there.
