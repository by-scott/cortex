use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cortex_kernel::{ActorBindingsStore, CortexPaths};
use serde_json::Value;
use tower::util::ServiceExt;

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

async fn build_http_rpc_router(
    http_actor: &str,
) -> (tempfile::TempDir, Arc<DaemonState>, axum::Router<()>) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    bindings.set_transport_actor("http", http_actor);

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = Arc::new(must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    ));
    let router = DaemonServer::build_http_router_for_tests(&state);
    (temp, state, router)
}

fn parse_json(body: &[u8]) -> Value {
    match serde_json::from_slice(body) {
        Ok(value) => value,
        Err(err) => panic!("response should decode as JSON: {err}"),
    }
}

#[tokio::test]
async fn http_rpc_rejects_hidden_session_ids() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":1,"method":"session/get","params":{{"session_id":"{bob_session}"}}}}"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    assert!(
        payload.get("error").is_some(),
        "http rpc should reject hidden sessions: {payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_memory_list_stays_actor_scoped() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible HTTP RPC note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "Bob-visible HTTP RPC note",
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

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":2,"method":"memory/list","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let memories = payload
        .get("result")
        .and_then(|value| value.get("memories"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("http rpc memory/list should return memories: {payload:?}"));

    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/list item should contain content")),
        "Scott-visible HTTP RPC note"
    );
}
