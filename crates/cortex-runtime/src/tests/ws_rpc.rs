use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_turn::skills::{Skill, SkillContent};
use cortex_types::{ExecutionMode, SkillMetadata};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::net::TcpListener;
use tokio_tungstenite::{connect_async, tungstenite::Message};

use crate::daemon::{DaemonServer, DaemonState};
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

async fn build_ws_rpc_server(
    ws_actor: &str,
) -> (
    tempfile::TempDir,
    Arc<DaemonState>,
    tokio::task::JoinHandle<()>,
    String,
) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    bindings.set_transport_actor("ws", ws_actor);

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = Arc::new(must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    ));
    let router = DaemonServer::build_http_router_for_tests(&state);

    let listener = must(
        TcpListener::bind("127.0.0.1:0").await,
        "listener should bind",
    );
    let addr = must(listener.local_addr(), "listener should expose local addr");
    let join = tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });

    (temp, state, join, format!("ws://{addr}/api/ws"))
}

fn parse_json(text: &str) -> Value {
    match serde_json::from_str(text) {
        Ok(value) => value,
        Err(err) => panic!("response should decode as JSON: {err}; text={text:?}"),
    }
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

async fn ws_request(url: &str, request: &str) -> Value {
    let (mut socket, _response) = must(connect_async(url).await, "websocket should connect");
    must(
        socket.send(Message::Text(request.to_string().into())).await,
        "request should send",
    );

    let message = loop {
        let Some(frame) = socket.next().await else {
            panic!("websocket should return a response frame");
        };
        let frame = must(frame, "websocket frame should decode");
        match frame {
            Message::Text(text) => break text,
            Message::Binary(_) | Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => {}
            Message::Close(frame) => panic!("websocket closed before text response: {frame:?}"),
        }
    };

    parse_json(&message)
}

#[tokio::test]
async fn ws_sync_rpc_rejects_hidden_session_ids() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"session/get","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;

    assert!(
        payload.get("error").is_some(),
        "ws sync rpc should reject hidden sessions: {payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_session_routes_stay_actor_scoped() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let new_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":1,"method":"session/new","params":{}}"#,
    )
    .await;
    let session_id = new_payload
        .get("result")
        .and_then(|value| value.get("session_id"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("session/new should return session_id: {new_payload:?}"));
    let own_session = state
        .visible_sessions("user:scott")
        .into_iter()
        .find(|session| session.id.to_string() == session_id)
        .unwrap_or_else(|| panic!("new ws session should be visible to actor"));
    assert_eq!(own_session.owner_actor, "user:scott");

    let list_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":2,"method":"session/list","params":{}}"#,
    )
    .await;
    let sessions = list_payload
        .get("result")
        .and_then(|value| value.get("sessions"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    assert_eq!(sessions.len(), 1);

    let get_own_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/get","params":{{"session_id":"{session_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        get_own_payload.get("result").is_some(),
        "own ws session/get should succeed: {get_own_payload:?}"
    );

    let get_hidden_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":4,"method":"session/get","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;
    assert!(
        get_hidden_payload.get("error").is_some(),
        "hidden ws session/get should be rejected: {get_hidden_payload:?}"
    );

    let end_own_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"session/end","params":{{"session_id":"{session_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        end_own_payload.get("result").is_some(),
        "own ws session/end should succeed: {end_own_payload:?}"
    );

    let end_hidden_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":6,"method":"session/end","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;
    assert!(
        end_hidden_payload.get("error").is_some(),
        "hidden ws session/end should be rejected: {end_hidden_payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_memory_list_is_actor_scoped() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible WS note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "Bob-hidden WS note",
        "other",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    other.owner_actor = "user:bob".to_string();
    must(state.memory_store().save(&own), "own memory should save");
    must(
        state.memory_store().save(&other),
        "other memory should save",
    );

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":2,"method":"memory/list","params":{}}"#,
    )
    .await;

    let memories = payload
        .get("result")
        .and_then(|value| value.get("memories"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("memory/list should return memories: {payload:?}"));

    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/list item should contain content")),
        "Scott-visible WS note"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_memory_save_assigns_transport_actor_owner() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":21,"method":"memory/save","params":{"content":"Scott-only WS note","description":"ws note","type":"Project"}}"#,
    )
    .await;
    let id = payload
        .get("result")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("memory/save should return id: {payload:?}"));

    let saved = must(
        state.memory_store().load_for_actor(id, "user:scott"),
        "saved memory should be visible to the ws actor",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.description, "ws note");

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_memory_get_and_delete_respect_actor_visibility() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible WS get/delete note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let own_id = own.id.clone();

    let mut other = cortex_types::MemoryEntry::new(
        "Bob-hidden WS get/delete note",
        "other",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    other.owner_actor = "user:bob".to_string();
    let other_id = other.id.clone();

    must(state.memory_store().save(&own), "own memory should save");
    must(
        state.memory_store().save(&other),
        "other memory should save",
    );

    let get_own = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":22,"method":"memory/get","params":{{"id":"{own_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        get_own.get("result").is_some(),
        "ws memory/get should return own memory: {get_own:?}"
    );

    let get_hidden = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":23,"method":"memory/get","params":{{"id":"{other_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        get_hidden.get("error").is_some(),
        "ws memory/get should reject hidden memory: {get_hidden:?}"
    );

    let delete_hidden = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":24,"method":"memory/delete","params":{{"id":"{other_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        delete_hidden.get("error").is_some(),
        "ws memory/delete should reject hidden memory: {delete_hidden:?}"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&other_id, "user:bob")
            .is_ok(),
        "hidden memory should remain after rejected delete"
    );

    let delete_own = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":25,"method":"memory/delete","params":{{"id":"{own_id}"}}}}"#
        ),
    )
    .await;
    assert!(
        delete_own.get("result").is_some(),
        "ws memory/delete should succeed for owned memory: {delete_own:?}"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&own_id, "user:scott")
            .is_err(),
        "deleted own memory should no longer be visible"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_memory_search_is_actor_scoped() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible WS search note",
        "own searchable note",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();

    let mut hidden = cortex_types::MemoryEntry::new(
        "Bob-hidden WS search note",
        "hidden searchable note",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    hidden.owner_actor = "user:bob".to_string();

    must(state.memory_store().save(&own), "own memory should save");
    must(
        state.memory_store().save(&hidden),
        "hidden memory should save",
    );

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":26,"method":"memory/search","params":{"query":"searchable","limit":10}}"#,
    )
    .await;
    let results = payload
        .get("result")
        .and_then(|value| value.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/search item should contain content")),
        "Scott-visible WS search note"
    );

    join.abort();
}

