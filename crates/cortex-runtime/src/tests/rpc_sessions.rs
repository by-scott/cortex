use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
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
