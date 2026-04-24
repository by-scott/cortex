use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, CortexPaths};
use tempfile::TempDir;

use crate::channels::store::ChannelStore;
use crate::channels::{ChannelSlashAction, handle_message_events, resolve_channel_slash};
use crate::daemon::DaemonState;
use crate::daemon::{BroadcastEvent, BroadcastMessage};
use crate::hot_reload::ReloadTarget;
use crate::runtime::CortexRuntime;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn seed_pending_pair(store: &ChannelStore, user_id: &str, user_name: &str, code: &str) {
    store.save_pending_pairs(&[crate::channels::store::PendingPair {
        user_id: user_id.to_string(),
        user_name: user_name.to_string(),
        code: code.to_string(),
        created_at: "2026-04-24T00:00:00Z".to_string(),
    }]);
}

fn canonical_actor(home: &std::path::Path, actor: &str) -> String {
    let store = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(home));
    let aliases = store.actor_aliases();
    let mut current = actor.to_string();
    let mut visited = std::collections::HashSet::new();
    while let Some(next) = aliases.get(&current) {
        if !visited.insert(current.clone()) {
            break;
        }
        current.clone_from(next);
    }
    current
}

fn assert_runtime_invariants(
    state: &DaemonState,
    home: &std::path::Path,
    actors: &[&str],
    transports: &[&str],
) {
    for actor in actors {
        let canonical = canonical_actor(home, actor);
        let visible = state.visible_sessions(actor);
        for session in &visible {
            let owner = canonical_actor(home, &session.owner_actor);
            assert!(
                canonical == "local:default" || owner == canonical,
                "actor {actor} should only see sessions owned by {canonical}, got owner {owner}"
            );
        }

        if let Some(active) = state.active_actor_session(actor) {
            assert!(
                visible
                    .iter()
                    .any(|session| session.id.to_string() == active),
                "active session {active} for {actor} must remain visible"
            );
        }
    }

    for transport in transports {
        let transport_actor =
            ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(home))
                .transport_actors()
                .get(*transport)
                .cloned()
                .unwrap_or_else(|| "local:default".to_string());
        let direct: Vec<String> = state
            .visible_sessions_for_transport(transport)
            .into_iter()
            .map(|session| session.id.to_string())
            .collect();
        let via_actor: Vec<String> = state
            .visible_sessions(&transport_actor)
            .into_iter()
            .map(|session| session.id.to_string())
            .collect();
        assert_eq!(
            direct, via_actor,
            "transport {transport} should expose the same sessions as its bound actor"
        );
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
async fn alias_and_transport_matrix_reuses_sessions_for_one_canonical_actor() {
    let cases = [
        (
            vec![("telegram:alpha", "user:scott"), ("qq:beta", "user:scott")],
            vec![("http", "user:scott")],
            "telegram:alpha",
            vec!["qq:beta", "user:scott"],
            vec!["http"],
        ),
        (
            vec![
                ("telegram:alpha", "alias:tg"),
                ("alias:tg", "user:scott"),
                ("qq:beta", "alias:qq"),
                ("alias:qq", "user:scott"),
                ("alias:web", "user:scott"),
            ],
            vec![
                ("socket", "alias:web"),
                ("rpc", "user:scott"),
                ("stdio", "alias:web"),
            ],
            "telegram:alpha",
            vec!["qq:beta", "alias:tg", "alias:qq", "user:scott"],
            vec!["socket", "rpc", "stdio"],
        ),
    ];

    for (aliases, transports, owner_actor, linked_actors, linked_transports) in cases {
        let (_temp, _home, state) = build_state_with_bindings(&aliases, &transports).await;
        let (session_id, _) = state.create_session_for_actor(owner_actor);

        for actor in linked_actors {
            let resolved = state.resolve_actor_session(actor);
            assert_eq!(
                resolved, session_id,
                "actor {actor} should reuse shared session"
            );
        }

        for transport in linked_transports {
            let resolved = state.resolve_client_session(transport);
            assert_eq!(
                resolved, session_id,
                "transport {transport} should reuse shared session"
            );
        }

        assert_eq!(
            state.session_manager().list_sessions().len(),
            1,
            "matrix case should not allocate extra sessions"
        );
    }
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

    seed_pending_pair(&telegram_store, "5188621876", "Scott", "TG1234");
    seed_pending_pair(&qq_store, "bot-user", "ScottQQ", "QQ1234");
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
async fn revoke_blocks_channel_entry_and_repair_reuses_visible_session() {
    let (_temp, home, state) =
        build_state_with_bindings(&[("telegram:5188621876", "user:scott")], &[]).await;
    let state = Arc::new(state);
    let store = ChannelStore::open(&home, "telegram");

    seed_pending_pair(&store, "5188621876", "Scott", "TG1111");
    let _ = must(
        store.approve_pending_pair("5188621876"),
        "pair approval should succeed",
    );

    let session_id = state.resolve_actor_session("telegram:5188621876");
    assert_eq!(state.session_manager().list_sessions().len(), 1);

    assert!(store.revoke_pair("5188621876"));
    let denied = handle_message_events(
        &state,
        &store,
        "5188621876",
        "Scott",
        "hello",
        &[],
        "telegram",
    );
    assert_eq!(denied.len(), 1);
    match &denied[0] {
        BroadcastEvent::Done { response, .. } => {
            assert!(response.contains("requires pairing"));
        }
        other => panic!("expected pairing prompt after revoke, got {other:?}"),
    }

    seed_pending_pair(&store, "5188621876", "Scott", "TG2222");
    let _ = must(
        store.approve_pending_pair("5188621876"),
        "repair approval should succeed",
    );

    let repaired_session = state.resolve_actor_session("telegram:5188621876");
    assert_eq!(repaired_session, session_id);
}

#[tokio::test]
async fn subscription_toggle_does_not_change_actor_session_or_visibility() {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[],
    )
    .await;
    let telegram_store = ChannelStore::open(&home, "telegram");

    seed_pending_pair(&telegram_store, "5188621876", "Scott", "TG3333");
    let _ = must(
        telegram_store.approve_pending_pair("5188621876"),
        "pair approval should succeed",
    );

    let base_session = state.resolve_actor_session("telegram:5188621876");
    let visible_before: Vec<String> = state
        .visible_sessions("telegram:5188621876")
        .into_iter()
        .map(|session| session.id.to_string())
        .collect();
    let active_before = state.active_actor_session("telegram:5188621876");

    let enabled = must(
        telegram_store.set_pair_subscription("5188621876", true),
        "subscription enable should succeed",
    );
    assert!(enabled.subscribe);
    assert_eq!(
        state.active_actor_session("telegram:5188621876"),
        active_before
    );
    let visible_after_enable: Vec<String> = state
        .visible_sessions("telegram:5188621876")
        .into_iter()
        .map(|session| session.id.to_string())
        .collect();
    assert_eq!(visible_after_enable, visible_before);

    let disabled = must(
        telegram_store.set_pair_subscription("5188621876", false),
        "subscription disable should succeed",
    );
    assert!(!disabled.subscribe);
    assert_eq!(
        state.active_actor_session("telegram:5188621876"),
        Some(base_session)
    );
    let visible_after_disable: Vec<String> = state
        .visible_sessions("telegram:5188621876")
        .into_iter()
        .map(|session| session.id.to_string())
        .collect();
    assert_eq!(visible_after_disable, visible_before);
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
async fn active_session_switch_sequence_preserves_per_client_binding() {
    let (_temp, _home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[],
    )
    .await;

    let (session_a, _) = state
        .session_manager()
        .create_session_with_id_for_actor("shared-a", "user:scott");
    let (session_b, _) = state
        .session_manager()
        .create_session_with_id_for_actor("shared-b", "user:scott");
    let (session_c, _) = state
        .session_manager()
        .create_session_with_id_for_actor("shared-c", "user:scott");

    let session_a = session_a.to_string();
    let session_b = session_b.to_string();
    let session_c = session_c.to_string();

    let steps = [
        (
            "telegram:5188621876",
            session_a.as_str(),
            Some(session_a.as_str()),
            None,
        ),
        (
            "qq:bot-user",
            session_b.as_str(),
            Some(session_a.as_str()),
            Some(session_b.as_str()),
        ),
        (
            "telegram:5188621876",
            session_c.as_str(),
            Some(session_c.as_str()),
            Some(session_b.as_str()),
        ),
        (
            "qq:bot-user",
            session_a.as_str(),
            Some(session_c.as_str()),
            Some(session_a.as_str()),
        ),
    ];

    for (actor, target, expected_tg, expected_qq) in steps {
        state.set_actor_session(actor, target);
        assert_eq!(
            state.active_actor_session("telegram:5188621876").as_deref(),
            expected_tg
        );
        assert_eq!(
            state.active_actor_session("qq:bot-user").as_deref(),
            expected_qq
        );
    }
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
async fn ownership_model_sequence_preserves_runtime_invariants() {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[("http", "user:scott"), ("socket", "user:bob")],
    )
    .await;
    let state = Arc::new(state);
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let telegram_store = ChannelStore::open(&home, "telegram");
    let qq_store = ChannelStore::open(&home, "qq");
    let actors = [
        "telegram:5188621876",
        "qq:bot-user",
        "user:scott",
        "user:bob",
        "user:alex",
        "local:default",
    ];
    let transports = ["http", "socket"];

    seed_pending_pair(&telegram_store, "5188621876", "Scott", "SEQTG1");
    seed_pending_pair(&qq_store, "bot-user", "ScottQQ", "SEQQQ1");
    let _ = must(
        telegram_store.approve_pending_pair("5188621876"),
        "telegram pair approval should succeed",
    );
    let _ = must(
        qq_store.approve_pending_pair("bot-user"),
        "qq pair approval should succeed",
    );
    assert_runtime_invariants(&state, &home, &actors, &transports);

    let scott_session = state.resolve_actor_session("telegram:5188621876");
    let bob_session = state.resolve_client_session("socket");
    assert_ne!(scott_session, bob_session);
    assert_runtime_invariants(&state, &home, &actors, &transports);

    let _ = must(
        telegram_store.set_pair_subscription("5188621876", true),
        "subscription enable should succeed",
    );
    assert_runtime_invariants(&state, &home, &actors, &transports);

    assert!(telegram_store.revoke_pair("5188621876"));
    let denied = handle_message_events(
        &state,
        &telegram_store,
        "5188621876",
        "Scott",
        "hello",
        &[],
        "telegram",
    );
    assert_eq!(denied.len(), 1);
    assert_runtime_invariants(&state, &home, &actors, &transports);

    seed_pending_pair(&telegram_store, "5188621876", "Scott", "SEQTG2");
    let _ = must(
        telegram_store.approve_pending_pair("5188621876"),
        "telegram repair should succeed",
    );
    assert_eq!(
        state.resolve_actor_session("telegram:5188621876"),
        scott_session
    );
    assert_runtime_invariants(&state, &home, &actors, &transports);

    bindings.set_actor_alias("qq:bot-user", "user:alex");
    ReloadTarget::reload_config(&*state);
    let alex_session = state.resolve_actor_session("qq:bot-user");
    assert_ne!(alex_session, scott_session);
    assert_runtime_invariants(&state, &home, &actors, &transports);

    bindings.set_transport_actor("socket", "user:scott");
    ReloadTarget::reload_config(&*state);
    assert_eq!(state.resolve_client_session("socket"), scott_session);
    assert_runtime_invariants(&state, &home, &actors, &transports);
}

#[tokio::test]
async fn alias_rewrite_reload_moves_visibility_to_new_canonical_actor() {
    let (_temp, home, state) = build_state_with_bindings(
        &[("telegram:alpha", "user:scott"), ("qq:beta", "user:scott")],
        &[],
    )
    .await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let (shared_session, _) = state.create_session_for_actor("telegram:alpha");

    assert_eq!(state.resolve_actor_session("qq:beta"), shared_session);
    assert_eq!(state.visible_sessions("qq:beta").len(), 1);

    bindings.set_actor_alias("qq:beta", "user:alex");
    ReloadTarget::reload_config(&state);

    assert!(state.visible_sessions("qq:beta").is_empty());
    assert_eq!(state.visible_sessions("telegram:alpha").len(), 1);

    let alex_session = state.resolve_actor_session("qq:beta");
    assert_ne!(alex_session, shared_session);
    assert_eq!(state.visible_sessions("qq:beta").len(), 1);
    assert_eq!(state.session_manager().list_sessions().len(), 2);
}

#[tokio::test]
async fn transport_rebind_reload_updates_visible_sessions() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("http", "user:alice"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));

    let (alice_session, _) = state.create_session_for_actor("user:alice");
    let (bob_session, _) = state.create_session_for_actor("user:bob");

    let http_before = state.visible_sessions_for_transport("http");
    assert_eq!(http_before.len(), 1);
    assert_eq!(http_before[0].id.to_string(), alice_session);

    bindings.set_transport_actor("http", "user:bob");
    ReloadTarget::reload_config(&state);

    let http_after = state.visible_sessions_for_transport("http");
    assert_eq!(http_after.len(), 1);
    assert_eq!(http_after[0].id.to_string(), bob_session);
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
