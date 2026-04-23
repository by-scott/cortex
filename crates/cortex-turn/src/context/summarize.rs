use cortex_types::Message;

use crate::llm::client::LlmClient;
use crate::llm::types::LlmRequest;

use super::compress::{SummaryCache, compress_messages};
use super::pressure::estimate_tokens;
use super::sliding_window::DEFAULT_KEEP_RECENT_ROUNDS;

const CONTENT_PLACEHOLDER: &str = "{content}";

pub enum SummarizeResult {
    NoCompression,
    Summarized {
        original_tokens: usize,
        compressed_tokens: usize,
        preserved_user_messages: usize,
        suffix_messages: usize,
        summary: String,
        replacement_messages: Vec<Message>,
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
/// 5. Fallback: leave history unchanged on LLM failure
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
        let replacement_messages = history.clone();
        return SummarizeResult::Summarized {
            original_tokens,
            compressed_tokens,
            preserved_user_messages: result.preserved_user_messages,
            suffix_messages: result.suffix_messages,
            summary: cached.to_string(),
            replacement_messages,
        };
    }

    // LLM summarization
    if let Ok(summary) = call_llm_summarize(&to_compress, llm, template, max_tokens).await {
        let compressed_tokens = estimate_tokens(&summary);
        summary_cache.put(content_hash, summary.clone());
        inject_summary(history, &result.kept, &summary);
        let replacement_messages = history.clone();
        SummarizeResult::Summarized {
            original_tokens,
            compressed_tokens,
            preserved_user_messages: result.preserved_user_messages,
            suffix_messages: result.suffix_messages,
            summary,
            replacement_messages,
        }
    } else {
        SummarizeResult::Fallback { removed: 0 }
    }
}

fn inject_summary(history: &mut Vec<Message>, kept: &[Message], summary: &str) {
    history.clear();
    history.push(Message::user(format!("[Conversation Summary]\n{summary}")));
    history.extend_from_slice(kept);
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
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };
    let response = llm.complete(request).await.map_err(|e| e.to_string())?;
    response.text.ok_or_else(|| "no text in response".into())
}
