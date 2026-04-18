use super::{Tool, ToolError, ToolResult, block_on_tool_future};
use cortex_types::config::WebConfig;
use regex::Regex;

const MAX_URL_LENGTH: usize = 2000;
const MAX_CONTENT_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const TIMEOUT_SECS: u64 = 60;

pub struct WebFetchTool {
    config: WebConfig,
}

impl WebFetchTool {
    #[must_use]
    pub const fn new(config: WebConfig) -> Self {
        Self { config }
    }
}

impl Tool for WebFetchTool {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn description(&self) -> &'static str {
        "Fetch a URL and extract its text content.\n\n\
         Use for: reading documentation pages, extracting article text, checking \
         API responses, verifying page content, and any situation where you need \
         the actual content behind a URL (e.g. from web_search results).\n\n\
         HTTP URLs are auto-upgraded to HTTPS. HTML pages have script/style \
         blocks stripped and tags removed, producing clean text. Non-text content \
         types (images, PDFs, binaries) are rejected with a descriptive message.\n\n\
         Limits: URLs max 2000 characters, response body max 10 MB, 60-second \
         timeout. Text output is truncated at max_chars (default from config, \
         overridable per request, hard-capped by server limit).\n\n\
         Prefer this over bash+curl when you need clean extracted text from HTML. \
         Use bash+curl for raw HTTP responses, custom headers, or non-GET methods."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "maxLength": 2000,
                    "description": "HTTPS or HTTP URL to fetch. HTTP is auto-upgraded to HTTPS."
                },
                "prompt": {
                    "type": "string",
                    "minLength": 1,
                    "description": "What information to extract or focus on from the fetched page."
                },
                "max_chars": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum characters to return. Defaults to config value, hard-capped by server limit."
                }
            },
            "required": ["url", "prompt"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let url = input
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing url".into()))?;

        let prompt = input
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing prompt".into()))?;

        let max_chars = input
            .get("max_chars")
            .and_then(serde_json::Value::as_u64)
            .map_or(self.config.fetch_max_chars, |v| {
                usize::try_from(v).unwrap_or(self.config.fetch_max_chars)
            });
        let max_chars = max_chars.min(self.config.fetch_max_chars_limit);

        let validated_url = validate_and_upgrade_url(url)?;

        let result = block_on_tool_future(async { fetch_url(&validated_url).await })?;

        let content = process_content(&result.body, &result.content_type, max_chars);

        Ok(format_output(prompt, &content))
    }
}

struct FetchResult {
    body: String,
    content_type: String,
}

fn validate_and_upgrade_url(url: &str) -> Result<String, ToolError> {
    if url.len() > MAX_URL_LENGTH {
        return Err(ToolError::InvalidInput(format!(
            "URL exceeds {MAX_URL_LENGTH} character limit"
        )));
    }

    let url = if url.starts_with("http://") {
        url.replacen("http://", "https://", 1)
    } else {
        url.to_string()
    };

    if !url.starts_with("https://") {
        return Err(ToolError::InvalidInput(
            "only HTTP and HTTPS URLs are supported".into(),
        ));
    }

    // Basic URL format validation: must have a host after the scheme
    let after_scheme = &url["https://".len()..];
    if after_scheme.is_empty() || after_scheme.starts_with('/') || after_scheme.starts_with(':') {
        return Err(ToolError::InvalidInput("invalid URL format".into()));
    }

    Ok(url)
}

async fn fetch_url(url: &str) -> Result<FetchResult, ToolError> {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(TIMEOUT_SECS))
        .build()
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to create HTTP client: {e}")))?;

    let resp = client
        .get(url)
        .header("User-Agent", "Cortex/0.2 (cognitive-runtime)")
        .send()
        .await
        .map_err(|e| {
            if e.is_timeout() {
                ToolError::ExecutionFailed("request timed out after 60 seconds".into())
            } else {
                ToolError::ExecutionFailed(format!("HTTP request failed: {e}"))
            }
        })?;

    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_lowercase();

    let bytes = resp
        .bytes()
        .await
        .map_err(|e| ToolError::ExecutionFailed(format!("failed to read response body: {e}")))?;

    if bytes.len() > MAX_CONTENT_BYTES {
        let truncated = &bytes[..MAX_CONTENT_BYTES];
        let body = String::from_utf8_lossy(truncated).to_string();
        return Ok(FetchResult { body, content_type });
    }

    let body = String::from_utf8_lossy(&bytes).to_string();
    Ok(FetchResult { body, content_type })
}

