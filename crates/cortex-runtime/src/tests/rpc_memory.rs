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
async fn rpc_memory_save_assigns_transport_actor_owner() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/save".to_string(),
        id: json!(1),
        params: json!({
            "content": "Scott-only RPC note",
            "description": "rpc note",
            "type": "Project"
        }),
    });

    let id = response
        .result
        .as_ref()
        .and_then(|value| value.get("id"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or_else(|| panic!("memory/save should return id: {response:?}"));

    let saved = must(
        state.memory_store().load_for_actor(id, "user:scott"),
        "saved memory should be visible to the rpc actor",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.description, "rpc note");
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_memory_list_is_filtered_to_transport_actor() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible RPC note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "Bob-visible RPC note",
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

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/list".to_string(),
        id: json!(2),
        params: json!({}),
    });

    let memories = response
        .result
        .as_ref()
        .and_then(|value| value.get("memories"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("memory/list should return a memories array: {response:?}"));

    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("memory/list item should contain content")),
        "Scott-visible RPC note"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_memory_get_and_delete_respect_transport_actor_visibility() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-owned RPC memory",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let own_id = own.id.clone();

    let mut other = cortex_types::MemoryEntry::new(
        "Bob-owned RPC memory",
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

    let get_own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/get".to_string(),
        id: json!(3),
        params: json!({ "id": own_id }),
    });
    assert!(
        get_own.result.is_some(),
        "own memory should be visible: {get_own:?}"
    );

    let get_hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/get".to_string(),
        id: json!(4),
        params: json!({ "id": other_id }),
    });
    assert!(
        get_hidden.error.is_some(),
        "hidden memory should be rejected"
    );

    let delete_hidden = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/delete".to_string(),
        id: json!(5),
        params: json!({ "id": other_id }),
    });
    assert!(
        delete_hidden.error.is_some(),
        "deleting hidden memory should be rejected"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&other_id, "user:bob")
            .is_ok(),
        "hidden memory should remain after rejected delete"
    );

    let delete_own = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/delete".to_string(),
        id: json!(6),
        params: json!({ "id": own_id }),
    });
    assert!(
        delete_own.result.is_some(),
        "own memory delete should succeed"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&own_id, "user:scott")
            .is_err(),
        "deleted own memory should no longer be visible"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn rpc_memory_search_is_filtered_to_transport_actor() {
    let (_temp, state, handler) = build_rpc_handler("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "release checklist for scott",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "release checklist for bob",
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

    let response = handler.handle(&RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/search".to_string(),
        id: json!(7),
        params: json!({
            "query": "release checklist",
            "limit": 10
        }),
    });

    let memories = response
        .result
        .as_ref()
        .and_then(|value| value.get("results"))
        .and_then(serde_json::Value::as_array)
        .unwrap_or_else(|| panic!("memory/search should return a results array: {response:?}"));

    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_else(|| panic!("search result should contain content")),
        "release checklist for scott"
    );
}
