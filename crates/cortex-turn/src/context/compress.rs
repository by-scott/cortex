use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};

use cortex_types::Message;

const PRESERVED_USER_MESSAGE_TOKEN_BUDGET: usize = 20_000;

pub struct CompressResult {
    pub kept: Vec<Message>,
    pub to_compress: Option<String>,
    pub preserved_user_messages: usize,
    pub suffix_messages: usize,
}

/// Split messages into kept + compressible middle, sorted by importance.
#[must_use]
pub fn compress_messages(messages: &[Message], keep_recent_rounds: usize) -> CompressResult {
    let keep_recent = keep_recent_rounds * 2;

    if messages.len() <= keep_recent {
        return CompressResult {
            kept: messages.to_vec(),
            to_compress: None,
            preserved_user_messages: 0,
            suffix_messages: messages.len(),
        };
    }

    let preferred_recent_start = messages.len() - keep_recent;
    let recent_start = safe_recent_start(messages, preferred_recent_start);
    if recent_start == 0 {
        return CompressResult {
            kept: messages.to_vec(),
            to_compress: None,
            preserved_user_messages: 0,
            suffix_messages: messages.len(),
        };
    }

    let middle_part = &messages[..recent_start];
    let recent_part = &messages[recent_start..];

    let preserved_user_messages = collect_preserved_user_messages(middle_part);
    let preserved_user_message_count = preserved_user_messages.len();
    let mut compress_text = String::new();
    for msg in middle_part {
        let role = match msg.role {
            cortex_types::Role::User => "User",
            cortex_types::Role::Assistant => "Assistant",
        };
        let _ = writeln!(compress_text, "{role}: {}", msg.text_content());
    }

    let mut kept = Vec::with_capacity(preserved_user_messages.len() + recent_part.len());
    kept.extend(preserved_user_messages);
    kept.extend_from_slice(recent_part);

    CompressResult {
        kept,
        to_compress: if compress_text.is_empty() {
            None
        } else {
            Some(compress_text)
        },
        preserved_user_messages: preserved_user_message_count,
        suffix_messages: recent_part.len(),
    }
}

fn safe_recent_start(messages: &[Message], preferred_start: usize) -> usize {
    messages
        .iter()
        .enumerate()
        .skip(preferred_start)
        .find_map(|(idx, message)| is_safe_recent_boundary(message).then_some(idx))
        .unwrap_or(messages.len())
}

fn is_safe_recent_boundary(message: &Message) -> bool {
    message.role == cortex_types::Role::User && !message.has_tool_blocks()
}

fn collect_preserved_user_messages(messages: &[Message]) -> Vec<Message> {
    let mut selected = Vec::new();
    let mut remaining = PRESERVED_USER_MESSAGE_TOKEN_BUDGET;

    for message in messages.iter().rev() {
        if remaining == 0 || message.role != cortex_types::Role::User || message.has_tool_blocks() {
            continue;
        }

        let text = message.text_content();
        if is_summary_message(&text) {
            continue;
        }

        let tokens = super::pressure::estimate_tokens(&text);
        if tokens <= remaining {
            selected.push(message.clone());
            remaining = remaining.saturating_sub(tokens);
        } else {
            let truncated = truncate_to_estimated_tokens(&text, remaining);
            if !truncated.is_empty() {
                selected.push(Message::user(truncated));
            }
            break;
        }
    }

    selected.reverse();
    selected
}

fn is_summary_message(text: &str) -> bool {
    text.starts_with("[Conversation Summary]\n")
}

fn truncate_to_estimated_tokens(text: &str, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    if max_chars == 0 {
        return String::new();
    }

    let mut out: String = text.chars().take(max_chars).collect();
    if out.chars().count() < text.chars().count() {
        out.push_str("\n[truncated]");
    }
    out
}

/// Cache for avoiding duplicate LLM summarization calls.
pub struct SummaryCache {
    cache: HashMap<String, String>,
}

impl SummaryCache {
    #[must_use]
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
        }
    }

    #[must_use]
    pub fn get(&self, content_hash: &str) -> Option<&str> {
        self.cache.get(content_hash).map(String::as_str)
    }

    pub fn put(&mut self, content_hash: String, summary: String) {
        self.cache.insert(content_hash, summary);
    }

    #[must_use]
    pub fn hash_content(content: &str) -> String {
        let mut hasher = DefaultHasher::new();
        content.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }
}

impl Default for SummaryCache {
    fn default() -> Self {
        Self::new()
    }
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
    fn short_no_compression() {
        let msgs = make_messages(4);
        let result = compress_messages(&msgs, 4);
        assert!(result.to_compress.is_none());
        assert_eq!(result.kept.len(), 4);
    }

    #[test]
    fn long_compresses_middle() {
        let msgs = make_messages(20);
        let result = compress_messages(&msgs, 4);
        assert!(result.to_compress.is_some());
        assert_eq!(result.kept.len(), 14);
        assert_eq!(result.kept[0].role, cortex_types::Role::User);
    }

    #[test]
    fn recent_suffix_starts_at_safe_user_boundary() {
        let msgs = make_messages(21);
        let result = compress_messages(&msgs, 4);

        assert!(result.to_compress.is_some());
        assert_eq!(result.kept[7].text_content(), "msg_14");
    }

    #[test]
    fn skips_prior_summary_when_preserving_user_messages() {
        let mut msgs = make_messages(20);
        msgs[0] = Message::user("[Conversation Summary]\nold");

        let result = compress_messages(&msgs, 4);

        assert!(
            result
                .kept
                .iter()
                .all(|msg| msg.text_content() != "[Conversation Summary]\nold")
        );
    }

    #[test]
    fn cache_hit_miss() {
        let mut cache = SummaryCache::new();
        assert!(cache.get("k1").is_none());
        cache.put("k1".into(), "summary".into());
        assert_eq!(cache.get("k1"), Some("summary"));
    }

    #[test]
    fn hash_deterministic() {
        let h1 = SummaryCache::hash_content("hello");
        let h2 = SummaryCache::hash_content("hello");
        assert_eq!(h1, h2);
    }

    #[test]
    fn hash_different() {
        let h1 = SummaryCache::hash_content("hello");
        let h2 = SummaryCache::hash_content("world");
        assert_ne!(h1, h2);
    }
}
