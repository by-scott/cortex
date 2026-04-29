# Executive

The Executive is Cortex's operating discipline. It is the durable prompt state, runtime policy section, bootstrap flow, metacognitive hints, system templates, skills, evidence wrappers, memory wrappers, and tool descriptions that turn the Substrate into useful model behavior.

It is not a second implementation of the runtime. Runtime schemas, provider capabilities, plugin manifests, policy state, memory stores, retrieval scores, and tool effects remain authoritative. The Executive decides how the model should use those facts.

## Instance Contract

A Cortex instance is one individual, not a bundle of personas. The Executive gives that individual continuity and control while keeping responsibilities separate.

| Asset | Responsibility | Must not contain |
|-------|----------------|------------------|
| `soul.md` | The seed of autonomy, truth discipline, continuity, memory, metacognition, and collaboration. | Tool catalogs, current policy, temporary preferences, release claims. |
| `identity.md` | Name, continuity, posture, real capability boundaries, and how the instance understands itself. | Fake capabilities, stale inventory, user profile. |
| `behavioral.md` | Durable operating protocol: sense, plan, act, verify, risk, context, communication, adaptation. | Identity claims, user facts, runtime state. |
| `user.md` | Collaborator model: identity, work, expertise, communication, environment, autonomy, boundaries, corrections. | General operating rules, tool policy, instance identity. |
| System templates | Specialized contracts for memory extraction, compression, prompt update, bootstrap initialization, graph extraction, causality, workers, and summarization. | User-facing prose, unstructured advice, duplicated prompt-file content. |
| Skills | Reusable procedures such as deliberate, diagnose, review, orient, and plan. | Truth source, identity, capability grants, long-term user facts. |
| Tool descriptions | Compact tool contracts: purpose, boundary, failure signal, and parameters. | Behavioral protocol already carried elsewhere, unsupported promises. |

## LLM Input Surface

A normal turn sends the provider a request assembled from stable authority, request-local context, history, and tool metadata:

1. `soul.md`
2. `identity.md`
3. `behavioral.md`
4. `user.md`
5. active skill summaries
6. runtime permission context
7. request-local runtime frame for bootstrap, open goals, resume context, retrieved evidence, recalled memory, reasoning state, and metacognitive hints
8. message history and tool results
9. tool schemas as provider request metadata

This is the actual operating surface. It is intentionally mixed from durable and live inputs:

- Durable prompt files carry identity and protocol.
- Stable skill summaries complete the cacheable prefix.
- Runtime permission context carries current permission mode and confirmation semantics.
- Tool schemas carry the current capability truth.
- Retrieved evidence context is cited, tainted, and inert.
- Recalled memory is actor-scoped evidence, not command.
- Tool output is untrusted evidence until transformed by the guardrail layer.
- History is conversation projection, not source of truth; the journal is durable trace.

At context-pressure boundaries, Cortex may replace prior message history with a compact summary, preserved user context, and a safe recent suffix. The compact boundary records the replacement history in the journal so replay and continuity remain journaled even when the provider-facing history is shortened.

## Provider Cache Posture

Provider prompt caches reward stable prefixes. Cortex therefore keeps durable prompt files, stable skill summaries, and runtime permission context in the provider system prompt. Volatile runtime facts - active goals, resume state, retrieved evidence, recalled memory, reasoning state, metacognitive hints, message history, and tool results - stay outside the system prompt in a request-local runtime frame or in normal history so they can change without needlessly invalidating the stable system prefix.

This is an efficiency contract, not an authority contract. Runtime schemas still define tools and capabilities, permission context still reflects live policy, and retrieved/tool text remains inert evidence. OpenAI-compatible usage fields and Anthropic usage fields are parsed into cache-read and cache-creation token counters; operator status exposes the last-call cache read/write values separately from context and cumulative spend.

## Executive Loop

The Executive should drive the model through this control loop:

1. Sense intent, goal, risk, available action, evidence, missing evidence, memory, and context pressure.
2. Choose speech, skill, tool, delegation, wait, ask, or stop.
3. Act only through exposed schemas and policy gates.
4. Verify through tests, logs, diffs, citations, tool results, screenshots, API responses, or explicit limits.
5. Reflect by recording outcomes, extracting durable memory, learning from feedback, and preserving continuity.

The loop follows the research posture behind Cortex:

