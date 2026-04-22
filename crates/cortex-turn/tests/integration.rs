//! End-to-end integration tests for the Turn lifecycle.
//!
//! These tests use `MockLlmClient` to simulate LLM responses and verify
//! the complete SN → TPN → DMN pipeline.

use cortex_kernel::Journal;
use cortex_turn::llm::{LlmResponse, LlmToolCall, MockLlmClient, Usage};
use cortex_turn::orchestrator::{NullTracer, TurnConfig, TurnContext};
use cortex_turn::risk::AutoApproveGate;
use cortex_turn::tools::{self, ToolRegistry};
use cortex_types::{CorrelationId, Event, Payload, TurnId, TurnState};

fn setup() -> (Journal, ToolRegistry) {
    let journal = Journal::open_in_memory().unwrap();
    let mut tools = ToolRegistry::new();
    tools::register_core_tools_basic(&mut tools);
    (journal, tools)
}

fn default_config() -> TurnConfig {
    TurnConfig {
        system_prompt: Some("You are a helpful assistant.".into()),
        max_tokens: 1024,
        agent_depth: 0,
        working_memory_capacity: 5,
        max_tool_iterations: 10,
        auto_extract: false,
        extract_min_turns: 5,
        turns_since_extract: 0,
        tool_timeout_secs: 30,
        llm_transient_retries: cortex_types::config::DEFAULT_LLM_TRANSIENT_RETRIES,
        strip_think_tags: true,
        evolution_weights: [1.0, 0.8, 0.6, 0.5, 0.4, 0.3],
        pressure_thresholds: [0.60, 0.75, 0.85, 0.95],
        metacognition: cortex_types::config::MetacognitionConfig::default(),
        trace: cortex_types::config::TurnTraceConfig::default(),
        session_id: None,
        actor: None,
        source: None,
        execution_scope: cortex_sdk::ExecutionScope::Foreground,
    }
}

// ── Basic Turn lifecycle ───────────────────────────────────────

#[tokio::test]
async fn pure_text_turn() {
    let (journal, tools) = setup();
    let mock = MockLlmClient::new();
    mock.push_text("Hello! I'm Cortex.");

    let gate = AutoApproveGate;
    let mut history = Vec::new();
    let mut cache = cortex_turn::context::SummaryCache::new();

    let ctx = TurnContext {
        input: "Hi there",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };

    let result = cortex_turn::orchestrator::run_turn(ctx).await;
    assert!(result.is_ok());
    let turn = result.unwrap();
    assert_eq!(turn.state, TurnState::Completed);
    assert!(turn.response_text.is_some());
    assert!(turn.response_text.unwrap().contains("Cortex"));
    assert!(!turn.events.is_empty());
}

#[tokio::test]
async fn turn_with_tool_call() {
    let (journal, tools) = setup();
    let mock = MockLlmClient::new();

    // First response: tool call
    mock.push_response(LlmResponse {
        text: None,
        tool_calls: vec![LlmToolCall {
            id: "t1".into(),
            name: "read".into(),
            input: serde_json::json!({"file_path": "/dev/null"}),
        }],
        usage: Usage {
            input_tokens: 10,
            output_tokens: 5,
        },
        model: "mock".into(),
    });
    // Second response: text after tool result
    mock.push_text("The file is empty.");

    let gate = AutoApproveGate;
    let mut history = Vec::new();
    let mut cache = cortex_turn::context::SummaryCache::new();

    let ctx = TurnContext {
        input: "Read /dev/null",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };

    let result = cortex_turn::orchestrator::run_turn(ctx).await;
    assert!(result.is_ok());
    let turn = result.unwrap();
    assert_eq!(turn.state, TurnState::Completed);
    // History should have: user + assistant(tool_use) + user(tool_result) + assistant(text)
    assert!(history.len() >= 3);
}

#[tokio::test]
async fn turn_tool_permission_denied() {
    let (journal, tools) = setup();
    let mock = MockLlmClient::new();

    // Tool call for dangerous operation
    mock.push_response(LlmResponse {
        text: None,
        tool_calls: vec![LlmToolCall {
            id: "t1".into(),
            name: "bash".into(),
            input: serde_json::json!({"command": "rm -rf /"}),
        }],
        usage: Usage::default(),
        model: "mock".into(),
    });
    // After denial, LLM responds with text
    mock.push_text("I cannot execute that command.");

    // Use default gate (not auto-approve) — bash rm -rf should be blocked
    let gate = cortex_turn::risk::DefaultPermissionGate;
    let mut history = Vec::new();
    let mut cache = cortex_turn::context::SummaryCache::new();

    let ctx = TurnContext {
        input: "Delete everything",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };

    let result = cortex_turn::orchestrator::run_turn(ctx).await;
    assert!(result.is_ok());
    let turn = result.unwrap();
    // Should have permission-related events
    let has_permission_event = turn.events.iter().any(|e| {
        matches!(
            e,
            Payload::PermissionDenied { .. } | Payload::PermissionRequested { .. }
        )
    });
    assert!(has_permission_event);
}

