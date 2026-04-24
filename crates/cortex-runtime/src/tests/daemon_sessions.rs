use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use tempfile::TempDir;

use crate::channels::store::ChannelStore;
use crate::channels::{ChannelSlashAction, resolve_channel_slash};
use crate::daemon::DaemonState;
use crate::daemon::{BroadcastEvent, BroadcastMessage};
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
) -> (TempDir, PathBuf, DaemonState) {
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
    (temp, home, state)
}

#[tokio::test]
async fn resolve_actor_session_reuses_visible_session_for_same_canonical_actor() {
    let (_temp, _home, state) = build_state_with_bindings(
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
    let (_temp, _home, state) =
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
async fn pairing_does_not_allocate_session_before_first_real_message() {
    let (_temp, home, state) =
        build_state_with_bindings(&[("telegram:5188621876", "user:scott")], &[]).await;
    let store = ChannelStore::open(&home, "telegram");

    store.save_pending_pairs(&[crate::channels::store::PendingPair {
        user_id: "5188621876".to_string(),
        user_name: "Scott".to_string(),
        code: "ABC123".to_string(),
        created_at: "2026-04-24T00:00:00Z".to_string(),
    }]);
    let approved = must(
        store.approve_pending_pair("5188621876"),
        "pair approval should succeed",
    );
    assert_eq!(approved.user_id, "5188621876");

    assert!(state.active_actor_session("telegram:5188621876").is_none());
    assert!(state.session_manager().list_sessions().is_empty());

    let first_session = state.resolve_actor_session("telegram:5188621876");
    assert_eq!(state.session_manager().list_sessions().len(), 1);
    assert_eq!(
        state.active_actor_session("telegram:5188621876"),
        Some(first_session)
    );
}

#[tokio::test]
async fn first_real_message_reuses_visible_session_for_newly_paired_client() {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[],
    )
    .await;
    let telegram_store = ChannelStore::open(&home, "telegram");
    let qq_store = ChannelStore::open(&home, "qq");

    telegram_store.save_pending_pairs(&[crate::channels::store::PendingPair {
        user_id: "5188621876".to_string(),
        user_name: "Scott".to_string(),
        code: "TG1234".to_string(),
        created_at: "2026-04-24T00:00:00Z".to_string(),
    }]);
    qq_store.save_pending_pairs(&[crate::channels::store::PendingPair {
        user_id: "bot-user".to_string(),
        user_name: "ScottQQ".to_string(),
        code: "QQ1234".to_string(),
        created_at: "2026-04-24T00:00:00Z".to_string(),
    }]);
    let _ = must(
        telegram_store.approve_pending_pair("5188621876"),
        "telegram pair approval should succeed",
    );
    let _ = must(
        qq_store.approve_pending_pair("bot-user"),
        "qq pair approval should succeed",
    );

    assert!(state.active_actor_session("telegram:5188621876").is_none());
    assert!(state.active_actor_session("qq:bot-user").is_none());

    let session_from_telegram = state.resolve_actor_session("telegram:5188621876");
    assert_eq!(state.session_manager().list_sessions().len(), 1);

    let session_from_qq = state.resolve_actor_session("qq:bot-user");
    assert_eq!(session_from_qq, session_from_telegram);
    assert_eq!(state.session_manager().list_sessions().len(), 1);
}

#[tokio::test]
async fn client_active_sessions_stay_distinct_within_one_canonical_actor() {
    let (_temp, _home, state) = build_state_with_bindings(
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
async fn session_broadcasts_stay_isolated_per_session() {
    let (_temp, _home, state) = build_state_with_bindings(&[], &[]).await;
    let (first_session, _) = state.create_session_for_actor("user:alice");
    let (second_session, _) = state.create_session_for_actor("user:alice");
    let mut first_rx = state.subscribe_session(&first_session);
    let mut second_rx = state.subscribe_session(&second_session);

    let _ = state
        .session_broadcast(&first_session)
        .send(BroadcastMessage {
            session_id: first_session.clone(),
            source: "http".to_string(),
            event: BroadcastEvent::Text("first".to_string()),
        });
    let _ = state
        .session_broadcast(&second_session)
        .send(BroadcastMessage {
            session_id: second_session.clone(),
            source: "qq".to_string(),
            event: BroadcastEvent::Text("second".to_string()),
        });

    let first_msg = must(
        tokio::time::timeout(std::time::Duration::from_millis(100), first_rx.recv()).await,
        "first session should receive a broadcast",
    );
    let second_msg = must(
        tokio::time::timeout(std::time::Duration::from_millis(100), second_rx.recv()).await,
        "second session should receive a broadcast",
    );

    let first_msg = must(first_msg, "first session broadcast should decode");
    let second_msg = must(second_msg, "second session broadcast should decode");

    assert_eq!(first_msg.session_id, first_session);
    assert_eq!(first_msg.source, "http");
    assert!(matches!(first_msg.event, BroadcastEvent::Text(ref text) if text == "first"));

    assert_eq!(second_msg.session_id, second_session);
    assert_eq!(second_msg.source, "qq");
    assert!(matches!(second_msg.event, BroadcastEvent::Text(ref text) if text == "second"));
}

#[tokio::test]
async fn channel_session_commands_are_scoped_to_actor_visibility() {
    let (_temp, _home, state) = build_state_with_bindings(&[], &[]).await;
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
