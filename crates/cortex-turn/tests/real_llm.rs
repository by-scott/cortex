#![cfg(feature = "real-llm")]
//! Real LLM integration tests.
//!
//! These tests call actual LLM APIs and require:
//! - `CORTEX_REAL_API_KEY` environment variable set
//! - Network access to the API endpoint
//!
//! Run with: `cargo test -p cortex-turn --test real_llm -- --ignored`

use cortex_turn::llm::LlmRequest;
use cortex_turn::llm::anthropic::AnthropicClient;
use cortex_turn::llm::client::LlmClient;
use cortex_types::Message;

fn api_key() -> String {
    std::env::var("CORTEX_REAL_API_KEY").unwrap_or_default()
}

fn base_url() -> String {
    std::env::var("CORTEX_REAL_BASE_URL")
        .unwrap_or_else(|_| "https://open.bigmodel.cn/api/paas".into())
}

fn model() -> String {
    std::env::var("CORTEX_REAL_MODEL").unwrap_or_else(|_| "glm-4-plus".into())
}

fn skip_if_no_key() -> bool {
    api_key().is_empty()
}

fn real_client() -> AnthropicClient {
    AnthropicClient::new(&base_url(), &api_key(), &model(), 0, "", 24)
}

#[tokio::test]
async fn simple_text_completion() {
    if skip_if_no_key() {
        eprintln!("Skipping: CORTEX_REAL_API_KEY not set");
        return;
    }

    let client = real_client();
    let messages = [Message::user("What is 2+2? Answer in one word.")];
    let request = LlmRequest {
        system: Some("You are a helpful assistant. Be concise."),
        messages: &messages,
        tools: None,
        max_tokens: 50,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    let response = client.complete(request).await;
    assert!(response.is_ok(), "LLM call failed: {response:?}");
    let resp = response.unwrap();
    assert!(resp.text.is_some(), "No text in response");
    let text = resp.text.unwrap();
    assert!(!text.is_empty(), "Empty response text");
    eprintln!("Response: {text}");
}

#[tokio::test]
async fn streaming_text() {
    if skip_if_no_key() {
        return;
    }

    let client = real_client();
    let streamed = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let streamed_clone = streamed.clone();
    let on_text = move |text: &str| {
        streamed_clone.lock().unwrap().push_str(text);
    };

    let messages = [Message::user("Count from 1 to 5.")];
    let request = LlmRequest {
        system: None,
        messages: &messages,
        tools: None,
        max_tokens: 100,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: Some(&on_text),
    };

    let response = client.complete(request).await.unwrap();
    let final_text = response.text.unwrap_or_default();
    let streamed_text = streamed.lock().unwrap().clone();

    assert!(!streamed_text.is_empty(), "No streaming output received");
    eprintln!("Streamed: {streamed_text}");
    eprintln!("Final: {final_text}");
}

#[tokio::test]
async fn tool_use_response() {
    if skip_if_no_key() {
        return;
    }

    let client = real_client();
    let tools = vec![serde_json::json!({
        "name": "get_weather",
        "description": "Get weather for a city",
        "input_schema": {
            "type": "object",
            "properties": {
                "city": { "type": "string" }
            },
            "required": ["city"]
        }
    })];

    let messages = [Message::user("What's the weather in Tokyo?")];
    let request = LlmRequest {
        system: Some("Use the get_weather tool to answer weather questions."),
        messages: &messages,
        tools: Some(&tools),
        max_tokens: 200,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    let response = client.complete(request).await;
    assert!(response.is_ok(), "LLM call failed: {response:?}");
    let resp = response.unwrap();
    eprintln!("Tool calls: {:?}", resp.tool_calls);
    eprintln!("Text: {:?}", resp.text);
    // Model may or may not use the tool — both are valid responses
}

#[tokio::test]
async fn multi_turn_conversation() {
    if skip_if_no_key() {
        return;
    }

    let client = real_client();

    // Turn 1
    let messages1 = [Message::user("My name is Alice.")];
    let req1 = LlmRequest {
        system: Some("Remember what the user tells you."),
        messages: &messages1,
        tools: None,
        max_tokens: 100,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };
    let resp1 = client.complete(req1).await.unwrap();
    let text1 = resp1.text.unwrap_or_default();
    eprintln!("Turn 1: {text1}");

    // Turn 2 — should remember the name
    let messages2 = [
        Message::user("My name is Alice."),
        Message::assistant(&text1),
        Message::user("What is my name?"),
    ];
    let req2 = LlmRequest {
        system: Some("Remember what the user tells you."),
        messages: &messages2,
        tools: None,
        max_tokens: 100,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };
    let resp2 = client.complete(req2).await.unwrap();
    let text2 = resp2.text.unwrap_or_default();
    eprintln!("Turn 2: {text2}");
    assert!(
        text2.to_lowercase().contains("alice"),
        "Model forgot the name: {text2}"
    );
}

#[tokio::test]
async fn token_usage_tracking() {
    if skip_if_no_key() {
        return;
    }

    let client = real_client();
    let messages = [Message::user("Hi")];
    let request = LlmRequest {
        system: None,
        messages: &messages,
        tools: None,
        max_tokens: 50,
        transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        on_text: None,
    };

    let resp = client.complete(request).await.unwrap();
    eprintln!(
        "Usage: input={}, output={}",
        resp.usage.input_tokens, resp.usage.output_tokens
    );
    // At minimum, there should be some tokens used
    assert!(
        resp.usage.input_tokens > 0 || resp.usage.output_tokens > 0,
        "No token usage reported"
    );
}
