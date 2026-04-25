use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use cortex_types::RiskLevel;

use crate::daemon::{CancelTurnError, DaemonState};
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

async fn build_state(actor: &str) -> (tempfile::TempDir, Arc<DaemonState>) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    bindings.set_transport_actor("rpc", actor);

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = Arc::new(must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    ));
    (temp, state)
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_turn_for_actor_denies_pending_permissions_for_target_session() {
    let (_temp, state) = build_state("user:scott").await;
    let (session_id, control) = state.register_active_turn_for_actor("user:scott");
    let permission_id = state.register_pending_permission_for_session(
        &session_id,
        "user:scott",
        "rpc",
        "project_map",
        RiskLevel::RequireConfirmation,
    );

    let cancelled = state.cancel_turn_for_actor("user:scott", None);
    assert!(matches!(cancelled, Ok(ref value) if value == &session_id));
    assert!(control.is_cancel_requested());
    assert!(
        state.pending_permission_info(&permission_id).is_none(),
        "cancel should remove pending permissions for the target session"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn cancel_turn_for_actor_rejects_hidden_session_ids() {
    let (_temp, state) = build_state("user:scott").await;
    let (hidden_session, _meta) = state.create_session_for_actor("user:bob");
    let _ = state.register_active_turn_for_actor("user:bob");

    let cancelled = state.cancel_turn_for_actor("user:scott", Some(&hidden_session));
    assert!(matches!(cancelled, Err(CancelTurnError::SessionNotFound)));
}

#[tokio::test(flavor = "multi_thread")]
async fn admin_cancel_without_explicit_session_targets_global_active_turn() {
    let (_temp, state) = build_state("local:default").await;
    let (session_id, control) = state.register_active_turn_for_actor("user:scott");

    let cancelled = state.cancel_turn_for_actor("local:default", None);
    assert!(matches!(cancelled, Ok(ref value) if value == &session_id));
    assert!(control.is_cancel_requested());
}
