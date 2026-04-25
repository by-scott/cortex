use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_turn::skills::{Skill, SkillContent};
use cortex_types::{ExecutionMode, SkillMetadata};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::daemon::{DaemonState, handle_line_protocol};
use crate::rpc::RpcHandler;
use crate::runtime::CortexRuntime;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn temp_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let base = temp.path().join("cortex-home");
    let home = base.join("default");
    (temp, base, home)
}

struct TestSkill {
    name: &'static str,
    user_invocable: bool,
}

impl Skill for TestSkill {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &'static str {
        "test skill"
    }

    fn when_to_use(&self) -> &'static str {
        "test"
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Inline
    }

    fn content(&self, _args: &str) -> SkillContent {
        SkillContent::Markdown(format!("content:{}", self.name))
    }

    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            user_invocable: self.user_invocable,
            agent_invocable: true,
            ..SkillMetadata::default()
        }
    }
}

async fn build_state_with_transport_actor(
    transport: &str,
    actor: &str,
) -> (tempfile::TempDir, Arc<DaemonState>) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    bindings.set_transport_actor(transport, actor);

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = Arc::new(must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    ));
    (temp, state)
}

async fn run_line_protocol_request(
    state: Arc<DaemonState>,
    source: &'static str,
    request: &str,
) -> String {
    let handler = RpcHandler::new(Arc::clone(&state));
    let (client, server) = tokio::io::duplex(4096);

    let join = tokio::spawn(async move {
        handle_line_protocol(server, &handler, &state, source).await;
    });

    let (read_half, mut write_half) = tokio::io::split(client);
    must(
        write_half.write_all(request.as_bytes()).await,
        "request should write",
    );
    must(write_half.write_all(b"\n").await, "newline should write");
    must(write_half.shutdown().await, "writer should shut down");

    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    must(reader.read_line(&mut line).await, "response should read");
    must(join.await, "line protocol task should join");
    line
}

async fn run_line_protocol_stream(
    state: Arc<DaemonState>,
    source: &'static str,
    request: &str,
) -> Vec<String> {
    let handler = RpcHandler::new(Arc::clone(&state));
    let (client, server) = tokio::io::duplex(4096);

    let join = tokio::spawn(async move {
        handle_line_protocol(server, &handler, &state, source).await;
    });

    let (read_half, mut write_half) = tokio::io::split(client);
    must(
        write_half.write_all(request.as_bytes()).await,
        "request should write",
    );
    must(write_half.write_all(b"\n").await, "newline should write");
    must(write_half.shutdown().await, "writer should shut down");

    let mut lines_reader = BufReader::new(read_half).lines();
    let mut lines = Vec::new();
    loop {
        match lines_reader.next_line().await {
            Ok(Some(line)) => lines.push(line),
            Ok(None) => break,
            Err(err) => panic!("response should read: {err}"),
        }
    }
    must(join.await, "line protocol task should join");
    lines
}

fn parse_json(line: &str) -> Value {
    match serde_json::from_str(line.trim()) {
        Ok(value) => value,
        Err(err) => panic!("response should decode as JSON: {err}; line={line:?}"),
    }
}

#[tokio::test]
async fn socket_line_protocol_uses_socket_actor_visibility_for_sync_rpc() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:bob").await;
    let (_bob_session, _) = state.create_session_for_actor("user:bob");
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"session/get","params":{{"session_id":"{scott_session}"}}}}"#
        ),
    )
    .await;
    let payload = parse_json(&line);
    assert!(
        payload.get("error").is_some(),
        "socket sync rpc should reject hidden sessions: {payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_batch_uses_socket_actor_visibility() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:bob").await;
    let (_bob_session, _) = state.create_session_for_actor("user:bob");
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let mut bob_memory = cortex_types::MemoryEntry::new(
        "Bob-visible socket note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    bob_memory.owner_actor = "user:bob".to_string();
    let mut scott_memory = cortex_types::MemoryEntry::new(
        "Scott-hidden socket note",
        "other",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    scott_memory.owner_actor = "user:scott".to_string();
    must(
        state.memory_store().save(&bob_memory),
        "own memory should save",
    );
    must(
        state.memory_store().save(&scott_memory),
        "other memory should save",
    );

    let line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"[{{"jsonrpc":"2.0","id":1,"method":"session/get","params":{{"session_id":"{scott_session}"}}}},{{"jsonrpc":"2.0","id":2,"method":"memory/list","params":{{}}}}]"#
        ),
    )
    .await;
    let payload = parse_json(&line);
    let items = payload
        .as_array()
        .unwrap_or_else(|| panic!("batch response should be an array: {payload:?}"));
    assert_eq!(items.len(), 2);
    assert!(
        items[0].get("error").is_some(),
        "hidden session/get should fail inside socket batch: {payload:?}"
    );
    let memories = items[1]
        .get("result")
        .and_then(|value| value.get("memories"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("memory/list should succeed inside batch: {payload:?}"));
    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/list item should contain content")),
        "Bob-visible socket note"
    );
}

