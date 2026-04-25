use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cortex_kernel::{ActorBindingsStore, CortexPaths};
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

async fn build_http_meta_router(
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

#[tokio::test]
async fn http_meta_alerts_respects_transport_actor_visibility() {
    let (_temp, state, router) = build_http_meta_router("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let own = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/meta/alerts?session_id={scott_session}"))
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "meta alerts should return a response",
    );
    assert_eq!(own.status(), StatusCode::OK);

    let hidden = must(
        router
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(format!("/api/meta/alerts?session_id={bob_session}"))
                    .body(Body::empty())
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "hidden meta alerts should return a response",
    );
    assert_eq!(hidden.status(), StatusCode::NOT_FOUND);
}
