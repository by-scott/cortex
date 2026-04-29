use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_types::{Goal, GoalLevel, GoalStatus};
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

fn request(id: u64, method: &str, params: serde_json::Value) -> RpcRequest {
    RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        id: json!(id),
        params,
    }
}

fn create_goal(handler: &RpcHandler) -> String {
    let create = handler.handle(&request(
        1,
        "goal/create",
        json!({
            "description": "ship goal rpc",
            "success_criteria": "visible in actor-scoped state and active context",
            "level": "Tactical",
            "priority": 9,
            "evidence_refs": ["event:1"],
            "memory_refs": ["memory:1"]
        }),
    ));
    create
        .result
        .as_ref()
        .and_then(|value| value.get("goal"))
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("goal/create should return a goal id: {create:?}"))
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_goal_create_and_list_are_actor_scoped() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let goal_id = create_goal(&handler);

    let saved = must(
        state.goal_store().load_for_actor(&goal_id, "user:scott"),
        "created goal should be visible to owner",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.level, GoalLevel::Tactical);
    assert_eq!(saved.priority, 9);
    assert_eq!(saved.evidence_refs, vec!["event:1".to_string()]);
    assert_eq!(saved.memory_refs, vec!["memory:1".to_string()]);

    let mut hidden = Goal::new("hidden bob goal", GoalLevel::Strategic);
    hidden.owner_actor = "user:bob".to_string();
    let hidden_id = hidden.id.clone();
    must(state.goal_store().save(&hidden), "hidden goal should save");

    let list = handler.handle(&request(2, "goal/list", json!({})));
    let goals = list
        .result
        .as_ref()
        .and_then(|value| value.get("goals"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("goal/list should return goals: {list:?}"));
    assert_eq!(goals.len(), 1);
    assert_eq!(
        goals[0].get("id").and_then(serde_json::Value::as_str),
        Some(goal_id.as_str())
    );

    let get_hidden = handler.handle(&request(3, "goal/get", json!({ "id": hidden_id })));
    assert!(
        get_hidden.error.is_some(),
        "hidden goal should not be visible"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_goal_update_enforces_state_machine_and_open_filter() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let goal_id = create_goal(&handler);

    let invalid_backtrack = handler.handle(&request(
        4,
        "goal/update",
        json!({
            "id": goal_id.as_str(),
            "status": "Proposed"
        }),
    ));
    assert!(
        invalid_backtrack.error.is_some(),
        "Active goal should not move back to Proposed"
    );

    let block = handler.handle(&request(
        5,
        "goal/update",
        json!({
            "id": goal_id.as_str(),
            "status": "Blocked",
            "success_criteria": "blocked state persisted"
        }),
    ));
    assert!(
        block.result.is_some(),
        "goal block should succeed: {block:?}"
    );
    let blocked = must(
        state.goal_store().load_for_actor(&goal_id, "user:scott"),
        "blocked goal should load",
    );
    assert_eq!(blocked.status, GoalStatus::Blocked);
    assert_eq!(blocked.success_criteria, "blocked state persisted");

    let complete = handler.handle(&request(
        6,
        "goal/update",
        json!({
            "id": goal_id.as_str(),
            "status": "Completed"
        }),
    ));
    assert!(
        complete.result.is_some(),
        "goal completion should succeed: {complete:?}"
    );
    let completed = must(
        state.goal_store().load_for_actor(&goal_id, "user:scott"),
        "completed goal should load",
    );
    assert_eq!(completed.status, GoalStatus::Completed);
    assert!(completed.completed_at.is_some());

    let open = handler.handle(&request(7, "goal/list", json!({ "open_only": true })));
    let goals = open
        .result
        .as_ref()
        .and_then(|value| value.get("goals"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("open goal/list should return goals: {open:?}"));
    assert!(goals.is_empty(), "completed goal should not be open");
}
