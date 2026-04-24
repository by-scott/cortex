use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use tempfile::TempDir;

use crate::channels::{ChannelSlashAction, resolve_channel_slash};
use crate::daemon::DaemonState;
use crate::runtime::CortexRuntime;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn temp_paths() -> (TempDir, PathBuf, PathBuf) {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let base = temp.path().join("cortex-home");
    let home = base.join("default");
    (temp, base, home)
}

async fn build_state_with_bindings(
    aliases: &[(&str, &str)],
    transports: &[(&str, &str)],
) -> (TempDir, DaemonState) {
    let (temp, base, home) = temp_paths();
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    for (from, to) in aliases {
        bindings.set_actor_alias(from, to);
    }
    for (transport, actor) in transports {
        bindings.set_transport_actor(transport, actor);
    }

    let mut runtime = must(
        CortexRuntime::new(&base, &home).await,
        "runtime should initialize",
    );
    let state = must(
        DaemonState::from_runtime(&mut runtime),
        "daemon state should initialize",
    );
    (temp, state)
}

#[tokio::test]
async fn resolve_actor_session_reuses_visible_session_for_same_canonical_actor() {
    let (_temp, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[],
    )
    .await;

    let (session_id, _meta) = state.create_session_for_actor("telegram:5188621876");

    let visible = state.visible_sessions("qq:bot-user");
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].id.to_string(), session_id);

    let resolved = state.resolve_actor_session("qq:bot-user");
    assert_eq!(resolved, session_id);
    assert_eq!(state.session_manager().list_sessions().len(), 1);
}

#[tokio::test]
async fn visible_sessions_for_transport_follow_bound_actor() {
    let (_temp, state) =
        build_state_with_bindings(&[], &[("http", "user:alice"), ("socket", "user:bob")]).await;

    let (alice_session, _) = state.create_session_for_actor("user:alice");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let http_sessions = state.visible_sessions_for_transport("http");
    assert_eq!(http_sessions.len(), 1);
    assert_eq!(http_sessions[0].id.to_string(), alice_session);

    let socket_sessions = state.visible_sessions_for_transport("socket");
    assert_eq!(socket_sessions.len(), 1);
    assert_eq!(socket_sessions[0].id.to_string(), bob_session);

    let admin_sessions = state.visible_sessions("local:default");
    assert_eq!(admin_sessions.len(), 2);
}

#[tokio::test]
async fn client_active_sessions_stay_distinct_within_one_canonical_actor() {
    let (_temp, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[],
    )
    .await;

    let (first_session, _) = state
        .session_manager()
        .create_session_with_id_for_actor("shared-a", "user:scott");
    let (second_session, _) = state
        .session_manager()
        .create_session_with_id_for_actor("shared-b", "user:scott");
    let first_session = first_session.to_string();
    let second_session = second_session.to_string();

    state.set_actor_session("telegram:5188621876", &first_session);
    state.set_actor_session("qq:bot-user", &second_session);

    assert_eq!(
        state.active_actor_session("telegram:5188621876"),
        Some(first_session)
    );
    assert_eq!(
        state.active_actor_session("qq:bot-user"),
        Some(second_session)
    );

    let telegram_visible = state.visible_sessions("telegram:5188621876");
    let qq_visible = state.visible_sessions("qq:bot-user");
    assert_eq!(telegram_visible.len(), 2);
    assert_eq!(qq_visible.len(), 2);
}

#[tokio::test]
async fn channel_session_commands_are_scoped_to_actor_visibility() {
    let (_temp, state) = build_state_with_bindings(&[], &[]).await;
    let (alice_session, _) = state
        .session_manager()
        .create_session_with_id_for_actor("alice-visible", "user:alice");
    let (bob_session, _) = state
        .session_manager()
        .create_session_with_id_for_actor("bob-hidden", "user:bob");
    state.set_actor_session("user:alice", &alice_session.to_string());
    state.set_actor_session("user:bob", &bob_session.to_string());
    let alice_session = alice_session.to_string();
    let bob_session = bob_session.to_string();
    let state = Arc::new(state);

    let list = resolve_channel_slash(&state, "user:alice", "/session list");
    match list {
        ChannelSlashAction::Reply(text) => {
            assert!(text.contains("alice-visible"));
            assert!(!text.contains("bob-hidden"));
        }
        ChannelSlashAction::RunPrompt { .. } => {
            panic!("session list should reply directly");
        }
    }

    let denied = resolve_channel_slash(
        &state,
        "user:alice",
        &format!("/session switch {bob_session}"),
    );
    match denied {
        ChannelSlashAction::Reply(text) => {
            assert_eq!(text, "You can only access your own sessions.");
        }
        ChannelSlashAction::RunPrompt { .. } => {
            panic!("session switch should reply directly");
        }
    }

    let allowed = resolve_channel_slash(
        &state,
        "user:alice",
        &format!("/session switch {alice_session}"),
    );
    match allowed {
        ChannelSlashAction::Reply(text) => {
            assert_eq!(text, format!("Switched to session: {alice_session}"));
        }
        ChannelSlashAction::RunPrompt { .. } => {
            panic!("session switch should reply directly");
        }
    }
}
