use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_turn::skills::{Skill, SkillContent};
use cortex_types::{ExecutionMode, SkillMetadata};
use serde_json::json;

use crate::daemon::DaemonState;
use crate::rpc::{RpcHandler, RpcRequest};
use crate::runtime::CortexRuntime;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn response_item_names(response: &crate::rpc::RpcResponse, field: &str) -> Vec<String> {
    response
        .result
        .as_ref()
        .and_then(|value| value.get(field))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
}

fn temp_paths() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let base = temp.path().join("cortex-home");
    let home = base.join("default");
    (temp, base, home)
}

async fn build_rpc_handler(rpc_actor: &str) -> (tempfile::TempDir, Arc<DaemonState>, RpcHandler) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    bindings.set_transport_actor("rpc", rpc_actor);

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = Arc::new(must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    ));
    let handler = RpcHandler::new(Arc::clone(&state));
    (temp, state, handler)
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

#[tokio::test(flavor = "multi_thread")]
async fn rpc_session_new_assigns_transport_actor_owner() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/new".to_string(),
        id: json!(1),
        params: json!({}),
    });

    let session_id = response
        .result
        .as_ref()
        .and_then(|value| value.get("session_id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("session/new should return session_id"));
    let session = state
        .visible_sessions("user:scott")
        .into_iter()
        .find(|session| session.id.to_string() == session_id)
        .unwrap_or_else(|| panic!("new rpc session should be visible to rpc actor"));
    assert_eq!(session.owner_actor, "user:scott");
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_session_list_and_get_are_filtered_to_transport_actor() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let list_response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/list".to_string(),
        id: json!(1),
        params: json!({}),
    });
    let sessions = list_response
        .result
        .as_ref()
        .and_then(|value| value.get("sessions"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("session/list should return a sessions array"));
    assert_eq!(sessions.len(), 1);

    let get_own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/get".to_string(),
        id: json!(2),
        params: json!({ "session_id": scott_session }),
    });
    assert!(get_own.result.is_some());

    let get_hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/get".to_string(),
        id: json!(3),
        params: json!({ "session_id": bob_session }),
    });
    assert!(get_hidden.error.is_some());

    let end_own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/end".to_string(),
        id: json!(4),
        params: json!({ "session_id": scott_session }),
    });
    assert!(end_own.result.is_some(), "own session should be endable");

    let end_hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/end".to_string(),
        id: json!(5),
        params: json!({ "session_id": bob_session }),
    });
    assert!(
        end_hidden.error.is_some(),
        "hidden session should not be endable"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_prompt_rejects_inaccessible_session_ids() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/prompt".to_string(),
        id: json!(6),
        params: json!({
            "session_id": bob_session,
            "prompt": "/status"
        }),
    });
    assert!(response.error.is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_prompt_without_session_id_reuses_rpc_actor_session() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (session_id, _) = state.create_session_for_actor("user:scott");

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/prompt".to_string(),
        id: json!(7),
        params: json!({
            "prompt": "/status"
        }),
    });

    assert!(
        response.error.is_some(),
        "session/prompt should fail without API key in test runtime"
    );
    assert_eq!(
        response.error.as_ref().map(|error| error.code),
        Some(1100),
        "session/prompt should fail at turn execution after session resolution: {response:?}"
    );
    assert_eq!(
        state.active_actor_session("user:scott").as_deref(),
        Some(session_id.as_str())
    );
    assert_eq!(state.visible_sessions("user:scott").len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_meta_alerts_respects_transport_actor_visibility() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "meta/alerts".to_string(),
        id: json!(8),
        params: json!({ "session_id": scott_session }),
    });
    let own_alerts = own
        .result
        .as_ref()
        .and_then(|value| value.get("alerts"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("meta/alerts should return alerts array: {own:?}"));
    assert!(own_alerts.is_empty());

    let hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "meta/alerts".to_string(),
        id: json!(9),
        params: json!({ "session_id": bob_session }),
    });
    assert!(hidden.error.is_some(), "hidden session should be rejected");
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_command_dispatch_rejects_hidden_session_ids() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "command/dispatch".to_string(),
        id: json!(10),
        params: json!({
            "session_id": bob_session,
            "command": "/status"
        }),
    });
    assert!(
        hidden.error.is_some(),
        "command dispatch should reject hidden sessions"
    );

    let own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "command/dispatch".to_string(),
        id: json!(11),
        params: json!({
            "command": "/status"
        }),
    });
    assert!(
        own.result.is_some(),
        "sessionless command dispatch should work"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_operator_methods_require_local_operator_identity() {
    let (_temp, _state, handler) = build_rpc_handler("user:scott").await;

    let status = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "daemon/status".to_string(),
        id: json!(12),
        params: json!({}),
    });
    assert!(
        status.error.is_some(),
        "daemon/status should reject non-local operators"
    );

    let reload = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "admin/reload-config".to_string(),
        id: json!(13),
        params: json!({}),
    });
    assert!(
        reload.error.is_some(),
        "admin/reload-config should reject non-local operators"
    );

    let health = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "health/check".to_string(),
        id: json!(14),
        params: json!({}),
    });
    assert!(
        health.error.is_some(),
        "health/check should reject non-local operators"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_local_operator_methods_return_results() {
    let (_temp, _state, handler) = build_rpc_handler("local:default").await;

    let status = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "daemon/status".to_string(),
        id: json!(14),
        params: json!({}),
    });
    assert!(
        status.result.is_some(),
        "daemon/status should succeed for local operator: {status:?}"
    );

    let reload = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "admin/reload-config".to_string(),
        id: json!(15),
        params: json!({}),
    });
    assert!(
        reload.result.is_some(),
        "admin/reload-config should succeed for local operator: {reload:?}"
    );

    let health = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "health/check".to_string(),
        id: json!(16),
        params: json!({}),
    });
    assert!(
        health.result.is_some(),
        "health/check should succeed for local operator: {health:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_session_initialize_filters_local_operator_only_tools() {
    let (_temp, _state, user_handler) = build_rpc_handler("user:scott").await;
    let user_init = user_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/initialize".to_string(),
        id: json!(15),
        params: json!({}),
    });
    let user_tools = user_init
        .result
        .as_ref()
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !user_tools.iter().any(|value| value == forbidden),
            "non-local actors should not see {forbidden}: {user_tools:?}"
        );
    }

    let (_temp, _state, operator_handler) = build_rpc_handler("local:default").await;
    let operator_init = operator_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/initialize".to_string(),
        id: json!(16),
        params: json!({}),
    });
    let operator_tools = operator_init
        .result
        .as_ref()
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            operator_tools.iter().any(|value| value == expected),
            "local operator should keep {expected}: {operator_tools:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_mcp_tools_list_filters_local_operator_only_tools() {
    let (_temp, _state, user_handler) = build_rpc_handler("user:scott").await;
    let user_tools = user_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/tools-list".to_string(),
        id: json!(17),
        params: json!({}),
    });
    let user_tool_names = user_tools
        .result
        .as_ref()
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
            "non-local actors should not see {forbidden} through MCP tools/list: {user_tool_names:?}"
        );
    }

    let (_temp, _state, operator_handler) = build_rpc_handler("local:default").await;
    let operator_tools = operator_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/tools-list".to_string(),
        id: json!(18),
        params: json!({}),
    });
    let operator_tool_names = operator_tools
        .result
        .as_ref()
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
            "local operator should keep {expected} visible through MCP tools/list: {operator_tool_names:?}"
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_mcp_tools_call_enforces_local_operator_only_introspection() {
    let (_temp, _state, user_handler) = build_rpc_handler("user:scott").await;
    let user_call = user_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/tools-call".to_string(),
        id: json!(18),
        params: json!({
            "name": "prompt_inspect",
            "arguments": { "layer": "soul" }
        }),
    });
    assert!(
        user_call.error.is_some(),
        "non-local actor should not reach prompt_inspect through mcp/tools-call: {user_call:?}"
    );

    let (_temp, _state, operator_handler) = build_rpc_handler("local:default").await;
    let operator_call = operator_handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/tools-call".to_string(),
        id: json!(19),
        params: json!({
            "name": "prompt_inspect",
            "arguments": { "layer": "soul" }
        }),
    });
    assert!(
        operator_call.result.is_some(),
        "local operator should reach prompt_inspect through mcp/tools-call: {operator_call:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_session_cancel_requests_active_visible_turn() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (_session_id, control) = state.register_active_turn_for_actor("user:scott");

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/cancel".to_string(),
        id: json!(23),
        params: json!({}),
    });

    assert_eq!(
        response
            .result
            .as_ref()
            .and_then(|value| value.get("message"))
            .and_then(serde_json::Value::as_str),
        Some("Turn cancellation requested")
    );
    assert!(
        control.is_cancel_requested(),
        "session/cancel should request cancellation for the active visible turn"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_session_cancel_rejects_hidden_session_ids() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "session/cancel".to_string(),
        id: json!(24),
        params: json!({ "session_id": bob_session }),
    });

    assert!(
        response.error.is_some(),
        "session/cancel should reject hidden sessions"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_skill_surfaces_hide_non_user_invocable_skills() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let list = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "skill/list".to_string(),
        id: json!(19),
        params: json!({}),
    });
    let listed_names = response_item_names(&list, "skills");
    assert!(listed_names.iter().any(|name| name == "visible-skill"));
    assert!(!listed_names.iter().any(|name| name == "hidden-skill"));

    let hidden_skill = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "skill/invoke".to_string(),
        id: json!(20),
        params: json!({ "name": "hidden-skill", "args": "" }),
    });
    assert!(
        hidden_skill.error.is_some(),
        "hidden user skill should not be invocable through skill/invoke"
    );

    let hidden_prompt = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/prompts-list".to_string(),
        id: json!(21),
        params: json!({}),
    });
    let hidden_prompt_names = response_item_names(&hidden_prompt, "prompts");
    assert!(
        hidden_prompt_names
            .iter()
            .any(|name| name == "visible-skill"),
        "visible user skill should remain available through mcp/prompts-list: {hidden_prompt_names:?}"
    );
    assert!(
        !hidden_prompt_names
            .iter()
            .any(|name| name == "hidden-skill"),
        "hidden user skill should stay out of mcp/prompts-list: {hidden_prompt_names:?}"
    );

    let hidden_prompt = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/prompts-get".to_string(),
        id: json!(22),
        params: json!({ "name": "hidden-skill", "arguments": "" }),
    });
    assert!(
        hidden_prompt.error.is_some(),
        "hidden user skill should not be exposed through mcp/prompts-get"
    );

    let visible_prompt = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "mcp/prompts-get".to_string(),
        id: json!(23),
        params: json!({ "name": "visible-skill", "arguments": "" }),
    });
    assert!(
        visible_prompt.result.is_some(),
        "visible user skill should remain available through mcp/prompts-get"
    );

    let suggestions = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "skill/suggestions".to_string(),
        id: json!(24),
        params: json!({ "input": "hidden-skill visible-skill" }),
    });
    let suggestion_names = response_item_names(&suggestions, "suggestions");
    assert!(
        suggestion_names.iter().any(|name| name == "visible-skill"),
        "visible skill should remain suggestible: {suggestion_names:?}"
    );
    assert!(
        !suggestion_names.iter().any(|name| name == "hidden-skill"),
        "hidden skill should stay out of suggestions: {suggestion_names:?}"
    );
}