#[tokio::test]
async fn ws_streaming_prompt_rejects_hidden_session_ids() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":3,"method":"session/prompt","params":{{"session_id":"{bob_session}","prompt":"/status"}}}}"#
        ),
    )
    .await;

    assert_eq!(payload.get("event"), Some(&Value::from("error")));
    assert_eq!(
        payload
            .get("data")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str),
        Some("session not found or not accessible for this identity")
    );

    join.abort();
}

#[tokio::test]
async fn ws_streaming_prompt_without_session_id_reuses_ws_actor_session() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (session_id, _) = state.create_session_for_actor("user:scott");

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":4,"method":"session/prompt","params":{"prompt":"/status"}}"#,
    )
    .await;

    assert_eq!(payload.get("event"), Some(&Value::from("error")));
    assert_eq!(
        state.active_actor_session("user:scott").as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(state.visible_sessions("user:scott").len(), 1);

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_meta_alerts_and_command_dispatch_reject_hidden_sessions() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let meta_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"meta/alerts","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;
    assert!(
        meta_payload.get("error").is_some(),
        "ws meta/alerts should reject hidden sessions: {meta_payload:?}"
    );

    let command_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":6,"method":"command/dispatch","params":{{"session_id":"{bob_session}","command":"/status"}}}}"#
        ),
    )
    .await;
    assert!(
        command_payload.get("error").is_some(),
        "ws command/dispatch should reject hidden sessions: {command_payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_meta_alerts_and_command_dispatch_use_visible_sessions() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");

    let meta_payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":5,"method":"meta/alerts","params":{{"session_id":"{scott_session}"}}}}"#
        ),
    )
    .await;
    assert!(
        meta_payload.get("result").is_some(),
        "ws meta/alerts should succeed for visible sessions: {meta_payload:?}"
    );

    let command_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":6,"method":"command/dispatch","params":{"command":"/status"}}"#,
    )
    .await;
    assert!(
        command_payload.get("result").is_some(),
        "ws command/dispatch should succeed for visible actor sessions: {command_payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_sync_rpc_operator_methods_require_local_operator_identity() {
    let (_temp, _state, join, url) = build_ws_rpc_server("user:scott").await;

    let status_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":7,"method":"daemon/status","params":{}}"#,
    )
    .await;
    assert!(
        status_payload.get("error").is_some(),
        "ws daemon/status should reject non-local operators: {status_payload:?}"
    );

    let reload_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":8,"method":"admin/reload-config","params":{}}"#,
    )
    .await;
    assert!(
        reload_payload.get("error").is_some(),
        "ws admin/reload-config should reject non-local operators: {reload_payload:?}"
    );

    let health_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":9,"method":"health/check","params":{}}"#,
    )
    .await;
    assert!(
        health_payload.get("error").is_some(),
        "ws health/check should reject non-local operators: {health_payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_local_operator_methods_return_results() {
    let (_temp, _state, join, url) = build_ws_rpc_server("local:default").await;

    let status_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":7,"method":"daemon/status","params":{}}"#,
    )
    .await;
    assert!(
        status_payload.get("result").is_some(),
        "daemon/status should succeed for local operator: {status_payload:?}"
    );

    let reload_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":8,"method":"admin/reload-config","params":{}}"#,
    )
    .await;
    assert!(
        reload_payload.get("result").is_some(),
        "admin/reload-config should succeed for local operator: {reload_payload:?}"
    );

    let health_payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":9,"method":"health/check","params":{}}"#,
    )
    .await;
    assert!(
        health_payload.get("result").is_some(),
        "health/check should succeed for local operator: {health_payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_session_initialize_filters_local_operator_only_tools() {
    let (_temp, _state, join, url) = build_ws_rpc_server("user:scott").await;
    let user_init = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":10,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let user_tools = user_init
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !user_tools.iter().any(|value| value == forbidden),
            "non-local ws actor should not see {forbidden}: {user_tools:?}"
        );
    }
    join.abort();
}

