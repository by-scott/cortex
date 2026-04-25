use std::path::PathBuf;
use std::sync::Arc;

use axum::body::Body;
use axum::http::{Request, StatusCode};
use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_turn::skills::{Skill, SkillContent};
use cortex_types::{ExecutionMode, SkillMetadata};
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

async fn post_json(router: axum::Router<()>, body: &'static str) -> axum::response::Response {
    must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    )
}

async fn parse_response_body(response: axum::response::Response, context: &str) -> Value {
    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        context,
    );
    parse_json(&body)
}

fn response_item_names(payload: &Value, field: &str) -> Vec<String> {
    payload
        .get("result")
        .and_then(|value| value.get(field))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|item| {
            item.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>()
}

struct TestSkill {
    name: &'static str,
    user_invocable: bool,
}

impl Skill for TestSkill {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &'static str {
        "test skill"
    }

    fn when_to_use(&self) -> &'static str {
        "test"
    }

    fn execution_mode(&self) -> ExecutionMode {
        ExecutionMode::Inline
    }

    fn content(&self, _args: &str) -> SkillContent {
        SkillContent::Markdown(format!("content:{}", self.name))
    }

    fn metadata(&self) -> SkillMetadata {
        SkillMetadata {
            user_invocable: self.user_invocable,
            agent_invocable: true,
            ..SkillMetadata::default()
        }
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
async fn http_rpc_single_notification_returns_no_content() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":null,"method":"memory/list","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc notification should return a response",
    );
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    assert!(
        body.is_empty(),
        "single notifications should not return a body"
    );
}

#[tokio::test]
async fn http_rpc_rejects_unsupported_content_type() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "text/plain")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"memory/list","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(response.status(), StatusCode::UNSUPPORTED_MEDIA_TYPE);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    assert!(
        payload.get("error").is_some(),
        "unsupported content type should return an error payload: {payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_returns_parse_error_for_malformed_json() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":1,"method":"memory/list","params":{"#,
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
    let error = payload
        .get("error")
        .unwrap_or_else(|| panic!("malformed JSON should produce an error: {payload:?}"));
    assert_eq!(error.get("code"), Some(&Value::from(-32_700)));
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

#[tokio::test]
async fn http_rpc_memory_save_assigns_transport_actor_owner() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;

    let response = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":29,"method":"memory/save","params":{"content":"Scott-only HTTP RPC note","description":"http rpc note","type":"Project"}}"#,
    )
    .await;
    let payload = parse_response_body(response, "memory/save body should load").await;
    let id = payload
        .get("result")
        .and_then(|value| value.get("id"))
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("memory/save should return id: {payload:?}"));

    let saved = must(
        state.memory_store().load_for_actor(id, "user:scott"),
        "saved memory should be visible to the http rpc actor",
    );
    assert_eq!(saved.owner_actor, "user:scott");
    assert_eq!(saved.description, "http rpc note");
}