#[tokio::test]
async fn multi_turn_conversation() {
    let (journal, tools) = setup();
    let mock = MockLlmClient::new();
    mock.push_text("I'm ready to help.");
    mock.push_text("Sure, 2 + 2 = 4.");

    let gate = AutoApproveGate;
    let mut history = Vec::new();

    // Turn 1
    let mut cache1 = cortex_turn::context::SummaryCache::new();
    let ctx1 = TurnContext {
        input: "Hello",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache1),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };
    let r1 = cortex_turn::orchestrator::run_turn(ctx1).await.unwrap();
    assert_eq!(r1.state, TurnState::Completed);

    // Turn 2 — history should carry over
    let mut cache2 = cortex_turn::context::SummaryCache::new();
    let ctx2 = TurnContext {
        input: "What is 2+2?",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache2),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };
    let r2 = cortex_turn::orchestrator::run_turn(ctx2).await.unwrap();
    assert_eq!(r2.state, TurnState::Completed);

    // History should have messages from both turns
    assert!(history.len() >= 4); // user1 + asst1 + user2 + asst2
}

// ── Journal event verification ─────────────────────────────────

#[tokio::test]
async fn journal_records_turn_events() {
    let (journal, tools) = setup();
    let mock = MockLlmClient::new();
    mock.push_text("Done.");

    let gate = AutoApproveGate;
    let mut history = Vec::new();
    let mut cache = cortex_turn::context::SummaryCache::new();

    let ctx = TurnContext {
        input: "test",
        history: &mut history,
        llm: &mock,
        vision_llm: None,
        tools: &tools,
        journal: &journal,
        gate: &gate as &dyn cortex_turn::risk::PermissionGate,
        config: &default_config(),
        on_event: None,
        images: Vec::new(),
        compress_template: None,
        summary_cache: Some(&mut cache),
        prompt_manager: None,
        skill_registry: None,
        post_turn_llm: None,
        tracer: &NullTracer,
        control: None,
        on_tpn_complete: None,
    };

    cortex_turn::orchestrator::run_turn(ctx).await.unwrap();

    let events = journal.recent_events(20).unwrap();
    assert!(!events.is_empty());

    // Should have TurnStarted
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.payload, Payload::TurnStarted))
    );
    // Should have TurnCompleted
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.payload, Payload::TurnCompleted))
    );
    // Should have UserMessage
    assert!(
        events
            .iter()
            .any(|e| matches!(&e.payload, Payload::UserMessage { .. }))
    );
}

// ── Memory recall integration ──────────────────────────────────

#[test]
fn memory_recall_bm25_ranking() {
    use cortex_turn::memory::recall::rank_memories;
    use cortex_types::{MemoryEntry, MemoryKind, MemoryType};

    let entries = vec![
        MemoryEntry::new(
            "rust programming language",
            "desc",
            MemoryType::Project,
            MemoryKind::Semantic,
        ),
        MemoryEntry::new(
            "python data science",
            "desc",
            MemoryType::Project,
            MemoryKind::Semantic,
        ),
        MemoryEntry::new(
            "rust compiler errors",
            "desc",
            MemoryType::Feedback,
            MemoryKind::Episodic,
        ),
    ];

    let ranked = rank_memories("rust", &entries, 2);
    assert_eq!(ranked.len(), 2);
    // Rust entries should rank higher than python
    assert!(ranked[0].content.contains("rust"));
}

// ── Session persistence ────────────────────────────────────────

#[test]
fn session_persist_and_restore() {
    use cortex_kernel::{SessionStore, project_message_history};
    use cortex_types::{SessionId, SessionMetadata};

    let dir = tempfile::tempdir().unwrap();
    let store = SessionStore::open(dir.path()).unwrap();
    let journal = Journal::open_in_memory().unwrap();

    // Create session and record events
    let sid = SessionId::new();
    let meta = SessionMetadata::new(sid, 0);
    store.save(&meta).unwrap();

    let tid = TurnId::new();
    let cid = CorrelationId::new();
    journal
        .append(&Event::new(
            tid,
            cid,
            Payload::UserMessage {
                content: "hello".into(),
            },
        ))
        .unwrap();
    journal
        .append(&Event::new(
            tid,
            cid,
            Payload::AssistantMessage {
                content: "hi".into(),
            },
        ))
        .unwrap();

    // Restore
    let loaded = store.load(&sid).unwrap();
    assert_eq!(loaded.id, sid);

    let events = journal.recent_events(10).unwrap();
    let history = project_message_history(&events);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].text_content(), "hello");
}
