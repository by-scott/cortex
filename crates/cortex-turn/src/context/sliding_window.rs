use cortex_types::Message;

use super::importance::score_messages;
use super::pressure::estimate_tokens;

pub const DEFAULT_KEEP_RECENT_ROUNDS: usize = 8;
const KEEP_IMPORTANT_MIDDLE_MESSAGES: usize = 4;

fn important_middle_indices(messages: &[Message], keep_count: usize) -> Vec<usize> {
    if keep_count == 0 || messages.is_empty() {
        return Vec::new();
    }

    let scores = score_messages(messages);
    let mut indexed: Vec<(usize, f64)> = scores.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.0.cmp(&a.0))
    });
    indexed.truncate(keep_count.min(messages.len()));
    let mut kept: Vec<usize> = indexed.into_iter().map(|(idx, _)| idx).collect();
    kept.sort_unstable();
    kept
}

/// Trim history while favoring recent context and important older messages.
///
/// This avoids a hard first-message bias while still preserving
/// high-signal earlier context.
/// Returns count of messages removed.
pub fn trim_sliding_window(messages: &mut Vec<Message>, keep_recent_rounds: usize) -> usize {
    let keep_recent = keep_recent_rounds * 2;
    let threshold = keep_recent + KEEP_IMPORTANT_MIDDLE_MESSAGES;
    if messages.len() <= threshold {
        return 0;
    }

    let recent_start = messages.len() - keep_recent;
    let middle = &messages[..recent_start];
    let keep_indices = important_middle_indices(middle, KEEP_IMPORTANT_MIDDLE_MESSAGES);
    let mut kept = Vec::with_capacity(keep_indices.len() + keep_recent);
    for idx in keep_indices {
        kept.push(messages[idx].clone());
    }
    kept.extend_from_slice(&messages[recent_start..]);
    let removed = messages.len().saturating_sub(kept.len());
    *messages = kept;
    removed
}

/// Estimate total tokens across all messages.
#[must_use]
pub fn estimate_history_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum()
}
