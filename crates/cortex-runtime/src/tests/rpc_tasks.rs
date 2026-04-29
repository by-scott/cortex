use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_types::{SharedTask, SharedTaskStatus};
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

fn create_task(handler: &RpcHandler) -> String {
    let create = handler.handle(&request(
        1,
        "task/create",
        json!({
            "description": "ship task rpc",
            "priority": 9
        }),
    ));
    create
        .result
        .as_ref()
        .and_then(|value| value.get("task"))
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("task/create should return a task id: {create:?}"))
        .to_string()
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_task_create_and_list_are_actor_scoped() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let task_id = create_task(&handler);

    let saved = must(
        state.task_store().load_for_actor(&task_id, "user:scott"),
        "created task should be visible to owner",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.priority, 9);

    let mut hidden = SharedTask::new("hidden bob task");
    hidden.owner_actor = "user:bob".to_string();
    let hidden_id = hidden.id.clone();
    must(state.task_store().save(&hidden), "hidden task should save");

    let list = handler.handle(&request(2, "task/list", json!({})));
    let tasks = list
        .result
        .as_ref()
        .and_then(|value| value.get("tasks"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("task/list should return tasks: {list:?}"));
    assert_eq!(tasks.len(), 1);
    assert_eq!(
        tasks[0].get("id").and_then(serde_json::Value::as_str),
        Some(task_id.as_str())
    );

    let get_hidden = handler.handle(&request(3, "task/get", json!({ "id": hidden_id })));
    assert!(
        get_hidden.error.is_some(),
        "hidden task should not be visible"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_task_claim_and_update_enforce_state_machine() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;
    let task_id = create_task(&handler);

    let invalid_complete = handler.handle(&request(
        4,
        "task/update",
        json!({
            "id": task_id.as_str(),
            "status": "Completed"
        }),
    ));
    assert!(
        invalid_complete.error.is_some(),
        "Pending task should not jump directly to Completed"
    );

    let claim = handler.handle(&request(
        5,
        "task/claim",
        json!({
            "id": task_id.as_str(),
            "instance_id": "worker-1"
        }),
    ));
    assert!(
        claim.result.is_some(),
        "task/claim should succeed: {claim:?}"
    );
    let claimed = must(
        state.task_store().load_for_actor(&task_id, "user:scott"),
        "claimed task should load",
    );
    assert_eq!(claimed.status, SharedTaskStatus::Assigned);
    assert_eq!(claimed.assigned_instance.as_deref(), Some("worker-1"));

    let start = handler.handle(&request(
        6,
        "task/update",
        json!({
            "id": task_id.as_str(),
            "status": "InProgress"
        }),
    ));
    assert!(
        start.result.is_some(),
        "task start should succeed: {start:?}"
    );

    let complete = handler.handle(&request(
        7,
        "task/update",
        json!({
            "id": task_id.as_str(),
            "status": "Completed",
            "result": "done"
        }),
    ));
    assert!(
        complete.result.is_some(),
        "task completion should succeed: {complete:?}"
    );
    let completed = must(
        state.task_store().load_for_actor(&task_id, "user:scott"),
        "completed task should load",
    );
    assert_eq!(completed.status, SharedTaskStatus::Completed);
    assert_eq!(completed.result.as_deref(), Some("done"));
}