#[tokio::test]
async fn http_rpc_memory_get_and_delete_respect_actor_visibility() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible HTTP RPC get/delete note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let own_id = own.id.clone();

    let mut other = cortex_types::MemoryEntry::new(
        "Bob-hidden HTTP RPC get/delete note",
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

    let get_own = post_json(
        router.clone(),
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":30,"method":"memory/get","params":{{"id":"{own_id}"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let get_own_payload = parse_response_body(get_own, "own memory/get body should load").await;
    assert!(
        get_own_payload.get("result").is_some(),
        "own memory/get should succeed: {get_own_payload:?}"
    );

    let get_hidden = post_json(
        router.clone(),
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":31,"method":"memory/get","params":{{"id":"{other_id}"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let get_hidden_payload =
        parse_response_body(get_hidden, "hidden memory/get body should load").await;
    assert!(
        get_hidden_payload.get("error").is_some(),
        "hidden memory/get should be rejected: {get_hidden_payload:?}"
    );

    let delete_hidden = post_json(
        router.clone(),
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":32,"method":"memory/delete","params":{{"id":"{other_id}"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let delete_hidden_payload =
        parse_response_body(delete_hidden, "hidden memory/delete body should load").await;
    assert!(
        delete_hidden_payload.get("error").is_some(),
        "hidden memory/delete should be rejected: {delete_hidden_payload:?}"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&other_id, "user:bob")
            .is_ok(),
        "hidden memory should remain after rejected delete"
    );

    let delete_own = post_json(
        router,
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":33,"method":"memory/delete","params":{{"id":"{own_id}"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let delete_own_payload =
        parse_response_body(delete_own, "own memory/delete body should load").await;
    assert!(
        delete_own_payload.get("result").is_some(),
        "own memory/delete should succeed: {delete_own_payload:?}"
    );
    assert!(
        state
            .memory_store()
            .load_for_actor(&own_id, "user:scott")
            .is_err(),
        "deleted own memory should no longer be visible"
    );
}

#[tokio::test]
async fn http_rpc_memory_search_stays_actor_scoped() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible HTTP RPC search note",
        "own searchable note",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();

    let mut hidden = cortex_types::MemoryEntry::new(
        "Bob-hidden HTTP RPC search note",
        "hidden searchable note",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    hidden.owner_actor = "user:bob".to_string();

    must(state.memory_store().save(&own), "own memory should save");
    must(
        state.memory_store().save(&hidden),
        "hidden memory should save",
    );

    let response = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":34,"method":"memory/search","params":{"query":"searchable","limit":10}}"#,
    )
    .await;
    let payload = parse_response_body(response, "memory/search body should load").await;
    let results = payload
        .get("result")
        .and_then(|value| value.get("results"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/search item should contain content")),
        "Scott-visible HTTP RPC search note"
    );
}

#[tokio::test]
async fn http_rpc_batch_preserves_actor_scoped_results() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible HTTP RPC batch note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    let mut other = cortex_types::MemoryEntry::new(
        "Bob-visible HTTP RPC batch note",
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
                    .body(Body::from(format!(
                        r#"[{{"jsonrpc":"2.0","id":1,"method":"session/get","params":{{"session_id":"{bob_session}"}}}},{{"jsonrpc":"2.0","id":2,"method":"memory/list","params":{{}}}}]"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc batch should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let items = payload
        .as_array()
        .unwrap_or_else(|| panic!("batch response should be an array: {payload:?}"));
    assert_eq!(items.len(), 2);
    assert!(
        items[0].get("error").is_some(),
        "hidden session/get should fail inside batch: {payload:?}"
    );
    let memories = items[1]
        .get("result")
        .and_then(|value| value.get("memories"))
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("memory/list should succeed inside batch: {payload:?}"));
    assert_eq!(memories.len(), 1);
    assert_eq!(
        memories[0]
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or_else(|| panic!("memory/list item should contain content")),
        "Scott-visible HTTP RPC batch note"
    );
}

#[tokio::test]
async fn http_rpc_batch_omits_notifications_from_payload() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;

    let mut own = cortex_types::MemoryEntry::new(
        "Scott-visible HTTP RPC notification note",
        "own",
        cortex_types::MemoryType::Project,
        cortex_types::MemoryKind::Semantic,
    );
    own.owner_actor = "user:scott".to_string();
    must(state.memory_store().save(&own), "own memory should save");

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"[{"jsonrpc":"2.0","id":null,"method":"memory/list","params":{}},{"jsonrpc":"2.0","id":9,"method":"memory/list","params":{}}]"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc batch should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    let items = payload
        .as_array()
        .unwrap_or_else(|| panic!("batch response should be an array: {payload:?}"));
    assert_eq!(items.len(), 1);
    assert_eq!(
        items[0].get("id").cloned().unwrap_or(Value::Null),
        Value::from(9)
    );
}

#[tokio::test]
async fn http_rpc_batch_returns_no_content_for_notifications_only() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"[{"jsonrpc":"2.0","id":null,"method":"memory/list","params":{}}]"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc notification batch should return a response",
    );
    assert_eq!(response.status(), StatusCode::NO_CONTENT);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    assert!(
        body.is_empty(),
        "notification batches should not return a body"
    );
}

#[tokio::test]
async fn http_rpc_rejects_empty_batches() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let response = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from("[]"))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc empty batch should return a response",
    );
    assert_eq!(response.status(), StatusCode::OK);

    let body = must(
        axum::body::to_bytes(response.into_body(), usize::MAX).await,
        "response body should load",
    );
    let payload = parse_json(&body);
    assert!(
        payload.get("error").is_some(),
        "empty batches should produce an invalid-request error: {payload:?}"
    );
    assert_eq!(payload.get("id"), Some(&Value::Null));
}

