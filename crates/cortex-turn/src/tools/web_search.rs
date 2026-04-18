use super::{Tool, ToolError, ToolResult, block_on_tool_future};
use cortex_types::config::WebConfig;

pub struct WebSearchTool {
    config: WebConfig,
}

impl WebSearchTool {
    #[must_use]
    pub const fn new(config: WebConfig) -> Self {
        Self { config }
    }
}

impl Tool for WebSearchTool {
    fn name(&self) -> &'static str {
        "web_search"
    }

    fn description(&self) -> &'static str {
        "Search the web for current, real-time information.\n\n\
         Use for: verifying facts beyond training data, finding documentation, \
         checking current versions or release notes, researching error messages, \
         discovering recent API changes, and any query where up-to-date results \
         matter.\n\n\
         Backed by Brave Search API when configured (recommended), with LLM-based \
         fallback when no API key is set. Set count to control result quantity \
         (default from config, hard-capped by server limit). Results include \
         ranked titles, URLs, and descriptions. Domain filters narrow or exclude \
         specific sites.\n\n\
         allowed_domains and blocked_domains are mutually exclusive — set one or \
         neither, not both. Queries under 2 characters are rejected.\n\n\
         Prefer this over bash+curl for structured search. Use bash+curl only for \
         direct API calls to known endpoints."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "minLength": 2,
                    "description": "Natural language search query. Be specific for better results."
                },
                "allowed_domains": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Restrict results to these domains (e.g. [\"docs.rs\", \"crates.io\"]). Mutually exclusive with blocked_domains."
                },
                "blocked_domains": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Exclude results from these domains (e.g. [\"pinterest.com\"]). Mutually exclusive with allowed_domains."
                },
                "count": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Number of search results to return. Defaults to config value, hard-capped by server limit."
                }
            },
            "required": ["query"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let query = input
            .get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing query".into()))?;

        if query.len() < 2 {
            return Err(ToolError::InvalidInput(
                "query must be at least 2 characters".into(),
            ));
        }

        let allowed_domains = parse_string_array(&input, "allowed_domains");
        let blocked_domains = parse_string_array(&input, "blocked_domains");

        if !allowed_domains.is_empty() && !blocked_domains.is_empty() {
            return Err(ToolError::InvalidInput(
                "allowed_domains and blocked_domains are mutually exclusive".into(),
            ));
        }

        let count = input
            .get("count")
            .and_then(serde_json::Value::as_u64)
            .map_or(self.config.brave_max_results, |v| {
                usize::try_from(v).unwrap_or(self.config.brave_max_results)
            });
        let count = count.min(self.config.brave_max_results_limit);

        match self.config.search_backend.as_str() {
            "brave" => execute_brave(
                query,
                &allowed_domains,
                &blocked_domains,
                &self.config,
                count,
            ),
            _ => Ok(execute_llm_search(
                query,
                &allowed_domains,
                &blocked_domains,
            )),
        }
    }
}

fn parse_string_array(input: &serde_json::Value, key: &str) -> Vec<String> {
    input
        .get(key)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

/// Build a Brave search API query string with domain filters applied.
fn build_brave_query(query: &str, allowed: &[String], blocked: &[String]) -> String {
    use std::fmt::Write;
    let mut q = query.to_string();
    for domain in allowed {
        let _ = write!(q, " site:{domain}");
    }
    for domain in blocked {
        let _ = write!(q, " -site:{domain}");
    }
    q
}

/// Resolve the Brave API key: config value, then env var fallback.
fn resolve_brave_api_key(config: &WebConfig) -> Option<String> {
    if !config.brave_api_key.is_empty() {
        return Some(config.brave_api_key.clone());
    }
    std::env::var("CORTEX_BRAVE_API_KEY").ok()
}

fn execute_brave(
    query: &str,
    allowed: &[String],
    blocked: &[String],
    config: &WebConfig,
    count: usize,
) -> Result<ToolResult, ToolError> {
    let Some(api_key) = resolve_brave_api_key(config) else {
        return Ok(ToolResult::error(
            "Brave Search API key not configured. Set [web] brave_api_key in config.toml or CORTEX_BRAVE_API_KEY environment variable.",
        ));
    };

    let final_query = build_brave_query(query, allowed, blocked);

    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        urlencoded(&final_query),
        count
    );

    // Execute async HTTP in a new tokio runtime (Tool::execute is sync)
    let result = block_on_tool_future(async {
        let client = reqwest::Client::new();
        let resp = client
            .get(&url)
            .header("Accept", "application/json")
            .header("X-Subscription-Token", &api_key)
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("brave API request failed: {e}")))?;

        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("failed to parse response: {e}")))?;

        Ok::<serde_json::Value, ToolError>(json)
    })?;

    Ok(format_brave_results(&result))
}

