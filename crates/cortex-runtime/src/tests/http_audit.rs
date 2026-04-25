use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_types::{CorrelationId, Event, Payload, TurnId};
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

async fn build_http_audit_router(
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
async fn http_audit_routes_require_local_operator_identity() {
    let (_temp, _state, router) = build_http_audit_router("user:scott").await;

    for uri in [
        "/api/audit/summary".to_string(),
        "/api/audit/health".to_string(),
        "/api/audit/decision-path/correlation-1".to_string(),
    ] {
        let response = must(
            router
                .clone()
                .oneshot(
                    Request::builder()
                        .method("GET")
                        .uri(&uri)
                        .body(Body::empty())
                        .unwrap_or_else(|err| panic!("request should build: {err}")),
                )
                .await,
            "audit route should return a response",
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}

#[tokio::test]
async fn http_audit_routes_remain_available_to_local_operator() {
    let (_temp, state, router) = build_http_audit_router("local:default").await;
    let turn_id = TurnId::new();
    let correlation_id = CorrelationId::new();

    must(
        state
            .journal()
            .append(&Event::new(turn_id, correlation_id, Payload::TurnStarted)),
        "turn started should append",
    );

    let summary = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/audit/summary")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "audit summary should return a response",
    );
    assert_eq!(summary.status(), StatusCode::OK);
    let summary_body = must(
        axum::body::to_bytes(summary.into_body(), usize::MAX).await,
        "summary body should load",
    );
    let summary_payload = parse_json(&summary_body);
    assert_eq!(summary_payload.get("turn_count"), Some(&Value::from(1)));

    let health = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/api/audit/health")
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "audit health should return a response",
    );
    assert_eq!(health.status(), StatusCode::OK);

    let decision_path = must(
        router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/audit/decision-path/{correlation_id}"))
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "audit decision path should return a response",
    );
    assert_eq!(decision_path.status(), StatusCode::OK);
}