#[tokio::test]
async fn http_rpc_meta_alerts_respects_actor_visibility() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let own = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":10,"method":"meta/alerts","params":{{"session_id":"{scott_session}"}}}}"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(own.status(), StatusCode::OK);
    let own_body = must(
        axum::body::to_bytes(own.into_body(), usize::MAX).await,
        "response body should load",
    );
    let own_payload = parse_json(&own_body);
    assert!(
        own_payload.get("result").is_some(),
        "visible meta/alerts should succeed: {own_payload:?}"
    );

    let hidden = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":11,"method":"meta/alerts","params":{{"session_id":"{bob_session}"}}}}"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(hidden.status(), StatusCode::OK);
    let hidden_body = must(
        axum::body::to_bytes(hidden.into_body(), usize::MAX).await,
        "response body should load",
    );
    let hidden_payload = parse_json(&hidden_body);
    assert!(
        hidden_payload.get("error").is_some(),
        "hidden meta/alerts should be rejected: {hidden_payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_command_dispatch_rejects_hidden_session_ids() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (_scott_session, _) = state.create_session_for_actor("user:scott");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let hidden = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"jsonrpc":"2.0","id":12,"method":"command/dispatch","params":{{"session_id":"{bob_session}","command":"/status"}}}}"#
                    )))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(hidden.status(), StatusCode::OK);
    let hidden_body = must(
        axum::body::to_bytes(hidden.into_body(), usize::MAX).await,
        "response body should load",
    );
    let hidden_payload = parse_json(&hidden_body);
    assert!(
        hidden_payload.get("error").is_some(),
        "hidden command/dispatch should be rejected: {hidden_payload:?}"
    );

    let own = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":13,"method":"command/dispatch","params":{"command":"/status"}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(own.status(), StatusCode::OK);
    let own_body = must(
        axum::body::to_bytes(own.into_body(), usize::MAX).await,
        "response body should load",
    );
    let own_payload = parse_json(&own_body);
    assert!(
        own_payload.get("result").is_some(),
        "sessionless command/dispatch should succeed for visible actor: {own_payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_operator_methods_require_local_operator_identity() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let status = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":14,"method":"daemon/status","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = must(
        axum::body::to_bytes(status.into_body(), usize::MAX).await,
        "status body should load",
    );
    let status_payload = parse_json(&status_body);
    assert!(
        status_payload.get("error").is_some(),
        "daemon/status should reject non-local operators: {status_payload:?}"
    );

    let reload = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":15,"method":"admin/reload-config","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(reload.status(), StatusCode::OK);
    let reload_body = must(
        axum::body::to_bytes(reload.into_body(), usize::MAX).await,
        "reload body should load",
    );
    let reload_payload = parse_json(&reload_body);
    assert!(
        reload_payload.get("error").is_some(),
        "admin/reload-config should reject non-local operators: {reload_payload:?}"
    );

    let health = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":16,"method":"health/check","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(health.status(), StatusCode::OK);
    let health_body = must(
        axum::body::to_bytes(health.into_body(), usize::MAX).await,
        "health body should load",
    );
    let health_payload = parse_json(&health_body);
    assert!(
        health_payload.get("error").is_some(),
        "health/check should reject non-local operators: {health_payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_local_operator_methods_return_results() {
    let (_temp, _state, router) = build_http_rpc_router("local:default").await;

    let status = post_json(
        router.clone(),
        r#"{"jsonrpc":"2.0","id":14,"method":"daemon/status","params":{}}"#,
    )
    .await;
    assert_eq!(status.status(), StatusCode::OK);
    let status_payload = parse_response_body(status, "operator status body should load").await;
    assert!(
        status_payload.get("result").is_some(),
        "daemon/status should succeed for local operator: {status_payload:?}"
    );

    let reload = post_json(
        router.clone(),
        r#"{"jsonrpc":"2.0","id":15,"method":"admin/reload-config","params":{}}"#,
    )
    .await;
    assert_eq!(reload.status(), StatusCode::OK);
    let reload_payload = parse_response_body(reload, "operator reload body should load").await;
    assert!(
        reload_payload.get("result").is_some(),
        "admin/reload-config should succeed for local operator: {reload_payload:?}"
    );

    let health = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":16,"method":"health/check","params":{}}"#,
    )
    .await;
    assert_eq!(health.status(), StatusCode::OK);
    let health_payload = parse_response_body(health, "operator health body should load").await;
    assert!(
        health_payload.get("result").is_some(),
        "health/check should succeed for local operator: {health_payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_filters_local_operator_only_tools() {
    let (_temp, _state, router) = build_http_rpc_router("user:scott").await;

    let init = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":17,"method":"session/initialize","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(init.status(), StatusCode::OK);
    let init_body = must(
        axum::body::to_bytes(init.into_body(), usize::MAX).await,
        "init body should load",
    );
    let init_payload = parse_json(&init_body);
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !init_tools.iter().any(|value| value == forbidden),
            "non-local http actor should not see {forbidden}: {init_tools:?}"
        );
    }

    let tools = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":18,"method":"mcp/tools-list","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(tools.status(), StatusCode::OK);
    let tools_body = must(
        axum::body::to_bytes(tools.into_body(), usize::MAX).await,
        "tools body should load",
    );
    let tools_payload = parse_json(&tools_body);
    let tool_names = tools_payload
        .get("result")
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|tool| {
            tool.get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    for forbidden in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            !tool_names.iter().any(|name| name == forbidden),
            "non-local http actor should not see {forbidden} through mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn http_rpc_local_operator_keeps_introspection_tools_visible() {
    let (_temp, _state, router) = build_http_rpc_router("local:default").await;

    let init = post_json(
        router.clone(),
        r#"{"jsonrpc":"2.0","id":24,"method":"session/initialize","params":{}}"#,
    )
    .await;
    assert_eq!(init.status(), StatusCode::OK);
    let init_payload = parse_response_body(init, "operator init body should load").await;
    let init_tools = init_payload
        .get("result")
        .and_then(|value| value.get("capabilities"))
        .and_then(|value| value.get("tools"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            init_tools.iter().any(|value| value == expected),
            "local operator should keep {expected} through http session/initialize: {init_tools:?}"
        );
    }

    let tools = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":25,"method":"mcp/tools-list","params":{}}"#,
    )
    .await;
    assert_eq!(tools.status(), StatusCode::OK);
    let tools_payload = parse_response_body(tools, "operator tools body should load").await;
    let tool_names = response_item_names(&tools_payload, "tools");
    for expected in ["audit", "prompt_inspect", "memory_graph"] {
        assert!(
            tool_names.iter().any(|name| name == expected),
            "local operator should keep {expected} through http mcp/tools-list: {tool_names:?}"
        );
    }
}