#[tokio::test]
async fn ws_mcp_tools_list_filters_local_operator_only_tools() {
    let (_temp, _state, join, url) = build_ws_rpc_server("user:scott").await;
    let user_tools = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":11,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let user_tool_names = user_tools
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !user_tool_names.iter().any(|name| name == forbidden),
            "non-local ws actor should not see {forbidden} through mcp/tools-list: {user_tool_names:?}"
        );
    }
    join.abort();
}

#[tokio::test]
async fn ws_local_operator_keeps_introspection_tools_visible() {
    let (_temp, _state, join, url) = build_ws_rpc_server("local:default").await;

    let operator_init = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":17,"method":"session/initialize","params":{}}"#,
    )
    .await;
    let operator_tools = operator_init
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            operator_tools.iter().any(|value| value == expected),
            "local operator should keep {expected} through ws session/initialize: {operator_tools:?}"
        );
    }

    let operator_list = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":18,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    let operator_tool_names = operator_list
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            operator_tool_names.iter().any(|name| name == expected),
            "local operator should keep {expected} through ws mcp/tools-list: {operator_tool_names:?}"
        );
    }

    join.abort();
}

#[tokio::test]
async fn ws_mcp_tools_call_enforces_local_operator_only_introspection() {
    let (_temp, _state, join, url) = build_ws_rpc_server("user:scott").await;
    let user_call = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":19,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    assert!(
        user_call.get("error").is_some(),
        "non-local actor should not reach prompt_inspect through ws mcp/tools-call: {user_call:?}"
    );
    join.abort();

    let (_temp, _state, join, url) = build_ws_rpc_server("local:default").await;
    let operator_call = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":20,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    assert!(
        operator_call.get("result").is_some(),
        "local operator should reach prompt_inspect through ws mcp/tools-call: {operator_call:?}"
    );
    join.abort();
}

