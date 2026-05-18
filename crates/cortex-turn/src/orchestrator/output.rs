/// Strip `<think>...</think>` blocks and orphaned `</think>` prefixes from LLM
/// output. Only applied to assistant responses so user-authored `<think>` text
/// is never touched.
pub fn strip_think_tags(text: &str) -> String {
    let text = extract_json_response(text);
    let text = re_think_block().replace_all(&text, "");
    let text = text.strip_prefix("</think>").unwrap_or(&text);
    text.trim().to_string()
}

fn re_think_block() -> &'static regex::Regex {
    static RE_THINK_BLOCK: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| match regex::Regex::new(r"(?s)<think>.*?</think>\s*") {
            Ok(regex) => regex,
            Err(err) => panic!("invalid think-block regex: {err}"),
        });
    &RE_THINK_BLOCK
}

fn extract_json_response(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.starts_with('{')
        && trimmed.ends_with('}')
        && let Ok(obj) = serde_json::from_str::<serde_json::Value>(trimmed)
        && let Some(response) = obj.get("response").and_then(serde_json::Value::as_str)
    {
        return response.to_string();
    }
    text.to_string()
}
