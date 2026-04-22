use std::fs;
use std::path::Path;

// ── Species-Level Skills ─────────────────────────────────────
//
// Each skill encodes a cognitive principle from the research base.
// These are knowledge/strategy — they change HOW to reason, not
// WHAT to do. They are not step templates.

const DELIBERATE: &str = "\
---
description: Slow evidence-weighted reasoning for complex, ambiguous, or high-impact decisions
when_to_use: Use when the task has uncertainty, competing approaches, high cost of error, or insufficient evidence
required_tools:
  - read
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

## Frame

State the decision or claim being evaluated in one sentence. Name what would count as success and what would count as failure.

## Evidence

Separate **Observed**, **Inferred**, **Assumed**, and **Unknown**. If assumptions outnumber observations, gather evidence before deciding.

## Alternatives

Generate at least two structurally different approaches. For each, state upside, failure mode, required evidence, and reversibility.

## Falsify

For the leading approach, complete: \"This is wrong if ____.\" Identify the cheapest observation that would disconfirm it.

## Report

Return: decision, rationale, confidence, remaining uncertainty, and the next verification that would most change the conclusion.
";

const DIAGNOSE: &str = "\
---
description: Trace symptoms to root cause through hypothesis testing
when_to_use: Use for bugs, errors, regressions, crashes, missing messages, broken flows, or unexpected behavior
required_tools:
  - read
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

Raw facts first. Capture exact symptom, expected behavior, actual behavior, scope, timing, recent changes, logs, and reproduction path. Do not explain before observing.

## Hypothesize

Form 2-3 mechanisms that could produce this exact symptom. Force a second plausible mechanism before investing in the first.

## Discriminate

Find the observation that best separates hypotheses. Read actual code/config/log paths. Prefer one decisive test over many vague checks.

## Root Cause

Trace from symptom to mechanism to design boundary. A fixable root cause prevents recurrence, not just the observed instance.

## Fix

Change only what the root cause requires. Verify the symptom is fixed and the pattern does not exist nearby. Report cause, fix, and verification.
";

const REVIEW: &str = "\
---
description: Critical review for defects, regressions, risk, and missing verification
when_to_use: Use when asked to review, audit, verify, or inspect code, plans, docs, prompts, or architecture
required_tools:
  - read
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

Read the artifact and its surrounding contract. Identify intended behavior before judging implementation. If you authored it, distrust memory and re-read.

## Challenge

Look for correctness bugs, behavioral regressions, missing tests, unsafe assumptions, trust-boundary failures, concurrency issues, data loss, privacy leaks, and mismatches between docs and code. Also identify excess complexity when it creates risk.

## Report

Findings first, ordered by severity. Each finding needs location, impact, evidence, and recommendation. If no findings, state residual risks and testing gaps. Summary is secondary.
";

const ORIENT: &str = "\
---
description: Build an accurate map of an unfamiliar codebase, subsystem, project, or domain
when_to_use: Use before deep work in unfamiliar territory or when the collaborator asks for overview/architecture/how it works
required_tools:
  - read
tags:
  - understanding
  - exploration
activation:
  input_patterns:
    - (?i)(explain|understand|overview|architecture|how does)
---

# Orient

Target: ${ARGS}

Start broad, then narrow. Do not read implementation before you know the shape.

## Map

Identify top-level units, entry points, dependency direction, runtime processes, data stores, and external interfaces.

## Purpose

Read manifests, README/docs, configs, entry points, and tests. One sentence per unit. Separate stated design from observed design.

## Conventions

Extract recurring conventions: naming, error handling, config, tests, state management, logging, permissions, and deployment.

## Report

Return purpose, architecture map, critical paths, conventions, risks, and recommended next reads.
";

// workflow, progress, and verify skills are domain-specific (project management)
// and ship as part of the dev plugin, not the cognitive runtime core.
// Source: ~/cortex-plugin-sources/dev/skills/

const PLAN: &str = "\
---
description: Hierarchical task decomposition with dependencies, verification, and sequencing
when_to_use: Use for multi-step tasks where ordering, scope, risk, or parallelism matters
required_tools:
  - read
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

Define done, out of scope, constraints, and observable proof of completion.

## Dependencies

List information, files, permissions, services, tests, and decisions required before execution. Unknowns become first-class steps.

## Decompose

Each step needs: action, owner if relevant, deliverable, verification, risk, and dependency. Steps should be independently checkable.

## Sequence

Order by dependency and risk. Identify parallelizable work and critical path. Update the plan when evidence invalidates it.
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
            let _ = fs::write(&file, content);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_all_system_skills() {
        let dir = tempfile::tempdir().unwrap();
        ensure_system_skills(dir.path());
        for (name, _) in SYSTEM_SKILLS {
            assert!(
                dir.path().join(name).join("SKILL.md").exists(),
                "missing {name}/SKILL.md"
            );
        }
    }

    #[test]
    fn does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let sd = dir.path().join("deliberate");
        fs::create_dir_all(&sd).unwrap();
        fs::write(sd.join("SKILL.md"), "---\ndescription: Custom\n---\nMine").unwrap();
        ensure_system_skills(dir.path());
        let c = fs::read_to_string(sd.join("SKILL.md")).unwrap();
        assert!(c.contains("Custom"));
    }
}
