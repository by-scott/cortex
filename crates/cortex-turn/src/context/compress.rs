use std::collections::HashMap;
use std::fmt::Write;
use std::hash::{DefaultHasher, Hash, Hasher};

use cortex_types::Message;

use super::importance::score_messages;

const KEEP_FIRST: usize = 2;

pub struct CompressResult {
    pub kept: Vec<Message>,
    pub to_compress: Option<String>,
}

/// Split messages into kept + compressible middle, sorted by importance.
#[must_use]
pub fn compress_messages(messages: &[Message], keep_recent_rounds: usize) -> CompressResult {
    let keep_recent = keep_recent_rounds * 2;
    let threshold = KEEP_FIRST + keep_recent;

    if messages.len() <= threshold {
        return CompressResult {
            kept: messages.to_vec(),
            to_compress: None,
        };
    }

    let first_part = &messages[..KEEP_FIRST];
    let drain_end = messages.len() - keep_recent;
    let middle_part = &messages[KEEP_FIRST..drain_end];
    let recent_part = &messages[drain_end..];

    let scores = score_messages(middle_part);
    let mut indexed: Vec<(usize, f64)> = scores.into_iter().enumerate().collect();
    indexed.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut compress_text = String::new();
    for (i, _) in &indexed {
        let msg = &middle_part[*i];
        let role = match msg.role {
            cortex_types::Role::User => "User",
            cortex_types::Role::Assistant => "Assistant",
        };
        let _ = writeln!(compress_text, "{role}: {}", msg.text_content());
    }

    let mut kept = first_part.to_vec();
    kept.extend_from_slice(recent_part);

    CompressResult {
        kept,
        to_compress: if compress_text.is_empty() {
            None
        } else {
            Some(compress_text)
        },
    }
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
        assert_eq!(result.kept.len(), 10); // 2 first + 8 recent
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
