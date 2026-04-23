use std::collections::HashMap;
use std::path::Path;

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

    std::fs::write(&file, content).map_err(|e| format!("write SKILL.md: {e}"))?;
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
    existing_names: &[String],
    skills_dir: &Path,
    weak_threshold: f64,
    strong_threshold: f64,
) -> EvolutionResult {
    let mut result = EvolutionResult {
        created: Vec::new(),
        flagged_weak: Vec::new(),
        confirmed_strong: Vec::new(),
    };

    // Evaluate existing skills
    for (name, &score) in utility_scores {
        if score < weak_threshold {
            result.flagged_weak.push((name.clone(), score));
        } else if score > strong_threshold {
            result.confirmed_strong.push((name.clone(), score));
        }
    }

    // Materialize new suggestions that don't overlap with existing skills
    for suggestion in suggestions {
        if existing_names.contains(&suggestion.name) {
            continue;
        }
        if materialize_suggestion(suggestion, skills_dir) == Ok(true) {
            result.created.push(suggestion.name.clone());
        }
    }

    result
        .flagged_weak
        .sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    result
        .confirmed_strong
        .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    result
}