#[tokio::test]
async fn socket_line_protocol_prompt_rejects_hidden_session_ids() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:bob").await;
    let (_bob_session, _) = state.create_session_for_actor("user:bob");
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let lines = run_line_protocol_stream(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"session/prompt","params":{{"session_id":"{scott_session}","prompt":"/status"}}}}"#
        ),
    )
    .await;
    assert_eq!(
        lines.len(),
        1,
        "hidden socket prompt should emit one error line"
    );

    let payload = parse_json(&lines[0]);
    assert_eq!(payload.get("event"), Some(&Value::from("error")));
    assert_eq!(
        payload
            .get("data")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str),
        Some("session not found or not accessible for this identity")
    );
}

#[tokio::test]
async fn socket_line_protocol_prompt_without_session_id_reuses_socket_actor_session() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    let (session_id, _) = state.create_session_for_actor("user:scott");

    let lines = run_line_protocol_stream(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":1,"method":"session/prompt","params":{"prompt":"/status"}}"#,
    )
    .await;
    assert_eq!(
        lines.len(),
        1,
        "test runtime should return a single terminal error line"
    );

    let payload = parse_json(&lines[0]);
    assert_eq!(payload.get("event"), Some(&Value::from("error")));
    assert_eq!(
        state.active_actor_session("user:scott").as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(state.visible_sessions("user:scott").len(), 1);
}

