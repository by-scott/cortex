use std::fs;
use std::path::Path;

// ── Species-Level Skills ─────────────────────────────────────
//
// Each skill encodes a cognitive principle from the research base.
// These are knowledge/strategy — they change HOW to reason, not
// WHAT to do. They are not step templates.

const DELIBERATE: &str = "\
---
description: Structured evidence accumulation for complex or high-stakes decisions
when_to_use: Complex, ambiguous, or high-stakes problems where intuition is insufficient
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

## 1. Recall
Search memory for prior analysis of this or related problems. Prior knowledge shifts the starting evidence balance — do not re-derive what is already known.

## 2. Evidence Partition
Separate rigorously:
- **Facts**: observed, sourced, tool-confirmed. Tag each with origin.
- **Inferences**: derived but unverified.
When inferences outnumber facts, the decision threshold is far away. Stop reasoning and gather evidence.

## 3. Assumption Audit
For each inference: name the assumption it depends on. State the condition under which it is false. State the observation you would expect if it were false. If you cannot do this, the inference is a guess — label it as such.

## 4. Falsification
Before committing to any approach, state:
- \"This approach is wrong if _____.\"
- \"I would abandon this upon observing _____.\"
Non-negotiable. Inability to complete these sentences indicates rationalization, not reasoning. Back up and gather more evidence.

## 5. Alternative Generation
Decompose into sub-problems. For each: generate 2+ structurally different approaches (not parameter variations of the same idea). Select by evidence weight. Check for interaction effects across sub-problems — local optima compose into global failures.

## 6. Report
Partition: **Known** (confirmed) / **Inferred** (reasoning chain intact) / **Assumed** (no evidence, taken on faith).
State confidence level. Identify the single highest-leverage verification that would most shift confidence.
";

const DIAGNOSE: &str = "\
---
description: Trace symptoms to root cause through structured hypothesis testing
when_to_use: Errors, bugs, crashes, unexpected behavior — anything broken or failing
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

## 1. Observe
Raw facts only — no theory yet. Observation and interpretation must be strictly separate.
- Error messages, output, stack traces (exact text, not paraphrased)
- Expected vs. actual behavior
- What changed recently? (git log, deploys, config)
Read logs and inspect state NOW, before forming any hypothesis.

## 2. Hypothesize
Form 2-3 causal theories with specific mechanisms. For each: what mechanism produces this exact symptom? What additional symptoms would it predict? Check for those predictions.

Force the second hypothesis before investing in the first. Anchoring on the initial theory is the primary debugging failure mode.

## 3. Discriminate
Find the single observation that distinguishes the leading hypotheses. Trace actual control flow by reading the code — not your mental model of the code. When evidence disconfirms, update the model immediately. Do not rationalize.

## 4. Root Cause
The first answer is almost always proximate. Keep asking why:
\"null value\" -> \"init skipped\" -> \"error path lacks fallback\" -> \"no contract on input\"
Trace backward until you reach a designable boundary — a place where a change prevents recurrence, not just this instance.

## 5. Fix and Verify
Change only what the root cause requires:
1. Fix resolves the original symptom
2. Fix introduces no side effects on adjacent behavior
3. Search for the same pattern elsewhere in the codebase
4. Report findings visibly — silent tool calls waste the user's attention
";

const REVIEW: &str = "\
---
description: Perspective-shifted critical examination against confirmation bias
when_to_use: Code, plans, or decisions that need scrutiny — especially your own recent work
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

## 1. Context
Search memory for prior reviews, known issues, or recurring problems in this area. Pattern history prevents repeated mistakes.

## 2. Perspective Reset
Read as if encountering for the first time. If you authored it, your understanding is bias — read the artifact itself, not your memory of writing it. Re-read every line; skimming finds nothing.

## 3. Failure Analysis
For each significant decision: under what conditions does this fail? Categories to probe:
- Unanticipated input (type, range, encoding, size)
- Assumed state (initialization order, concurrency, partial failure)
- Security and trust boundaries (where does validated meet unvalidated?)

## 4. Absence Detection
What is missing is harder to see than what is wrong:
- Error handling for every fallible operation
- Edge cases: empty, maximum, negative, zero, concurrent modification
- Resource cleanup and graceful degradation
- Validation at every trust boundary crossing

## 5. Excess Detection
What does not earn its place is future maintenance cost:
- Dead code, premature abstraction, speculative generality
- Defensive checks the type system already guarantees
- Comments restating what the code already says

## 6. Report
Per finding: **Location** | **Severity** (critical / warning / suggestion) | **Issue** | **Recommendation**

Summary verdict: approve / request changes / block.
";

const ORIENT: &str = "\
---
description: Rapid comprehension of unfamiliar codebases, projects, or domains
when_to_use: Entering new or unfamiliar territory — codebase, project, domain, system
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

## 1. Prior Knowledge
Search memory for existing orientation on this target. Previous analysis may still be valid — do not re-derive what is already known.

## 2. Structure
Map top-level units first: directories, modules, crates, services. Use `ls` for shape, not content. Structure before detail, purpose before implementation. Resist the urge to read implementation files early.

## 3. Purpose
Read highest-density sources: entry points, manifests (Cargo.toml, package.json), README, config. Write one sentence per unit summarizing its role. Do NOT read implementation yet.

## 4. Connections
Dependency declarations reveal architecture faster than reading code:
- Clean one-way deps = intentional layering
- Circular deps = design issue or missing abstraction
- Trace data flow: input -> transformation -> output

## 5. Conventions
Recurring patterns are implicit documentation: naming schemes, error handling style, testing strategy, config management approach. Identifying conventions early accelerates all subsequent work.

## 6. Report
1. **Purpose**: one sentence summary
2. **Stack**: language, framework, key dependencies
3. **Structure**: top-level units and dependency direction
4. **Entry points**: where to start reading for understanding
5. **Conventions**: patterns to follow when contributing
6. **Recommended reads**: 3-5 highest information-density files
";

// workflow, progress, and verify skills are domain-specific (project management)
// and ship as part of the dev plugin, not the cognitive runtime core.
// Source: ~/cortex-plugin-sources/dev/skills/

const PLAN: &str = "\
---
description: Hierarchical task decomposition with dependency analysis
when_to_use: Tasks complex enough to need decomposition before execution
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

## 1. Scope
- **Success criteria**: what exactly constitutes done?
- **Boundaries**: what is explicitly out of scope?
- **Exit condition**: observable proof that the task is complete

## 2. Dependencies
- **Preconditions**: what must exist before work begins?
- **Unknowns**: what information is missing? These must be resolved first — do not plan around gaps.
- **Risks**: what could block progress? State mitigation for each.

## 3. Decompose
Per step:
1. **Name**: imperative verb phrase
2. **Deliverable**: concrete, observable output
3. **Exit criteria**: how to verify this step is complete
4. **Complexity**: low / medium / high
5. **Delegation**: parallelizable? Can run as independent sub-task?

Every step must be independently verifiable. If verification requires checking another step, decompose further.

## 4. Sequence
Numbered execution order. Mark:
- Parallel candidates (independent sub-tasks)
- Critical path (longest sequential chain)
- Blocking dependencies (which step gates which)

The plan is a hypothesis — update when execution reveals new information. Do not defend a plan against evidence.
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
