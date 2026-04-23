use super::delegation::DelegationResult;
use std::collections::HashSet;

/// Strategy for aggregating multiple worker results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AggregationStrategy {
    /// Select the most common output (majority vote).
    Vote,
    /// Select the highest quality output (longest/most detailed).
    Quality,
    /// Merge all outputs with attribution.
    Merge,
}

/// Result of an orchestrated multi-agent execution.
#[derive(Debug)]
pub struct OrchestrationResult {
    /// The aggregated output.
    pub output: String,
    /// Whether conflicting results were detected.
    pub has_conflicts: bool,
    /// Individual task results.
    pub task_results: Vec<DelegationResult>,
    /// Strategy used for aggregation.
    pub strategy: AggregationStrategy,
}

/// Decompose a complex goal into subtask descriptions by parsing LLM output.
///
/// Expected format: JSON array of strings `["subtask1", "subtask2"]`.
/// Returns empty vec on parse failure.
#[must_use]
pub fn parse_decompose_response(response: &str) -> Vec<String> {
    let trimmed = response.trim();
    let json_str = if trimmed.starts_with("```") {
        trimmed
            .strip_prefix("```json")
            .or_else(|| trimmed.strip_prefix("```"))
            .and_then(|s| s.rsplit_once("```"))
            .map_or(trimmed, |(content, _)| content.trim())
    } else {
        trimmed
    };

    serde_json::from_str::<Vec<String>>(json_str).unwrap_or_default()
}

/// Decompose a goal into subtasks by calling the LLM.
///
/// Returns a list of subtask descriptions. On failure, returns a single task with the original goal.
pub async fn decompose_goal(
    goal: &str,
    llm: &dyn crate::llm::client::LlmClient,
    max_tokens: usize,
) -> Vec<String> {
    let prompt = format!(
        "Decompose the following goal into 2-5 independent subtasks that can be executed in parallel.\n\n\
         Goal: {goal}\n\n\
         Respond with ONLY a JSON array of subtask descriptions (no markdown fences, no extra text):\n\
         [\"subtask 1 description\", \"subtask 2 description\"]"
    );

    let messages = vec![cortex_types::Message {
        role: cortex_types::Role::User,
        content: vec![cortex_types::ContentBlock::Text { text: prompt }],
        attachments: Vec::new(),
    }];

    let request = crate::llm::types::LlmRequest {
        system: None,
        messages: &messages,
        tools: None,
        max_tokens,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    match llm.complete(request).await {
        Ok(resp) => {
            let text = resp.text.unwrap_or_default();
            let subtasks = parse_decompose_response(&text);
            if subtasks.is_empty() {
                vec![goal.to_string()]
            } else {
                subtasks
            }
        }
        Err(_) => vec![goal.to_string()],
    }
}

/// Aggregate delegation results using the specified strategy.
#[must_use]
pub fn aggregate_by_strategy(
    results: &[DelegationResult],
    strategy: AggregationStrategy,
) -> String {
    let successful: Vec<&DelegationResult> = results.iter().filter(|r| r.success).collect();

    if successful.is_empty() {
        return "All tasks failed.".to_string();
    }

    match strategy {
        AggregationStrategy::Vote => {
            // Count output occurrences, select most common
            let mut counts: Vec<(&str, usize)> = Vec::new();
            for r in &successful {
                if let Some(entry) = counts
                    .iter_mut()
                    .find(|(text, _)| *text == r.output.as_str())
                {
                    entry.1 += 1;
                } else {
                    counts.push((&r.output, 1));
                }
            }
            counts.sort_by_key(|item| std::cmp::Reverse(item.1));
            counts
                .first()
                .map_or_else(String::new, |(text, _)| (*text).to_string())
        }
        AggregationStrategy::Quality => {
            // Select longest output as highest quality proxy
            successful
                .iter()
                .max_by_key(|r| r.output.len())
                .map_or_else(String::new, |r| r.output.clone())
        }
        AggregationStrategy::Merge => {
            // Combine all outputs with attribution
            successful
                .iter()
                .map(|r| format!("[{}]: {}", r.name, r.output))
                .collect::<Vec<_>>()
                .join("\n\n")
        }
    }
}

/// Detect conflicts between worker outputs using Jaccard similarity.
///
/// Two outputs are conflicting if their Jaccard term similarity is below the threshold.
/// Returns true if any pair of successful results conflicts.
#[must_use]
pub fn detect_conflicts(results: &[DelegationResult]) -> bool {
    const CONFLICT_THRESHOLD: f64 = 0.3;

    let successful: Vec<&DelegationResult> = results.iter().filter(|r| r.success).collect();

    if successful.len() < 2 {
        return false;
    }

    let term_sets: Vec<HashSet<&str>> = successful
        .iter()
        .map(|r| r.output.split_whitespace().collect())
        .collect();

    for i in 0..term_sets.len() {
        for j in (i + 1)..term_sets.len() {
            let intersection = term_sets[i].intersection(&term_sets[j]).count();
            let union = term_sets[i].union(&term_sets[j]).count();
            if union > 0 {
                let intersection_u32 = u32::try_from(intersection).unwrap_or(u32::MAX);
                let union_u32 = u32::try_from(union).unwrap_or(1);
                let jaccard = f64::from(intersection_u32) / f64::from(union_u32);
                if jaccard < CONFLICT_THRESHOLD {
                    return true;
                }
            }
        }
    }

    false
}

/// Build a complete orchestration result from individual task results.
#[must_use]
pub fn build_orchestration_result(
    task_results: Vec<DelegationResult>,
    strategy: AggregationStrategy,
) -> OrchestrationResult {
    let output = aggregate_by_strategy(&task_results, strategy);
    let has_conflicts = detect_conflicts(&task_results);

    OrchestrationResult {
        output,
        has_conflicts,
        task_results,
        strategy,
    }
}
