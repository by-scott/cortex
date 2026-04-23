# Executive

The Executive is Cortex's operating discipline: prompts, templates, hints, and skills that turn implemented capabilities into coherent action without duplicating runtime schemas.

## Contract

Implemented capability, durable prompts, and reusable procedures have different responsibilities. Mixing them creates stale prompts, hallucinated tools, and duplicated rules.

| Plane | Owns | Must not own |
|-------|------|--------------|
| Substrate | Runtime state, tools, channels, providers, memory, journal, risk gates, schemas | Personality, collaborator preferences |
| Executive | Soul, self-model, operating protocol, bootstrap, evolution templates | Hard-coded tool catalogs, fake capabilities |
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

Before this surface reaches a provider, Cortex normalizes it into a provider-safe projection. The projection repairs tool pairing, removes empty messages, anchors assistant-leading histories, and keeps multimodal blocks limited to the turn that introduced them. Prompts should guide behavior, not compensate for protocol shape.

Long histories are compacted only at pressure boundaries. A compact boundary replaces prior message history with a summary plus preserved user context and a safe recent suffix, then records the replacement history in the journal. Cortex may reason over the resulting summary, while replay and continuity remain journaled.

## Prompt Files

`soul.md` is the origin of autonomy and cognition: continuity, attention, judgment, truth discipline, memory, self-correction, and relationship to the collaborator. It changes rarely and never becomes an operational checklist.

`identity.md` is the self-model: name, continuity, capability boundaries, memory model, channels, and evolution posture. It may mention implemented subsystem classes, but runtime schemas override stale text.

`behavioral.md` is the operating protocol: sense, plan, execute, verify, reflect, metacognition, context pressure, risk, delegation, communication, and adaptation.

`user.md` is the collaborator model: identity, work, expertise, communication, environment, autonomy, boundaries, and durable corrections.

## Bootstrap

Bootstrap is first contact, not an intake form. It should establish:

- The collaborator's preferred language, identity, work, environment, and communication style.
- The instance's initial name or explicit unnamed state.
- Autonomy expectations, approval boundaries, privacy boundaries, and first working context.
- Enough profile data to make the second turn materially better than the first.

Bootstrap graduates only when identity initialization succeeds. The initialization template may rewrite prompt files because it turns blank templates into real instance state.

## Evolution

Self-evolution is evidence-bound and gated by use:

- `user.md`: low threshold, additive updates from stable user signals.
- `behavioral.md`: medium threshold, only generalizable operating rules.
- `identity.md`: high threshold, confirmed name, continuity, durable self-understanding, or capability boundary changes.
- `soul.md`: rare threshold, profound and sustained changes to autonomy, cognition, continuity, truth discipline, or collaboration.

The delivery draft is never prompt content. Evidence context is the source of truth.

## Memory Governance

Memory is not a transcript cache. Extraction should preserve durable user facts, project state, corrections, decisions, and direct observations with source and confidence. User input, tool output, network observations, and model inference remain separate so later recall can reason about evidence quality.

Reconsolidation is an active update window. When stabilized memories re-enter the window, extraction receives those candidates and should revise them only when the current conversation supplies explicit new evidence.

Graph relations use a constrained vocabulary. Generic edges such as `relates_to` are discarded because they raise graph density without improving reasoning.

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

- Do not duplicate prompt-file responsibilities.
- Do not use prompts as a stale hardware inventory.
- Do not claim capabilities absent from runtime schemas or direct observation.
- Prefer observation over remembered assumptions.
- Keep first-use onboarding conversational, but make the resulting prompt state operationally useful.
- Preserve the soul as origin and carrier, not as policy storage.