fn format_brave_results(json: &serde_json::Value) -> ToolResult {
    let results = json
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array());

    match results {
        Some(arr) if !arr.is_empty() => {
            let mut output = String::new();
            for (i, item) in arr.iter().enumerate() {
                let title = item.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
                let desc = item
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                {
                    use std::fmt::Write;
                    let _ = write!(output, "{}. {}\n   {}\n   {}\n\n", i + 1, title, url, desc);
                }
            }
            ToolResult::success(output.trim_end())
        }
        _ => ToolResult::success("No results found."),
    }
}

fn execute_llm_search(query: &str, allowed: &[String], blocked: &[String]) -> ToolResult {
    use std::fmt::Write;
    let mut message = format!(
        "Web search request: \"{query}\"\n\nPlease search the web for this query and provide relevant, up-to-date results with titles, URLs, and brief descriptions."
    );

    if !allowed.is_empty() {
        let _ = write!(
            message,
            "\n\nRestrict results to these domains: {}",
            allowed.join(", ")
        );
    }
    if !blocked.is_empty() {
        let _ = write!(
            message,
            "\n\nExclude results from these domains: {}",
            blocked.join(", ")
        );
    }

    ToolResult::success(message)
}

/// Minimal URL encoding for query parameters.
fn urlencoded(s: &str) -> String {
    let mut result = String::with_capacity(s.len() * 2);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                result.push(b as char);
            }
            b' ' => result.push('+'),
            _ => {
                use std::fmt::Write;
                let _ = write!(result, "%{b:02X}");
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> WebConfig {
        WebConfig {
            search_backend: "llm".into(),
            ..WebConfig::default()
        }
    }

    fn brave_config() -> WebConfig {
        WebConfig {
            search_backend: "brave".into(),
            brave_api_key: "test-key".into(),
            brave_max_results: 5,
            ..WebConfig::default()
        }
    }

    // ── Schema & Name ──

    #[test]
    fn web_search_name() {
        let tool = WebSearchTool::new(default_config());
        assert_eq!(tool.name(), "web_search");
    }

    #[test]
    fn web_search_schema_has_required_query() {
        let tool = WebSearchTool::new(default_config());
        let schema = tool.input_schema();
        let required = schema.get("required").unwrap().as_array().unwrap();
        assert!(required.iter().any(|v| v.as_str() == Some("query")));
    }

    #[test]
    fn web_search_schema_has_domain_properties() {
        let tool = WebSearchTool::new(default_config());
        let schema = tool.input_schema();
        let props = schema.get("properties").unwrap();
        assert!(props.get("allowed_domains").is_some());
        assert!(props.get("blocked_domains").is_some());
    }

    // ── Query Validation ──

    #[test]
    fn query_empty_rejected() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({"query": ""}));
        assert!(result.is_err());
    }

    #[test]
    fn query_single_char_rejected() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({"query": "a"}));
        assert!(result.is_err());
    }

    #[test]
    fn query_missing_rejected() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({}));
        assert!(result.is_err());
    }

    #[test]
    fn query_valid_accepted() {
        let tool = WebSearchTool::new(default_config());
        // LLM backend doesn't need network, so this should succeed
        let result = tool.execute(serde_json::json!({"query": "rust async"}));
        assert!(result.is_ok());
        assert!(!result.unwrap().is_error);
    }

    // ── Domain Filter Mutual Exclusion ──

    #[test]
    fn both_domain_filters_rejected() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({
            "query": "test",
            "allowed_domains": ["a.com"],
            "blocked_domains": ["b.com"]
        }));
        assert!(result.is_err());
    }

    #[test]
    fn only_allowed_domains_accepted() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({
            "query": "test",
            "allowed_domains": ["docs.rs"]
        }));
        assert!(result.is_ok());
    }

    #[test]
    fn only_blocked_domains_accepted() {
        let tool = WebSearchTool::new(default_config());
        let result = tool.execute(serde_json::json!({
            "query": "test",
            "blocked_domains": ["pinterest.com"]
        }));
        assert!(result.is_ok());
    }

    // ── Brave Query Construction ──

    #[test]
    fn brave_query_with_allowed_domains() {
        let q = build_brave_query("async", &["docs.rs".into(), "crates.io".into()], &[]);
        assert_eq!(q, "async site:docs.rs site:crates.io");
    }

    #[test]
    fn brave_query_with_blocked_domains() {
        let q = build_brave_query("async", &[], &["pinterest.com".into()]);
        assert_eq!(q, "async -site:pinterest.com");
    }

    #[test]
    fn brave_query_plain() {
        let q = build_brave_query("rust async", &[], &[]);
        assert_eq!(q, "rust async");
    }

    // ── Brave Response Parsing ──

    #[test]
    fn format_brave_results_valid() {
        let json = serde_json::json!({
            "web": {
                "results": [
                    {"title": "Async Rust", "url": "https://docs.rs/async", "description": "Guide to async"},
                    {"title": "Tokio", "url": "https://tokio.rs", "description": "Async runtime"}
                ]
            }
        });
        let result = format_brave_results(&json);
        assert!(!result.is_error);
        assert!(result.output.contains("1. Async Rust"));
        assert!(result.output.contains("2. Tokio"));
        assert!(result.output.contains("https://docs.rs/async"));
        assert!(result.output.contains("Guide to async"));
    }

    #[test]
    fn format_brave_results_empty() {
        let json = serde_json::json!({
            "web": {"results": []}
        });
        let result = format_brave_results(&json);
        assert!(!result.is_error);
        assert!(result.output.contains("No results found"));
    }

    #[test]
    fn format_brave_results_missing_web() {
        let json = serde_json::json!({});
        let result = format_brave_results(&json);
        assert!(result.output.contains("No results found"));
    }

    // ── API Key Resolution ──

    #[test]
    fn api_key_from_config() {
        let config = brave_config();
        assert_eq!(resolve_brave_api_key(&config), Some("test-key".into()));
    }

    #[test]
    fn api_key_from_env() {
        let config = WebConfig {
            brave_api_key: String::new(),
            ..default_config()
        };
        // Set env var for test
        unsafe { std::env::set_var("CORTEX_BRAVE_API_KEY", "env-key") };
        let key = resolve_brave_api_key(&config);
        unsafe { std::env::remove_var("CORTEX_BRAVE_API_KEY") };
        assert_eq!(key, Some("env-key".into()));
    }

    #[test]
    fn api_key_missing() {
        // Ensure env var is not set
        unsafe { std::env::remove_var("CORTEX_BRAVE_API_KEY") };
        let config = WebConfig {
            brave_api_key: String::new(),
            ..default_config()
        };
        assert_eq!(resolve_brave_api_key(&config), None);
    }

    // ── Backend Selection ──

    #[test]
    fn llm_backend_selected() {
        let config = WebConfig {
            search_backend: "llm".into(),
            ..default_config()
        };
        let tool = WebSearchTool::new(config);
        let result = tool.execute(serde_json::json!({"query": "rust async"}));
        assert!(result.is_ok());
        let output = result.unwrap().output;
        assert!(output.contains("Web search request"));
    }

    #[test]
    fn brave_backend_missing_key() {
        unsafe { std::env::remove_var("CORTEX_BRAVE_API_KEY") };
        let config = WebConfig {
            search_backend: "brave".into(),
            brave_api_key: String::new(),
            ..default_config()
        };
        let tool = WebSearchTool::new(config);
        let result = tool.execute(serde_json::json!({"query": "rust async"}));
        assert!(result.is_ok());
        let r = result.unwrap();
        assert!(r.is_error);
        assert!(r.output.contains("API key not configured"));
    }

    #[test]
    fn llm_backend_with_allowed_domains() {
        let config = WebConfig {
            search_backend: "llm".into(),
            ..default_config()
        };
        let tool = WebSearchTool::new(config);
        let result = tool.execute(serde_json::json!({
            "query": "rust async",
            "allowed_domains": ["docs.rs"]
        }));
        assert!(result.is_ok());
        let output = result.unwrap().output;
        assert!(output.contains("docs.rs"));
    }

    // ── URL Encoding ──

    #[test]
    fn urlencoded_spaces() {
        assert_eq!(urlencoded("hello world"), "hello+world");
    }

    #[test]
    fn urlencoded_special_chars() {
        assert_eq!(urlencoded("a&b=c"), "a%26b%3Dc");
    }

    #[test]
    fn urlencoded_alphanumeric() {
        assert_eq!(urlencoded("abc123"), "abc123");
    }
}
