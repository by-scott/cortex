# Executive

The Executive is Cortex's operating system: the prompt, template, hint, and skill layer that drives the Substrate without duplicating it.

## Contract

The Substrate is implemented capability. The Executive is behavioral control. The Repertoire is reusable procedure. Mixing these layers creates stale prompts, hallucinated tools, and duplicated rules.

| Layer | Owns | Must not own |
|-------|------|--------------|
| Substrate | Runtime state, tools, channels, providers, memory, journal, risk gates, schemas | Personality, collaborator preferences |
| Executive | Values, self-model, operating protocol, bootstrap, evolution templates | Hard-coded tool catalogs, fake capabilities |
| Repertoire | Skills and reusable procedures | Identity, policy, long-term user facts |

## LLM Input Surface

Normal user turns assemble the LLM request from:

1. `soul.md`
2. `identity.md`
3. `behavioral.md`
4. `user.md`
5. Active skill summaries
6. Bootstrap or resume situational context
7. Recalled memory context
8. Reasoning state and metacognitive hints
9. Tool schemas
10. Message history and tool results

The tool schemas are the source of truth for available actions. Durable prompts may describe how to use capabilities, but must not hard-code exact tool inventories.

Before this surface reaches a provider, Cortex normalizes it into a provider-safe projection. The projection repairs tool pairing, removes empty messages, anchors assistant-leading histories, and keeps multimodal blocks limited to the turn that introduced them. This belongs to the Substrate, not the Executive: prompts should guide behavior, not compensate for protocol shape.

Long histories are compacted only at pressure boundaries. A compact boundary replaces prior message history with a summary plus preserved user context and a safe recent suffix, then records the replacement history in the journal. The Executive may reason over the resulting summary, but replay and continuity are owned by the journaled boundary.

## Prompt Layers

`soul.md` is the sacred seed: continuity, values, epistemology, autonomy, and relationship to the collaborator. It changes rarely and never becomes an operational checklist.

`identity.md` is the self-model: name, substrate awareness, capability boundaries, memory model, channels, and evolution posture. It may mention implemented subsystem classes, but runtime schemas override stale text.

`behavioral.md` is the operating protocol: sense, plan, execute, verify, reflect, metacognition, context pressure, risk, delegation, communication, and adaptation.

`user.md` is the collaborator model: identity, work, expertise, communication, environment, autonomy, boundaries, and durable corrections.

## Bootstrap

Bootstrap is first contact, not an intake form. It should establish:

- The collaborator's preferred language, identity, work, environment, and communication style.
- The instance's initial name or explicit unnamed state.
- Autonomy expectations, approval boundaries, privacy boundaries, and first working context.
- Enough profile data to make the second turn materially better than the first.

Bootstrap graduates only when identity initialization succeeds. The initialization template may rewrite prompt layers because it turns blank templates into real instance state.

## Evolution

Prompt evolution is evidence-bound:

- `user.md`: low threshold, additive updates from stable user signals.
- `behavioral.md`: medium threshold, only generalizable operating rules.
- `identity.md`: high threshold, confirmed name/self-model/substrate boundary changes.
- `soul.md`: rare threshold, value-level maturation through sustained experience.

The delivery draft is never prompt content. Evidence context is the source of truth.

## Skills

Skills are strategy programs. They do not define truth, identity, or available tools. They activate through patterns, context pressure, metacognitive alerts, events, or autonomous judgment, then provide a procedure for the current turn.

System skills are deliberately small:

- `deliberate`: evidence-weighted reasoning.
- `diagnose`: symptom-to-root-cause debugging.
- `review`: critical defect and risk review.
- `orient`: map unfamiliar systems.
- `plan`: decompose work into verifiable steps.

Domain-specific workflows belong in plugins or instance skills, not in the core Executive.

## Design Rules

- Do not duplicate layer responsibilities.
- Do not use prompts as a stale hardware inventory.
- Do not claim capabilities absent from runtime schemas or direct observation.
- Prefer observation over remembered assumptions.
- Keep first-use onboarding conversational, but make the resulting prompt state operationally useful.
- Preserve the soul as seed and carrier, not as policy storage.
