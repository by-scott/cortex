use cortex_types::Message;

use crate::llm::client::LlmClient;
use crate::llm::types::LlmRequest;

use super::compress::{SummaryCache, compress_messages};
use super::pressure::estimate_tokens;
use super::sliding_window::{DEFAULT_KEEP_RECENT_ROUNDS, trim_sliding_window};

const CONTENT_PLACEHOLDER: &str = "{content}";

pub enum SummarizeResult {
    NoCompression,
    Summarized {
        original_tokens: usize,
        compressed_tokens: usize,
    },
    Fallback {
        removed: usize,
    },
}

/// Summarize and compress conversation history.
///
/// 1. Split into kept + compressible via importance scoring
/// 2. Check summary cache (return cached if hit)
/// 3. Call LLM to summarize compressible portion
/// 4. Inject `[Conversation Summary]` synthetic message
/// 5. Fallback: sliding window trim on LLM failure
pub async fn summarize_and_compress(
    history: &mut Vec<Message>,
    llm: &dyn LlmClient,
    template: &str,
    max_tokens: usize,
    summary_cache: &mut SummaryCache,
) -> SummarizeResult {
    let result = compress_messages(history, DEFAULT_KEEP_RECENT_ROUNDS);
    let Some(to_compress) = result.to_compress else {
        return SummarizeResult::NoCompression;
    };

    let content_hash = SummaryCache::hash_content(&to_compress);
    let original_tokens = estimate_tokens(&to_compress);

    // Cache hit
    if let Some(cached) = summary_cache.get(&content_hash) {
        let compressed_tokens = estimate_tokens(cached);
        inject_summary(history, &result.kept, cached);
        return SummarizeResult::Summarized {
            original_tokens,
            compressed_tokens,
        };
    }

    // LLM summarization
    if let Ok(summary) = call_llm_summarize(&to_compress, llm, template, max_tokens).await {
        let compressed_tokens = estimate_tokens(&summary);
        summary_cache.put(content_hash, summary.clone());
        inject_summary(history, &result.kept, &summary);
        SummarizeResult::Summarized {
            original_tokens,
            compressed_tokens,
        }
    } else {
        let removed = trim_sliding_window(history, DEFAULT_KEEP_RECENT_ROUNDS);
        SummarizeResult::Fallback { removed }
    }
}

fn inject_summary(history: &mut Vec<Message>, kept: &[Message], summary: &str) {
    history.clear();
    if kept.len() >= 2 {
        history.extend_from_slice(&kept[..2]);
    }
    history.push(Message::assistant(format!(
        "[Conversation Summary]\n{summary}"
    )));
    if kept.len() > 2 {
        history.extend_from_slice(&kept[2..]);
    }
}

fn render_template(template: &str, content: &str) -> String {
    template.replace(CONTENT_PLACEHOLDER, content)
}

async fn call_llm_summarize(
    text: &str,
    llm: &dyn LlmClient,
    template: &str,
    max_tokens: usize,
) -> Result<String, String> {
    let prompt = render_template(template, text);
    let system = cortex_kernel::prompt_manager::DEFAULT_SUMMARIZE_SYSTEM;
    let messages = [Message::user(&prompt)];
    let request = LlmRequest {
        system: Some(system),
        messages: &messages,
        tools: None,
        max_tokens,
        on_text: None,
    };
    let response = llm.complete(request).await.map_err(|e| e.to_string())?;
    response.text.ok_or_else(|| "no text in response".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_replaces() {
        let result = render_template("Summarize: {content}", "hello world");
        assert_eq!(result, "Summarize: hello world");
    }

    #[test]
    fn render_template_no_placeholder() {
        let result = render_template("No placeholder here", "content");
        assert_eq!(result, "No placeholder here");
    }

    #[test]
    fn cache_roundtrip() {
        let mut cache = SummaryCache::new();
        let hash = SummaryCache::hash_content("test");
        cache.put(hash.clone(), "summary".into());
        assert_eq!(cache.get(&hash), Some("summary"));
    }
}