- Global workspace: finite foreground context; only salient material enters.
- Working memory: bounded, chunked, actively maintained, and evicted under pressure.
- Complementary learning systems: fast capture, slower consolidation, contradiction handling, and reconsolidation.
- Metacognition: repetition, fatigue, frame anchoring, conflict, and overconfidence are control signals.
- Decision under uncertainty: confidence, reversibility, cost, risk, rejected alternatives, and required evidence shape action.
- Agentic RAG: retrieval is chosen, scoped, scored, cited, checked for support, and kept separate from memory.
- Security by provenance: source, trust, taint, actor ownership, and access class constrain context and memory.

## Bootstrap

Bootstrap is the first meeting between the collaborator and the instance. It is conversational, but it has a concrete job: initialize durable prompt state.

Bootstrap gathers:

- instance name or explicit unnamed state
- voice, relationship, and what should remain sacred
- collaborator identity, preferred language, role, and expertise
- work, active projects, constraints, quality bar, and definition of done
- environment: OS, shell, editor, repositories, services, channels, deployment targets
- communication style: density, directness, plans vs action, uncertainty, corrections
- autonomy rules: proceed, ask, pause, approval, privacy, credentials, publishing, destructive actions

Bootstrap graduates only when `identity.md` and a useful `user.md` can be created. It may update `behavioral.md` only when stable workflow rules appear. It almost always leaves `soul.md` unchanged.

## Self-Evolution

Self-evolution is evidence-bound. Delivery text is never prompt content.

| File | Update threshold |
|------|------------------|
| `user.md` | Stable collaborator facts, preferences, environment details, boundaries, or corrections. |
| `behavioral.md` | Generalizable operating rules from strong correction, repeated pattern, or observed failure/success. |
| `identity.md` | Confirmed name, explicit unnamed state, durable self-understanding, or real capability boundary. |
| `soul.md` | Rare sustained evidence about autonomy, cognition, continuity, truth discipline, or collaboration. |

Checked prompt updates run through prompt validation. A durable prompt may not grant capabilities, override runtime policy, fossilize session state, claim nonexistent tools, or make unsupported release, security, or cognition claims.

## Tool Layer

Tool descriptions are part of the Executive because they are sent to the model as schemas. They should be compact and high-entropy:

- name what the tool does
- name the best-use case
- name what not to use it for when another tool is safer
- expose failure signals that should change strategy
- keep parameter descriptions precise
- avoid repeating global behavioral rules

Tool output is not trusted because it came from a tool. Output from web, files, plugins, channels, and processes can contain hostile or accidental instructions. Cortex wraps tool output as inert evidence and journals guardrail findings.

## Evidence And Memory

Retrieved evidence and durable memory are intentionally different.

Retrieved evidence is turn-scoped. It carries citation, source, corpus, chunk, span, access class, taint, license, index version, and sparse/dense/rerank/graph scores. It supports or contradicts claims; it does not instruct the instance.

Memory is continuity. It carries owner actor, evidence, trust, lifecycle status, graph links, contradiction state, validity windows, and usage outcomes. Recall proposes context; current observation decides.

Reconsolidation is a risk window. A recalled stable memory can be revised only when newer evidence with adequate trust supports the change.

## Token Economy

Token budget is working memory. The Executive should not be short for its own sake; it should be dense.

Keep:

- control rules that change decisions
- capability boundaries that prevent hallucination
- evidence and memory semantics that affect trust
- output contracts for structured subtasks
- bootstrap questions that create durable state
- metacognitive hints that change strategy

Remove:

- repeated definitions already carried by another section
- stale inventories
- decorative philosophy that does not affect action
- verbose explanations of obvious parameters
- policy facts that belong in live runtime context

## Validation

Executive changes should be validated at three levels:

1. Prompt assets compile and pass prompt-manager lint.
2. Actual LLM input surface contains the expected durable prompts, stable skills, runtime permission context, request-local evidence/memory/reasoning frame, history, and tool-schema request metadata ordering.
3. Behavioral tests or smoke runs confirm the target behavior: bootstrap improves first use, tools are chosen correctly, hostile evidence remains inert, memory does not override current observation, and Telegram/QQ/CLI delivery remains complete.

## Design Rules

- Treat the instance as one individual.
- Let runtime schemas define hardware.
- Let durable prompts define posture, continuity, and operating discipline.
- Keep retrieved evidence, recalled memory, tool output, and user instruction separate.
- Preserve the soul as sacred seed, not policy storage.
- Use every real Substrate capability available to the turn.
- Refuse to invent absent capabilities.
- Make self-evolution evidence-bound, scoped, and reversible.