fn process_content(body: &str, content_type: &str, max_chars: usize) -> String {
    if is_non_text_content(content_type) {
        return format!(
            "Content type '{content_type}' is not supported for text extraction. \
             The URL returned binary/non-text content."
        );
    }

    let text = if content_type.contains("text/html") || content_type.contains("application/xhtml") {
        strip_html(body)
    } else {
        body.to_string()
    };

    truncate_text(&text, max_chars)
}

fn is_non_text_content(content_type: &str) -> bool {
    content_type.starts_with("image/")
        || content_type.starts_with("audio/")
        || content_type.starts_with("video/")
        || content_type.contains("application/pdf")
        || content_type.contains("application/octet-stream")
        || content_type.contains("application/zip")
}

/// Strip HTML tags, removing `script`/`style` blocks entirely.
fn strip_html(html: &str) -> String {
    // Remove script and style elements with their content
    let re_script = Regex::new(r"(?is)<script[^>]*>.*?</script>").unwrap();
    let text = re_script.replace_all(html, "");

    let re_style = Regex::new(r"(?is)<style[^>]*>.*?</style>").unwrap();
    let text = re_style.replace_all(&text, "");

    // Remove all remaining HTML tags
    let re_tags = Regex::new(r"<[^>]+>").unwrap();
    let text = re_tags.replace_all(&text, "");

    // Decode common HTML entities
    let text = decode_html_entities(&text);

    // Collapse whitespace
    let re_ws = Regex::new(r"[ \t]+").unwrap();
    let text = re_ws.replace_all(&text, " ");

    // Collapse multiple newlines
    let re_nl = Regex::new(r"\n{3,}").unwrap();
    let text = re_nl.replace_all(&text, "\n\n");

    text.trim().to_string()
}

fn decode_html_entities(text: &str) -> String {
    text.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&nbsp;", " ")
}

fn truncate_text(text: &str, max_chars: usize) -> String {
    use std::fmt::Write;

    if text.len() <= max_chars {
        return text.to_string();
    }

    // Find a safe truncation point (char boundary)
    let mut end = max_chars;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }

    let mut result = text[..end].to_string();
    let _ = write!(result, "\n\n[Content truncated at {max_chars} characters]");
    result
}

