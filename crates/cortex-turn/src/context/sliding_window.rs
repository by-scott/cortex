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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_messages(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Message::user(format!("msg_{i}"))
                } else {
                    Message::assistant(format!("reply_{i}"))
                }
            })
            .collect()
    }

    #[test]
    fn short_no_trim() {
        let mut msgs = make_messages(6);
        assert_eq!(trim_sliding_window(&mut msgs, 4), 0);
        assert_eq!(msgs.len(), 6);
    }

    #[test]
    fn exact_threshold_no_trim() {
        let mut msgs = make_messages(20);
        assert_eq!(trim_sliding_window(&mut msgs, 8), 0);
    }

    #[test]
    fn long_trimmed() {
        let mut msgs = make_messages(20);
        let removed = trim_sliding_window(&mut msgs, 4);
        assert_eq!(removed, 8);
        assert_eq!(msgs.len(), 12);
        assert_eq!(
            msgs.last().map(Message::text_content).as_deref(),
            Some("reply_19")
        );
    }

    #[test]
    fn custom_keep_rounds() {
        let mut msgs = make_messages(20);
        let removed = trim_sliding_window(&mut msgs, 2);
        assert_eq!(removed, 12);
        assert_eq!(msgs.len(), 8);
    }

    #[test]
    fn empty_no_trim() {
        let mut msgs: Vec<Message> = Vec::new();
        assert_eq!(trim_sliding_window(&mut msgs, 4), 0);
    }

    #[test]
    fn estimate_tokens() {
        let msgs = make_messages(2);
        assert!(estimate_history_tokens(&msgs) > 0);
    }
}
