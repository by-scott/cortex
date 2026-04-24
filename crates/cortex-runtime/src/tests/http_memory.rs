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

async fn build_http_memory_router(
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
async fn http_memory_save_assigns_transport_actor_owner() {
    let (_temp, state, router) = build_http_memory_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/memory")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"content":"Scott-only HTTP note","description":"http note","type":"Project"}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http memory save should return a response",
    );
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let id = payload
        .get("id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("memory save response should contain id"));

    let saved = must(
        state.memory_store().load_for_actor(id, "user:scott"),
        "saved memory should be visible to the http actor",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.description, "http note");
}

#[tokio::test]
async fn http_memory_list_is_filtered_to_transport_actor() {
    let (_temp, state, router) = build_http_memory_router("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "Bob-visible note",
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
                    .method("GET")
                    .uri("/api/memory")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http memory list should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let items = payload
        .as_array()
        .unwrap_or_else(|| panic!("memory list response should be an array"));

    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0]
            .get("owner_actor")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory list item should include owner_actor")),
        "user:scott"
    );
    assert_eq!(
        items[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory list item should include content")),
        "Scott-visible note"
    );
}