#[tokio::test]
async fn http_rpc_mcp_tools_call_enforces_local_operator_only_introspection() {
    let (_temp, _state, user_router) = build_http_rpc_router("user:scott").await;
    let user_call = post_json(
        user_router,
        r#"{"jsonrpc":"2.0","id":26,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    assert_eq!(user_call.status(), StatusCode::OK);
    let user_payload = parse_response_body(user_call, "user tools-call body should load").await;
    assert!(
        user_payload.get("error").is_some(),
        "non-local actor should not reach prompt_inspect through http mcp/tools-call: {user_payload:?}"
    );

    let (_temp, _state, operator_router) = build_http_rpc_router("local:default").await;
    let operator_call = post_json(
        operator_router,
        r#"{"jsonrpc":"2.0","id":27,"method":"mcp/tools-call","params":{"name":"prompt_inspect","arguments":{"layer":"soul"}}}"#,
    )
    .await;
    assert_eq!(operator_call.status(), StatusCode::OK);
    let operator_payload =
        parse_response_body(operator_call, "operator tools-call body should load").await;
    assert!(
        operator_payload.get("result").is_some(),
        "local operator should reach prompt_inspect through http mcp/tools-call: {operator_payload:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_rpc_session_cancel_requests_active_visible_turn() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (_session_id, control) = state.register_active_turn_for_actor("user:scott");

    let response = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":25,"method":"session/cancel","params":{}}"#,
    )
    .await;
    let payload = parse_response_body(response, "session cancel body should load").await;

    assert_eq!(
        payload
            .get("result")
            .and_then(|value| value.get("message"))
            .and_then(Value::as_str),
        Some("Turn cancellation requested")
    );
    assert!(
        control.is_cancel_requested(),
        "http rpc session/cancel should request cancellation for the active visible turn"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_rpc_session_cancel_rejects_hidden_session_ids() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let response = post_json(
        router,
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":26,"method":"session/cancel","params":{{"session_id":"{bob_session}"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let payload = parse_response_body(response, "hidden session cancel body should load").await;

    assert!(
        payload.get("error").is_some(),
        "http rpc session/cancel should reject hidden sessions: {payload:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_rpc_command_dispatch_stop_requests_active_visible_turn() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (_session_id, control) = state.register_active_turn_for_actor("user:scott");

    let response = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":27,"method":"command/dispatch","params":{"command":"/stop"}}"#,
    )
    .await;
    let payload = parse_response_body(response, "command stop body should load").await;

    assert_eq!(
        payload
            .get("result")
            .and_then(|value| value.get("output"))
            .and_then(Value::as_str),
        Some("Turn cancellation requested.")
    );
    assert!(
        control.is_cancel_requested(),
        "http rpc command/dispatch /stop should request cancellation"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn http_rpc_command_dispatch_stop_rejects_hidden_session_ids() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    let (bob_session, _) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let response = post_json(
        router,
        Box::leak(
            format!(
                r#"{{"jsonrpc":"2.0","id":28,"method":"command/dispatch","params":{{"session_id":"{bob_session}","command":"/stop"}}}}"#
            )
            .into_boxed_str(),
        ),
    )
    .await;
    let payload = parse_response_body(response, "hidden stop body should load").await;

    assert!(
        payload.get("error").is_some(),
        "http rpc command/dispatch /stop should reject hidden sessions: {payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_filters_non_user_invocable_prompts() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let prompts = must(
        router
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":19,"method":"mcp/prompts-list","params":{}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(prompts.status(), StatusCode::OK);
    let prompts_body = must(
        axum::body::to_bytes(prompts.into_body(), usize::MAX).await,
        "prompts body should load",
    );
    let prompts_payload = parse_json(&prompts_body);
    let prompt_names = prompts_payload
        .get("result")
        .and_then(|value| value.get("prompts"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|prompt| {
            prompt
                .get("name")
                .and_then(serde_json::Value::as_str)
                .map(str::to_string)
        })
        .collect::<Vec<_>>();
    assert!(prompt_names.iter().any(|name| name == "visible-skill"));
    assert!(!prompt_names.iter().any(|name| name == "hidden-skill"));

    let hidden = must(
        router
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/rpc")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","id":20,"method":"mcp/prompts-get","params":{"name":"hidden-skill","arguments":""}}"#,
                    ))
                    .unwrap_or_else(|err| panic!("request should build: {err}")),
            )
            .await,
        "http rpc should return a response",
    );
    assert_eq!(hidden.status(), StatusCode::OK);
    let hidden_body = must(
        axum::body::to_bytes(hidden.into_body(), usize::MAX).await,
        "hidden prompt body should load",
    );
    let hidden_payload = parse_json(&hidden_body);
    assert!(
        hidden_payload.get("error").is_some(),
        "hidden user skill should not be exposed through http mcp/prompts-get: {hidden_payload:?}"
    );
}

#[tokio::test]
async fn http_rpc_filters_non_user_invocable_skills() {
    let (_temp, state, router) = build_http_rpc_router("user:scott").await;
    state.skill_registry().register(Box::new(TestSkill {
        name: "visible-skill",
        user_invocable: true,
    }));
    state.skill_registry().register(Box::new(TestSkill {
        name: "hidden-skill",
        user_invocable: false,
    }));

    let list = post_json(
        router.clone(),
        r#"{"jsonrpc":"2.0","id":21,"method":"skill/list","params":{}}"#,
    )
    .await;
    assert_eq!(list.status(), StatusCode::OK);
    let list_payload = parse_response_body(list, "skill list body should load").await;
    let skill_names = response_item_names(&list_payload, "skills");
    assert!(skill_names.iter().any(|name| name == "visible-skill"));
    assert!(!skill_names.iter().any(|name| name == "hidden-skill"));

    let hidden_invoke = post_json(
        router.clone(),
        r#"{"jsonrpc":"2.0","id":22,"method":"skill/invoke","params":{"name":"hidden-skill","args":""}}"#,
    )
    .await;
    assert_eq!(hidden_invoke.status(), StatusCode::OK);
    let hidden_invoke_payload =
        parse_response_body(hidden_invoke, "hidden invoke body should load").await;
    assert!(
        hidden_invoke_payload.get("error").is_some(),
        "hidden user skill should not be invocable through http skill/invoke: {hidden_invoke_payload:?}"
    );

    let suggestions = post_json(
        router,
        r#"{"jsonrpc":"2.0","id":23,"method":"skill/suggestions","params":{"input":"hidden-skill visible-skill"}}"#,
    )
    .await;
    assert_eq!(suggestions.status(), StatusCode::OK);
    let suggestions_payload =
        parse_response_body(suggestions, "suggestions body should load").await;
    let suggestion_names = response_item_names(&suggestions_payload, "suggestions");
    assert!(suggestion_names.iter().any(|name| name == "visible-skill"));
    assert!(!suggestion_names.iter().any(|name| name == "hidden-skill"));
}
