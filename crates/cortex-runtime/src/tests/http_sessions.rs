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

async fn build_http_session_router(
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
async fn http_session_create_assigns_transport_actor_owner() {
    let (_temp, state, router) = build_http_session_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/session")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":"release-room"}"#))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http session create should return a response",
    );
    assert_eq!(response.status(), StatusCode::CREATED);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let session_id = payload
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("session create response should contain session_id"));
    let session = state
        .visible_sessions("user:scott")
        .into_iter()
        .find(|session| {
            session.id.to_string() == session_id || session.name.as_deref() == Some(session_id)
        })
        .unwrap_or_else(|| panic!("created session should be visible to the http actor"));

    assert_eq!(session.owner_actor, "user:scott");
    assert_eq!(session.name.as_deref(), Some("release-room"));
}

#[tokio::test]
async fn http_session_list_and_get_are_filtered_to_transport_actor() {
    let (_temp, state, router) = build_http_session_router("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let list_response = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/sessions")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http session list should return a response",
    );
    assert_eq!(list_response.status(), StatusCode::OK);
    let list_body = must(
        axum::body::to_bytes(list_response.into_body(), usize::MAX).await,
        "session list body should load",
    );
    let list_payload = parse_json(&list_body);
    let list_items = list_payload
        .as_array()
        .unwrap_or_else(|| panic!("session list response should be an array"));

    assert_eq!(list_items.len(), 1);
    assert_eq!(
        list_items[0]
            .get("owner_actor")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("session list item should contain owner_actor")),
        "user:scott"
    );

    let get_response = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/session/{scott_session}"))
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http session get should return a response",
    );
    assert_eq!(get_response.status(), StatusCode::OK);

    let hidden_response = must(
        router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/session/{bob_session}"))
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "hidden session get should return a response",
    );
    assert_eq!(hidden_response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn http_turn_rejects_inaccessible_session_ids() {
    let (_temp, state, router) = build_http_session_router("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/turn")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"session_id":"{bob_session}","input":"/status"}}"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http turn should return a response",
    );
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn http_turn_without_session_id_reuses_http_actor_session() {
    let (_temp, state, router) = build_http_session_router("user:scott").await;
    let (session_id, _) = state.create_session_for_actor("user:scott");

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/turn")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":"","input":"/status"}"#))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http turn should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    assert_eq!(
        payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("turn response should include session_id")),
        session_id
    );
}
