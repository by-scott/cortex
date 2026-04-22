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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_repeated_pair() {
        let calls: Vec<String> = vec!["read", "grep", "read", "grep", "read", "grep", "bash"]
            .into_iter()
            .map(String::from)
            .collect();
        let suggestions = detect_patterns(&calls, 3);
        assert!(!suggestions.is_empty());
        assert!(
            suggestions
                .iter()
                .any(|s| s.tool_sequence == ["read", "grep"])
        );
    }

    #[test]
    fn no_suggestions_below_threshold() {
        let calls: Vec<String> = vec!["read", "grep", "bash"]
            .into_iter()
            .map(String::from)
            .collect();
        let suggestions = detect_patterns(&calls, 3);
        assert!(suggestions.is_empty());
    }

    #[test]
    fn longer_sequences_detected() {
        let calls: Vec<String> = vec![
            "read", "grep", "bash", "read", "grep", "bash", "read", "grep", "bash",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let suggestions = detect_patterns(&calls, 3);
        assert!(
            suggestions
                .iter()
                .any(|s| s.tool_sequence == ["read", "grep", "bash"])
        );
    }

    #[test]
    fn materialize_creates_skill_file() {
        let dir = tempfile::tempdir().unwrap();
        let suggestion = SkillSuggestion {
            name: "read-then-grep".into(),
            description: "Read then search pattern".into(),
            tool_sequence: vec!["read".into(), "grep".into()],
            frequency: 5,
        };
        let created = materialize_suggestion(&suggestion, dir.path()).unwrap();
        assert!(created);
        assert!(dir.path().join("read-then-grep/SKILL.md").exists());

        let content = std::fs::read_to_string(dir.path().join("read-then-grep/SKILL.md")).unwrap();
        assert!(content.contains("auto-discovered"));
        assert!(content.contains("read → grep"));
    }

    #[test]
    fn materialize_does_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let sd = dir.path().join("existing");
        std::fs::create_dir_all(&sd).unwrap();
        std::fs::write(sd.join("SKILL.md"), "custom").unwrap();

        let suggestion = SkillSuggestion {
            name: "existing".into(),
            description: "test".into(),
            tool_sequence: vec!["read".into()],
            frequency: 3,
        };
        let created = materialize_suggestion(&suggestion, dir.path()).unwrap();
        assert!(!created);

        let content = std::fs::read_to_string(sd.join("SKILL.md")).unwrap();
        assert_eq!(content, "custom");
    }

    #[test]
    fn evolve_skills_creates_and_evaluates() {
        let dir = tempfile::tempdir().unwrap();
        let suggestions = vec![SkillSuggestion {
            name: "new-skill".into(),
            description: "new".into(),
            tool_sequence: vec!["read".into(), "edit".into()],
            frequency: 4,
        }];
        let mut scores = HashMap::new();
        scores.insert("weak-skill".into(), 0.2);
        scores.insert("strong-skill".into(), 0.9);
        scores.insert("mid-skill".into(), 0.5);

        let result = evolve_skills(
            &suggestions,
            &scores,
            &["existing-skill".into()],
            dir.path(),
            0.3,
            0.8,
        );

        assert_eq!(result.created.len(), 1);
        assert_eq!(result.created[0], "new-skill");
        assert_eq!(result.flagged_weak.len(), 1);
        assert_eq!(result.flagged_weak[0].0, "weak-skill");
        assert_eq!(result.confirmed_strong.len(), 1);
        assert_eq!(result.confirmed_strong[0].0, "strong-skill");
    }

    #[test]
    fn evolve_skips_existing_skill_names() {
        let dir = tempfile::tempdir().unwrap();
        let suggestions = vec![SkillSuggestion {
            name: "already-exists".into(),
            description: "dup".into(),
            tool_sequence: vec!["read".into()],
            frequency: 3,
        }];
        let result = evolve_skills(
            &suggestions,
            &HashMap::new(),
            &["already-exists".into()],
            dir.path(),
            0.3,
            0.8,
        );
        assert!(result.created.is_empty());
    }
}
