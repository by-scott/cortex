use std::collections::{HashMap, HashSet};
use std::path::Path;

use cortex_types::{SkillEvolutionProposal, SkillEvolutionRelation, SkillHealth, SkillHealthState};

/// A suggested skill based on detected usage patterns.
#[derive(Debug, Clone)]
pub struct SkillSuggestion {
    pub name: String,
    pub description: String,
    pub tool_sequence: Vec<String>,
    pub frequency: usize,
}

/// Result of a skill evolution cycle.
#[derive(Debug, Clone)]
pub struct EvolutionResult {
    /// New skills materialized to disk.
    pub created: Vec<String>,
    /// Existing skills flagged for improvement (low utility).
    pub flagged_weak: Vec<(String, f64)>,
    /// Existing skills confirmed as strong (high utility).
    pub confirmed_strong: Vec<(String, f64)>,
    /// Governance proposals created from new patterns or better alternatives.
    pub proposals: Vec<SkillEvolutionProposal>,
}

#[derive(Debug, Clone)]
pub struct ExistingSkillProfile {
    pub name: String,
    pub required_tools: Vec<String>,
    pub health: SkillHealth,
}

/// Detect repeated tool call patterns and suggest skills.
///
/// Analyzes tool call sequences of length 2-5. Patterns appearing >= `min_freq` times
/// are returned as suggestions.
#[must_use]
pub fn detect_patterns(tool_calls: &[String], min_freq: usize) -> Vec<SkillSuggestion> {
    let mut suggestions = Vec::new();
    for window_size in 2..=5.min(tool_calls.len()) {
        let mut counts: HashMap<Vec<&str>, usize> = HashMap::new();
        for window in tool_calls.windows(window_size) {
            let key: Vec<&str> = window.iter().map(String::as_str).collect();
            *counts.entry(key).or_default() += 1;
        }
        for (seq, count) in counts {
            if count >= min_freq {
                let name = seq.join("-then-");
                let desc = format!(
                    "Automates the pattern: {} (seen {count} times)",
                    seq.join(" \u{2192} "),
                );
                suggestions.push(SkillSuggestion {
                    name,
                    description: desc,
                    tool_sequence: seq.into_iter().map(String::from).collect(),
                    frequency: count,
                });
            }
        }
    }
    suggestions.sort_by_key(|s| std::cmp::Reverse(s.frequency));
    suggestions
}

/// Materialize a skill suggestion into a SKILL.md file on disk.
///
/// Writes to `{skills_dir}/{name}/SKILL.md`. Does NOT overwrite existing skills.
/// Returns `true` if the file was created, `false` if it already existed.
///
/// # Errors
///
/// Returns an error string if the directory or file cannot be created.
pub fn materialize_suggestion(
    suggestion: &SkillSuggestion,
    skills_dir: &Path,
) -> Result<bool, String> {
    let dir = skills_dir.join(&suggestion.name);
    let file = dir.join("SKILL.md");

    if file.exists() {
        return Ok(false);
    }

    std::fs::create_dir_all(&dir).map_err(|e| format!("create dir: {e}"))?;

    let tools_yaml = suggestion
        .tool_sequence
        .iter()
        .map(|t| format!("  - {t}"))
        .collect::<Vec<_>>()
        .join("\n");

    let content = format!(
        "\
---
description: {desc}
when_to_use: When performing the pattern {pattern}
required_tools:
{tools}
tags:
  - auto-discovered
  - pattern
activation:
  input_patterns: []
---

# {name}

This skill was automatically discovered from repeated usage patterns.

Pattern: {pattern} (observed {freq} times)

## Steps

{steps}
",
        desc = suggestion.description,
        pattern = suggestion.tool_sequence.join(" → "),
        tools = tools_yaml,
        name = suggestion.name,
        freq = suggestion.frequency,
        steps = suggestion
            .tool_sequence
            .iter()
            .enumerate()
            .map(|(i, t)| format!("{}. Execute `{t}` with appropriate parameters", i + 1))
            .collect::<Vec<_>>()
            .join("\n"),
    );

    cortex_kernel::atomic_write_text(&file, content).map_err(|e| format!("write SKILL.md: {e}"))?;
    Ok(true)
}

