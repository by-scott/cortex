use cortex_types::Message;

use super::pressure::estimate_tokens;

pub const DEFAULT_KEEP_RECENT_ROUNDS: usize = 4;
const KEEP_FIRST: usize = 2;

/// Trim history keeping first round and recent rounds.
/// Returns count of messages removed.
pub fn trim_sliding_window(messages: &mut Vec<Message>, keep_recent_rounds: usize) -> usize {
    let keep_recent = keep_recent_rounds * 2;
    let threshold = KEEP_FIRST + keep_recent;
    if messages.len() <= threshold {
        return 0;
    }
    let drain_end = messages.len() - keep_recent;
    messages.drain(KEEP_FIRST..drain_end).count()
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
        let mut msgs = make_messages(10);
        assert_eq!(trim_sliding_window(&mut msgs, 4), 0);
    }

    #[test]
    fn long_trimmed() {
        let mut msgs = make_messages(20);
        let removed = trim_sliding_window(&mut msgs, 4);
        assert_eq!(removed, 10);
        assert_eq!(msgs.len(), 10);
        assert_eq!(msgs[0].text_content(), "msg_0");
        assert_eq!(msgs[1].text_content(), "reply_1");
    }

    #[test]
    fn custom_keep_rounds() {
        let mut msgs = make_messages(20);
        let removed = trim_sliding_window(&mut msgs, 2);
        assert_eq!(removed, 14);
        assert_eq!(msgs.len(), 6);
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
