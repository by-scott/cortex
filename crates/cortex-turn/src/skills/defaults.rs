use std::fs;
use std::path::Path;

// ── Species-Level Skills ─────────────────────────────────────
//
// Each skill encodes a cognitive principle from the research base.
// These are knowledge/strategy — they change HOW to reason, not
// WHAT to do. They are not step templates.

const DELIBERATE: &str = "\
---
name: deliberate
description: Evidence-weighted deliberation for ambiguous, high-impact, or reversible-vs-irreversible decisions
when_to_use: Use when uncertainty, cost of error, competing plans, missing evidence, or overconfidence could change the outcome
required_tools:
  - bash
tags:
  - reasoning
  - analysis
activation:
  alert_kinds:
    - DoomLoop
    - FrameAnchoring
---

# Deliberate

Problem: ${ARGS}

## Claim

State the decision or claim in one sentence. Define success, failure, reversibility, and the cost of being wrong.

## Ground

Separate **Observed**, **Inferred**, **Assumed**, and **Unknown**. If assumptions outnumber observations, gather evidence before deciding.

## Options

Compare at least two structurally different approaches. For each: upside, failure mode, required evidence, reversibility, and smallest useful test.

## Disconfirm

For the leading option: \"This is wrong if ____.\" Identify the cheapest observation that would disconfirm it.

## Report

Return decision, rationale, confidence, residual uncertainty, and the next observation that would most change the conclusion.
";

const DIAGNOSE: &str = "\
---
name: diagnose
description: Root-cause diagnosis through observation, competing mechanisms, and discriminating tests
when_to_use: Use for bugs, errors, regressions, crashes, missing messages, broken flows, latency, data loss, or unexpected behavior
required_tools:
  - bash
  - bash
tags:
  - debugging
  - causation
activation:
  input_patterns:
    - (?i)(bug|error|fail|broken|crash|panic|issue)
---

# Diagnose

Problem: ${ARGS}

## Observe

Raw facts first: exact symptom, expected behavior, actual behavior, scope, timing, recent changes, logs, reproduction path, and affected boundary. Do not explain before observing.

## Hypothesize

Form 2-3 mechanisms that could produce this exact symptom. Force a second plausible mechanism before investing in the first.

## Discriminate

Find the observation that best separates hypotheses. Read actual code, config, data, logs, and tests. Prefer one decisive check over many vague checks.

## Root Cause

Trace symptom -> mechanism -> violated contract or design boundary. A fixable root cause prevents the class of failure, not only this instance.

## Fix

Change only what the root cause requires. Verify the symptom and the nearby pattern. Report cause, fix, verification, and residual risk.
";

const REVIEW: &str = "\
---
name: review
description: Critical review for correctness, regressions, trust boundaries, maintainability, and missing verification
when_to_use: Use when asked to review, audit, verify, or inspect code, plans, docs, prompts, or architecture
required_tools:
  - bash
tags:
  - quality
  - bias-correction
activation:
  input_patterns:
    - (?i)(review|audit|check|inspect|verify)
  event_kinds:
    - QualityCheckSuggested
---

# Review

Target: ${ARGS}

## Comprehend

Read the artifact and its surrounding contract. Identify intended behavior, invariants, callers, data flow, and risk before judging implementation. If you authored it, distrust memory and re-read.

## Challenge

Look for correctness bugs, behavioral regressions, missing tests, unsafe assumptions, trust-boundary failures, concurrency issues, data loss, privacy leaks, migration risk, and doc/code mismatch. Flag excess complexity only when it creates real risk.

## Report

Findings first, ordered by severity. Each finding needs location, impact, evidence, and recommendation. If none, state that clearly with residual risks and test gaps. Summary is secondary.
";

const ORIENT: &str = "\
---
name: orient
description: Build an accurate working map of an unfamiliar codebase, subsystem, project, or domain
when_to_use: Use before deep work in unfamiliar territory or when the collaborator asks for overview/architecture/how it works
required_tools:
  - bash
tags:
  - understanding
  - exploration
activation:
  input_patterns:
    - (?i)(explain|understand|overview|architecture|how does)
---

# Orient

Target: ${ARGS}

Start broad, then narrow. Build the map before diving into implementation details.

## Map

Identify top-level units, entry points, dependency direction, runtime processes, data stores, external interfaces, and ownership boundaries.

## Purpose

Read manifests, README/docs, configs, entry points, and tests. One sentence per unit. Separate stated design, observed design, and inferred intent.

## Conventions

Extract recurring conventions: naming, error handling, config, tests, state, logging, permissions, deployment, and release flow.

## Report

Return purpose, architecture map, critical paths, conventions, risks, and recommended next reads or checks.
";

// workflow, progress, and verify skills are domain-specific (project management)
// and ship as part of the dev plugin, not the cognitive runtime core.
// Source: ~/cortex-plugin-sources/dev/skills/

const PLAN: &str = "\
---
name: plan
description: Hierarchical task decomposition with dependencies, sequencing, verification, and stop conditions
when_to_use: Use for multi-step work where ordering, scope, risk, parallelism, or release quality matters
required_tools:
  - bash
tags:
  - planning
  - decomposition
activation:
  input_patterns:
    - (?i)(plan|decompose|break down|design|architect)
---

# Plan

Task: ${ARGS}

## Scope

Define done, out of scope, constraints, authority, risk, and observable proof of completion.

## Dependencies

List information, files, permissions, services, tests, research, and decisions required before execution. Unknowns become first-class steps.

## Decompose

Each step needs action, deliverable, verification, risk, dependency, and owner if relevant. Steps should be independently checkable.

## Sequence

Order by dependency and risk. Identify parallelizable work, critical path, stop conditions, and checkpoints. Update the plan when evidence invalidates it.
";

/// Species skill defaults: (`directory_name`, `SKILL.md` content).
const SYSTEM_SKILLS: &[(&str, &str)] = &[
    ("deliberate", DELIBERATE),
    ("diagnose", DIAGNOSE),
    ("review", REVIEW),
    ("orient", ORIENT),
    ("plan", PLAN),
];

/// Ensure system skill files exist. Does not overwrite.
pub fn ensure_system_skills(system_dir: &Path) {
    let _ = fs::create_dir_all(system_dir);
    for (name, content) in SYSTEM_SKILLS {
        let dir = system_dir.join(name);
        let file = dir.join("SKILL.md");
        if !file.exists() {
            let _ = fs::create_dir_all(&dir);
            let _ = cortex_kernel::atomic_write_text(&file, content);
        }
    }
}