#[tokio::test]
async fn socket_line_protocol_meta_alerts_and_command_dispatch_reject_hidden_sessions() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let meta_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"meta/alerts","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;
    let meta_payload = parse_json(&meta_line);
    assert!(
        meta_payload.get("error").is_some(),
        "socket meta/alerts should reject hidden sessions: {meta_payload:?}"
    );

    let command_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"command/dispatch","params":{{"session_id":"{bob_session}","command":"/status"}}}}"#
        ),
    )
    .await;
    let command_payload = parse_json(&command_line);
    assert!(
        command_payload.get("error").is_some(),
        "socket command/dispatch should reject hidden sessions: {command_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_meta_alerts_and_command_dispatch_use_visible_sessions() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let meta_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        &format!(
            r#"{{"jsonrpc":"2.0","id":2,"method":"meta/alerts","params":{{"session_id":"{scott_session}"}}}}"#
        ),
    )
    .await;
    let meta_payload = parse_json(&meta_line);
    assert!(
        meta_payload.get("result").is_some(),
        "socket meta/alerts should succeed for visible sessions: {meta_payload:?}"
    );

    let command_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":3,"method":"command/dispatch","params":{"command":"/status"}}"#,
    )
    .await;
    let command_payload = parse_json(&command_line);
    assert!(
        command_payload.get("result").is_some(),
        "socket command/dispatch should succeed for visible actor sessions: {command_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_operator_methods_require_local_operator_identity() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;

    let status_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":4,"method":"daemon/status","params":{}}"#,
    )
    .await;
    let status_payload = parse_json(&status_line);
    assert!(
        status_payload.get("error").is_some(),
        "socket daemon/status should reject non-local operators: {status_payload:?}"
    );

    let reload_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":5,"method":"admin/reload-config","params":{}}"#,
    )
    .await;
    let reload_payload = parse_json(&reload_line);
    assert!(
        reload_payload.get("error").is_some(),
        "socket admin/reload-config should reject non-local operators: {reload_payload:?}"
    );

    let health_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":6,"method":"health/check","params":{}}"#,
    )
    .await;
    let health_payload = parse_json(&health_line);
    assert!(
        health_payload.get("error").is_some(),
        "socket health/check should reject non-local operators: {health_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_local_operator_methods_return_results() {
    let (_temp, state) = build_state_with_transport_actor("socket", "local:default").await;

    let status_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":16,"method":"daemon/status","params":{}}"#,
    )
    .await;
    let status_payload = parse_json(&status_line);
    assert!(
        status_payload.get("result").is_some(),
        "daemon/status should succeed for local operator: {status_payload:?}"
    );

    let health_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":17,"method":"health/check","params":{}}"#,
    )
    .await;
    let health_payload = parse_json(&health_line);
    assert!(
        health_payload.get("result").is_some(),
        "health/check should succeed for local operator: {health_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_filters_local_operator_only_tools() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;

    let init_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":7,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let init_payload = parse_json(&init_line);
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !init_tools.iter().any(|value| value == forbidden),
            "non-local socket actor should not see {forbidden}: {init_tools:?}"
        );
    }

    let tools_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":8,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let tools_payload = parse_json(&tools_line);
    let tool_names = tools_payload
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !tool_names.iter().any(|name| name == forbidden),
            "non-local socket actor should not see {forbidden} through mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn socket_line_protocol_local_operator_keeps_introspection_tools_visible() {
    let (_temp, state) = build_state_with_transport_actor("socket", "local:default").await;

    let init_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":14,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let init_payload = parse_json(&init_line);
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            init_tools.iter().any(|value| value == expected),
            "local operator should keep {expected} through socket session/initialize: {init_tools:?}"
        );
    }

    let tools_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":15,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let tools_payload = parse_json(&tools_line);
    let tool_names = tools_payload
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            tool_names.iter().any(|name| name == expected),
            "local operator should keep {expected} through socket mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn socket_line_protocol_mcp_tools_call_enforces_local_operator_only_introspection() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    let user_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":18,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    let user_payload = parse_json(&user_line);
    assert!(
        user_payload.get("error").is_some(),
        "non-local actor should not reach prompt_inspect through socket mcp/tools-call: {user_payload:?}"
    );

    let (_temp, state) = build_state_with_transport_actor("socket", "local:default").await;
    let operator_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":19,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    let operator_payload = parse_json(&operator_line);
    assert!(
        operator_payload.get("result").is_some(),
        "local operator should reach prompt_inspect through socket mcp/tools-call: {operator_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_filters_non_user_invocable_prompts() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let prompts_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":9,"method":"mcp/prompts-list","params":{}}"#,
    )
    .await;
    let prompts_payload = parse_json(&prompts_line);
    let prompt_names = prompts_payload
        .get("result")
        .and_then(|value| value.get("prompts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|prompt| {
            prompt
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(prompt_names.iter().any(|name| name == "visible-skill"));
    assert!(!prompt_names.iter().any(|name| name == "hidden-skill"));

    let hidden_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":10,"method":"mcp/prompts-get","params":{"name":"hidden-skill","arguments":""}}"#,
    )
    .await;
    let hidden_payload = parse_json(&hidden_line);
    assert!(
        hidden_payload.get("error").is_some(),
        "hidden user skill should not be exposed through socket mcp/prompts-get: {hidden_payload:?}"
    );
}

