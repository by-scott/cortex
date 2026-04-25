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

async fn build_http_operator_router(
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
async fn http_operator_routes_require_local_operator_identity() {
    let (_temp, _state, router) = build_http_operator_router("user:scott").await;

    for uri in [
        "/api/daemon/status",
        "/api/health",
        "/api/metrics/structured",
    ] {
        let response = must(
            router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(uri)
                        .body(Body::empty())
                        .unwrap_or_else(|err| panic!("request should build: {err}")),
                )
                .await,
            "operator route should return a response",
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn http_operator_routes_remain_available_to_local_operator() {
    let (_temp, _state, router) = build_http_operator_router("local:default").await;

    let status = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/daemon/status")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "daemon status should return a response",
    );
    assert_eq!(status.status(), StatusCode::OK);

    let health = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "health should return a response",
    );
    assert_eq!(health.status(), StatusCode::OK);
    let health_body = must(
        axum::body::to_bytes(health.into_body(), usize::MAX).await,
        "health body should load",
    );
    let health_payload = parse_json(&health_body);
    assert_eq!(health_payload.get("status"), Some(&Value::from("ok")));

    let metrics = must(
        router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/metrics/structured")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "metrics should return a response",
    );
    assert_eq!(metrics.status(), StatusCode::OK);
}