/// Evaluate skill effectiveness and produce evolution actions.
///
/// - Skills with utility < `weak_threshold` are flagged for improvement
/// - Skills with utility > `strong_threshold` are confirmed as strong
/// - Suggestions that don't duplicate existing skills are materialized
pub fn evolve_skills<S: std::hash::BuildHasher>(
    suggestions: &[SkillSuggestion],
    utility_scores: &HashMap<String, f64, S>,
    existing: &[ExistingSkillProfile],
    skills_dir: &Path,
    weak_threshold: f64,
    strong_threshold: f64,
) -> EvolutionResult {
    let mut result = EvolutionResult {
        created: Vec::new(),
        flagged_weak: Vec::new(),
        confirmed_strong: Vec::new(),
        proposals: Vec::new(),
    };

    // Evaluate existing skills
    for (name, &score) in utility_scores {
        if score < weak_threshold {
            result.flagged_weak.push((name.clone(), score));
        } else if score > strong_threshold {
            result.confirmed_strong.push((name.clone(), score));
        }
    }

    let existing_names: HashSet<&str> = existing.iter().map(|skill| skill.name.as_str()).collect();

    // Materialize new suggestions that don't overlap with existing skills
    for suggestion in suggestions {
        if existing_names.contains(suggestion.name.as_str()) {
            continue;
        }
        if materialize_suggestion(suggestion, skills_dir) != Ok(true) {
            continue;
        }

        result.created.push(suggestion.name.clone());
        result.proposals.push(proposal_for_suggestion(
            suggestion,
            best_related_skill(suggestion, existing),
        ));
    }

    result
        .flagged_weak
        .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    result
        .confirmed_strong
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    result
}

fn proposal_for_suggestion(
    suggestion: &SkillSuggestion,
    related: Option<&ExistingSkillProfile>,
) -> SkillEvolutionProposal {
    let evidence = vec![
        format!("observed pattern {} times", suggestion.frequency),
        format!("tool sequence: {}", suggestion.tool_sequence.join(" -> ")),
    ];
    if let Some(target) = related {
        let relation = if matches!(
            target.health.state,
            SkillHealthState::NeedsReview | SkillHealthState::Quarantined
        ) {
            SkillEvolutionRelation::CandidateReplacement
        } else if target.required_tools == suggestion.tool_sequence {
            SkillEvolutionRelation::AlternativeTo
        } else {
            SkillEvolutionRelation::Improves
        };
        SkillEvolutionProposal::new(
            relation,
            suggestion.name.clone(),
            Some(target.name.clone()),
            format!(
                "new repeated workflow may {} existing skill '{}'",
                relation.as_str(),
                target.name
            ),
            evidence,
        )
    } else {
        SkillEvolutionProposal::new(
            SkillEvolutionRelation::NewPattern,
            suggestion.name.clone(),
            None,
            "new repeated workflow has no close existing skill".to_string(),
            evidence,
        )
    }
}

fn best_related_skill<'a>(
    suggestion: &SkillSuggestion,
    existing: &'a [ExistingSkillProfile],
) -> Option<&'a ExistingSkillProfile> {
    existing
        .iter()
        .filter_map(|skill| {
            let overlap = tool_overlap_ratio(&suggestion.tool_sequence, &skill.required_tools);
            (overlap >= 0.5).then_some((skill, overlap))
        })
        .max_by(|left, right| {
            left.1
                .partial_cmp(&right.1)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .map(|(skill, _)| skill)
}

fn tool_overlap_ratio(candidate: &[String], existing: &[String]) -> f64 {
    if candidate.is_empty() || existing.is_empty() {
        return 0.0;
    }
    let existing_tools: HashSet<&str> = existing.iter().map(String::as_str).collect();
    let overlap = candidate
        .iter()
        .filter(|tool| existing_tools.contains(tool.as_str()))
        .count();
    let denominator = candidate.len().max(existing.len());
    f64::from(u32::try_from(overlap).unwrap_or(u32::MAX))
        / f64::from(u32::try_from(denominator).unwrap_or(u32::MAX))
}