#[tokio::test]
async fn ws_filters_non_user_invocable_prompts() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let prompts = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":12,"method":"mcp/prompts-list","params":{}}"#,
    )
    .await;
    let prompt_names = prompts
        .get("result")
        .and_then(|value| value.get("prompts"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|prompt| {
            prompt
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(prompt_names.iter().any(|name| name == "visible-skill"));
    assert!(!prompt_names.iter().any(|name| name == "hidden-skill"));

    let hidden = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":13,"method":"mcp/prompts-get","params":{"name":"hidden-skill","arguments":""}}"#,
    )
    .await;
    assert!(
        hidden.get("error").is_some(),
        "hidden user skill should not be exposed through ws mcp/prompts-get: {hidden:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_filters_non_user_invocable_skills() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let list = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":14,"method":"skill/list","params":{}}"#,
    )
    .await;
    let skill_names = list
        .get("result")
        .and_then(|value| value.get("skills"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(skill_names.iter().any(|name| name == "visible-skill"));
    assert!(!skill_names.iter().any(|name| name == "hidden-skill"));

    let hidden_invoke = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":15,"method":"skill/invoke","params":{"name":"hidden-skill","args":""}}"#,
    )
    .await;
    assert!(
        hidden_invoke.get("error").is_some(),
        "hidden user skill should not be invocable through ws skill/invoke: {hidden_invoke:?}"
    );

    let suggestions = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":16,"method":"skill/suggestions","params":{"input":"hidden-skill visible-skill"}}"#,
    )
    .await;
    let suggestion_names = suggestions
        .get("result")
        .and_then(|value| value.get("suggestions"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|skill| {
            skill
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(suggestion_names.iter().any(|name| name == "visible-skill"));
    assert!(!suggestion_names.iter().any(|name| name == "hidden-skill"));

    join.abort();
}

#[tokio::test]
async fn ws_session_cancel_requests_active_visible_turn() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (_session_id, control) = state.register_active_turn_for_actor("user:scott");

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":17,"method":"session/cancel","params":{}}"#,
    )
    .await;

    assert_eq!(
        payload
            .get("result")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str),
        Some("Turn cancellation requested")
    );
    assert!(
        control.is_cancel_requested(),
        "ws session/cancel should request cancellation for the active visible turn"
    );

    join.abort();
}

#[tokio::test]
async fn ws_session_cancel_rejects_hidden_session_ids() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":18,"method":"session/cancel","params":{{"session_id":"{bob_session}"}}}}"#
        ),
    )
    .await;

    assert!(
        payload.get("error").is_some(),
        "ws session/cancel should reject hidden sessions: {payload:?}"
    );

    join.abort();
}

#[tokio::test]
async fn ws_command_dispatch_stop_requests_active_visible_turn() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (_session_id, control) = state.register_active_turn_for_actor("user:scott");

    let payload = ws_request(
        &url,
        r#"{"jsonrpc":"2.0","id":19,"method":"command/dispatch","params":{"command":"/stop"}}"#,
    )
    .await;

    assert_eq!(
        payload
            .get("result")
            .and_then(|value| value.get("output"))
            .and_then(Value::as_str),
        Some("Turn cancellation requested.")
    );
    assert!(
        control.is_cancel_requested(),
        "ws command/dispatch /stop should request cancellation"
    );

    join.abort();
}

#[tokio::test]
async fn ws_command_dispatch_stop_rejects_hidden_session_ids() {
    let (_temp, state, join, url) = build_ws_rpc_server("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let payload = ws_request(
        &url,
        &format!(
            r#"{{"jsonrpc":"2.0","id":20,"method":"command/dispatch","params":{{"session_id":"{bob_session}","command":"/stop"}}}}"#
        ),
    )
    .await;

    assert!(
        payload.get("error").is_some(),
        "ws command/dispatch /stop should reject hidden sessions: {payload:?}"
    );

    join.abort();
}