fn format_output(prompt: &str, content: &str) -> ToolResult {
    let output = format!("Prompt: {prompt}\n\n--- Fetched Content ---\n\n{content}");
    ToolResult::success(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Schema & Name ──

    fn default_config() -> WebConfig {
        WebConfig::default()
    }

    #[test]
    fn web_fetch_name() {
        assert_eq!(WebFetchTool::new(default_config()).name(), "web_fetch");
    }

    #[test]
    fn web_fetch_schema_required_fields() {
        let schema = WebFetchTool::new(default_config()).input_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        let fields: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(fields.contains(&"url"));
        assert!(fields.contains(&"prompt"));
    }

    // ── URL Validation ──

    #[test]
    fn valid_https_url() {
        let result = validate_and_upgrade_url("https://example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com");
    }

    #[test]
    fn invalid_url_rejected() {
        let result = validate_and_upgrade_url("not-a-url");
        assert!(result.is_err());
    }

    #[test]
    fn ftp_url_rejected() {
        let result = validate_and_upgrade_url("ftp://example.com/file");
        assert!(result.is_err());
    }

    #[test]
    fn url_too_long_rejected() {
        let long_url = format!("https://example.com/{}", "a".repeat(2000));
        let result = validate_and_upgrade_url(&long_url);
        assert!(result.is_err());
    }

    #[test]
    fn url_at_limit_accepted() {
        let path = "a".repeat(2000 - "https://e.co/".len());
        let url = format!("https://e.co/{path}");
        assert_eq!(url.len(), 2000);
        let result = validate_and_upgrade_url(&url);
        assert!(result.is_ok());
    }

    // ── HTTP->HTTPS Upgrade ──

    #[test]
    fn http_upgraded_to_https() {
        let result = validate_and_upgrade_url("http://example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com");
    }

    #[test]
    fn https_unchanged() {
        let result = validate_and_upgrade_url("https://example.com/path");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://example.com/path");
    }

    // ── HTML Stripping ──

    #[test]
    fn strip_basic_html() {
        let html = "<html><body><h1>Title</h1><p>Text content</p></body></html>";
        let text = strip_html(html);
        assert!(text.contains("Title"));
        assert!(text.contains("Text content"));
        assert!(!text.contains("<h1>"));
        assert!(!text.contains("<p>"));
    }

    #[test]
    fn strip_script_and_style() {
        const CSS_RULE: &str = "body{color:red}";
        let html = [
            "<p>Before</p><script>alert('xss')</script><style>",
            CSS_RULE,
            "</style><p>After</p>",
        ]
        .concat();
        let text = strip_html(&html);
        assert!(text.contains("Before"));
        assert!(text.contains("After"));
        assert!(!text.contains("alert"));
        assert!(!text.contains("color:red"));
    }

    #[test]
    fn decode_entities() {
        let html = "<p>A &amp; B &lt; C &gt; D &quot;E&quot; F&#39;s</p>";
        let text = strip_html(html);
        assert!(text.contains("A & B < C > D \"E\" F's"));
    }

    #[test]
    fn multiline_script_removed() {
        let html = "<script type=\"text/javascript\">\nvar x = 1;\nconsole.log(x);\n</script><p>Content</p>";
        let text = strip_html(html);
        assert!(!text.contains("var x"));
        assert!(text.contains("Content"));
    }

    // ── Content Truncation ──

    #[test]
    fn short_text_not_truncated() {
        let text = "Hello world";
        assert_eq!(truncate_text(text, 100), "Hello world");
    }

    #[test]
    fn long_text_truncated() {
        let text = "a".repeat(200);
        let result = truncate_text(&text, 100);
        assert!(result.len() < 200);
        assert!(result.contains("[Content truncated"));
    }

    #[test]
    fn truncation_at_char_boundary() {
        let text = "Hello 你好 world";
        let result = truncate_text(text, 10);
        // Should not panic on multi-byte chars
        assert!(result.contains("[Content truncated"));
    }

    // ── Content Type Detection ──

    #[test]
    fn html_content_stripped() {
        let content = process_content(
            "<h1>Title</h1><p>Body</p>",
            "text/html; charset=utf-8",
            100_000,
        );
        assert!(content.contains("Title"));
        assert!(!content.contains("<h1>"));
    }

    #[test]
    fn plain_text_returned_directly() {
        let content = process_content("Plain text content", "text/plain", 100_000);
        assert_eq!(content, "Plain text content");
    }

    #[test]
    fn pdf_content_unsupported() {
        let content = process_content("binary data", "application/pdf", 100_000);
        assert!(content.contains("not supported"));
    }

    #[test]
    fn image_content_unsupported() {
        let content = process_content("binary", "image/png", 100_000);
        assert!(content.contains("not supported"));
    }

    // ── Output Format ──

    #[test]
    fn output_includes_prompt_and_content() {
        let result = format_output("find the API endpoint", "endpoint: /api/v2/users");
        assert!(!result.is_error);
        assert!(result.output.contains("find the API endpoint"));
        assert!(result.output.contains("endpoint: /api/v2/users"));
    }

    // ── Missing Input ──

    #[test]
    fn missing_url_rejected() {
        let tool = WebFetchTool::new(default_config());
        let result = tool.execute(serde_json::json!({"prompt": "test"}));
        assert!(result.is_err());
    }

    #[test]
    fn missing_prompt_rejected() {
        let tool = WebFetchTool::new(default_config());
        let result = tool.execute(serde_json::json!({"url": "https://example.com"}));
        assert!(result.is_err());
    }
}