#[tokio::test]
async fn socket_line_protocol_filters_non_user_invocable_skills() {
    let (_temp, state) = build_state_with_transport_actor("socket", "user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let list_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":11,"method":"skill/list","params":{}}"#,
    )
    .await;
    let list_payload = parse_json(&list_line);
    let skill_names = list_payload
        .get("result")
        .and_then(|value| value.get("skills"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(skill_names.iter().any(|name| name == "visible-skill"));
    assert!(!skill_names.iter().any(|name| name == "hidden-skill"));

    let hidden_invoke = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":12,"method":"skill/invoke","params":{"name":"hidden-skill","args":""}}"#,
    )
    .await;
    let hidden_invoke_payload = parse_json(&hidden_invoke);
    assert!(
        hidden_invoke_payload.get("error").is_some(),
        "hidden user skill should not be invocable through socket skill/invoke: {hidden_invoke_payload:?}"
    );

    let suggestions_line = run_line_protocol_request(
        Arc::clone(&state),
        "socket",
        r#"{"jsonrpc":"2.0","id":13,"method":"skill/suggestions","params":{"input":"hidden-skill visible-skill"}}"#,
    )
    .await;
    let suggestions_payload = parse_json(&suggestions_line);
    let suggestion_names = suggestions_payload
        .get("result")
        .and_then(|value| value.get("suggestions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(suggestion_names.iter().any(|name| name == "visible-skill"));
    assert!(!suggestion_names.iter().any(|name| name == "hidden-skill"));
}

#[tokio::test]
async fn stdio_line_protocol_uses_stdio_actor_visibility_for_sync_rpc() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:bob").await;
    let (_bob_session, _) = state.create_session_for_actor("user:bob");
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        &format!(
            r#"{{"jsonrpc":"2.0","id":7,"method":"session/get","params":{{"session_id":"{scott_session}"}}}}"#
        ),
    )
    .await;
    let payload = parse_json(&line);
    assert!(
        payload.get("error").is_some(),
        "stdio sync rpc should reject hidden sessions: {payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_prompt_without_session_id_reuses_stdio_actor_session() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;
    let (session_id, _) = state.create_session_for_actor("user:scott");

    let lines = run_line_protocol_stream(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":8,"method":"session/prompt","params":{"prompt":"/status"}}"#,
    )
    .await;
    assert_eq!(
        lines.len(),
        1,
        "test runtime should return a single terminal error line"
    );

    let payload = parse_json(&lines[0]);
    assert_eq!(payload.get("event"), Some(&Value::from("error")));
    assert_eq!(
        state.active_actor_session("user:scott").as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(state.visible_sessions("user:scott").len(), 1);
}

#[tokio::test]
async fn stdio_line_protocol_operator_methods_require_local_operator_identity() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;

    let status_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":9,"method":"daemon/status","params":{}}"#,
    )
    .await;
    let status_payload = parse_json(&status_line);
    assert!(
        status_payload.get("error").is_some(),
        "stdio daemon/status should reject non-local operators: {status_payload:?}"
    );

    let reload_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":10,"method":"admin/reload-config","params":{}}"#,
    )
    .await;
    let reload_payload = parse_json(&reload_line);
    assert!(
        reload_payload.get("error").is_some(),
        "stdio admin/reload-config should reject non-local operators: {reload_payload:?}"
    );

    let health_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":11,"method":"health/check","params":{}}"#,
    )
    .await;
    let health_payload = parse_json(&health_line);
    assert!(
        health_payload.get("error").is_some(),
        "stdio health/check should reject non-local operators: {health_payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_meta_alerts_and_command_dispatch_use_visible_sessions() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let meta_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        &format!(
            r#"{{"jsonrpc":"2.0","id":12,"method":"meta/alerts","params":{{"session_id":"{scott_session}"}}}}"#
        ),
    )
    .await;
    let meta_payload = parse_json(&meta_line);
    assert!(
        meta_payload.get("result").is_some(),
        "stdio meta/alerts should succeed for visible sessions: {meta_payload:?}"
    );

    let command_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":13,"method":"command/dispatch","params":{"command":"/status"}}"#,
    )
    .await;
    let command_payload = parse_json(&command_line);
    assert!(
        command_payload.get("result").is_some(),
        "stdio command/dispatch should succeed for visible actor sessions: {command_payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_local_operator_methods_return_results() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "local:default").await;

    let status_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":21,"method":"daemon/status","params":{}}"#,
    )
    .await;
    let status_payload = parse_json(&status_line);
    assert!(
        status_payload.get("result").is_some(),
        "daemon/status should succeed for local operator: {status_payload:?}"
    );

    let health_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":22,"method":"health/check","params":{}}"#,
    )
    .await;
    let health_payload = parse_json(&health_line);
    assert!(
        health_payload.get("result").is_some(),
        "health/check should succeed for local operator: {health_payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_filters_local_operator_only_tools() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;

    let init_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":12,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let init_payload = parse_json(&init_line);
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !init_tools.iter().any(|value| value == forbidden),
            "non-local stdio actor should not see {forbidden}: {init_tools:?}"
        );
    }

    let tools_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":13,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let tools_payload = parse_json(&tools_line);
    let tool_names = tools_payload
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !tool_names.iter().any(|name| name == forbidden),
            "non-local stdio actor should not see {forbidden} through mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn stdio_line_protocol_local_operator_keeps_introspection_tools_visible() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "local:default").await;

    let init_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":19,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let init_payload = parse_json(&init_line);
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            init_tools.iter().any(|value| value == expected),
            "local operator should keep {expected} through stdio session/initialize: {init_tools:?}"
        );
    }

    let tools_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":20,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let tools_payload = parse_json(&tools_line);
    let tool_names = tools_payload
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str).map(str::to_string))
        .collect::<Vec<_>>();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            tool_names.iter().any(|name| name == expected),
            "local operator should keep {expected} through stdio mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn stdio_line_protocol_mcp_tools_call_enforces_local_operator_only_introspection() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;
    let user_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":23,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    let user_payload = parse_json(&user_line);
    assert!(
        user_payload.get("error").is_some(),
        "non-local actor should not reach prompt_inspect through stdio mcp/tools-call: {user_payload:?}"
    );

    let (_temp, state) = build_state_with_transport_actor("stdio", "local:default").await;
    let operator_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":24,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    let operator_payload = parse_json(&operator_line);
    assert!(
        operator_payload.get("result").is_some(),
        "local operator should reach prompt_inspect through stdio mcp/tools-call: {operator_payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_filters_non_user_invocable_prompts() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let prompts_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":14,"method":"mcp/prompts-list","params":{}}"#,
    )
    .await;
    let prompts_payload = parse_json(&prompts_line);
    let prompt_names = prompts_payload
        .get("result")
        .and_then(|value| value.get("prompts"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|prompt| {
            prompt
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(prompt_names.iter().any(|name| name == "visible-skill"));
    assert!(!prompt_names.iter().any(|name| name == "hidden-skill"));

    let hidden_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":15,"method":"mcp/prompts-get","params":{"name":"hidden-skill","arguments":""}}"#,
    )
    .await;
    let hidden_payload = parse_json(&hidden_line);
    assert!(
        hidden_payload.get("error").is_some(),
        "hidden user skill should not be exposed through stdio mcp/prompts-get: {hidden_payload:?}"
    );
}

#[tokio::test]
async fn stdio_line_protocol_filters_non_user_invocable_skills() {
    let (_temp, state) = build_state_with_transport_actor("stdio", "user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let list_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":16,"method":"skill/list","params":{}}"#,
    )
    .await;
    let list_payload = parse_json(&list_line);
    let skill_names = list_payload
        .get("result")
        .and_then(|value| value.get("skills"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(skill_names.iter().any(|name| name == "visible-skill"));
    assert!(!skill_names.iter().any(|name| name == "hidden-skill"));

    let hidden_invoke = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":17,"method":"skill/invoke","params":{"name":"hidden-skill","args":""}}"#,
    )
    .await;
    let hidden_invoke_payload = parse_json(&hidden_invoke);
    assert!(
        hidden_invoke_payload.get("error").is_some(),
        "hidden user skill should not be invocable through stdio skill/invoke: {hidden_invoke_payload:?}"
    );

    let suggestions_line = run_line_protocol_request(
        Arc::clone(&state),
        "stdio",
        r#"{"jsonrpc":"2.0","id":18,"method":"skill/suggestions","params":{"input":"hidden-skill visible-skill"}}"#,
    )
    .await;
    let suggestions_payload = parse_json(&suggestions_line);
    let suggestion_names = suggestions_payload
        .get("result")
        .and_then(|value| value.get("suggestions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(suggestion_names.iter().any(|name| name == "visible-skill"));
    assert!(!suggestion_names.iter().any(|name| name == "hidden-skill"));
}
