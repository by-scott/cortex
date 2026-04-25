use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

use cortex_kernel::{ActorBindingsStore, AuditEntry, AuditLog, CortexPaths, TaskStore};
use cortex_types::{MemoryEntry, MemoryKind, MemoryType, SharedTask, SharedTaskStatus};
use proptest::prelude::*;
use tempfile::TempDir;

use crate::channels::store::ChannelStore;
use crate::channels::{ChannelSlashAction, handle_message_events, resolve_channel_slash};
use crate::daemon::DaemonState;
use crate::daemon::{BroadcastEvent, BroadcastMessage};
use crate::hot_reload::ReloadTarget;
use crate::runtime::CortexRuntime;

struct SequenceRng {
    state: u64,
}

impl SequenceRng {
    const fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next(&mut self) -> u64 {
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.state
    }

    fn choose(&mut self, upper: u64) -> u64 {
        self.next() % upper
    }
}

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

fn ensure_pair(store: &ChannelStore, user_id: &str, user_name: &str, code: &str) {
    if store.is_paired(user_id) {
        return;
    }
    seed_pending_pair(store, user_id, user_name, code);
    let _ = must(
        store.approve_pending_pair(user_id),
        "pair approval should succeed",
    );
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

fn assert_revoke_requires_pairing(
    state: &Arc<DaemonState>,
    store: &ChannelStore,
    user_id: &str,
    user_name: &str,
    platform: &str,
) {
    let events = handle_message_events(state, store, user_id, user_name, "hello", &[], platform);
    assert!(
        events.iter().any(|event| matches!(
            event,
            BroadcastEvent::Done { response, .. }
                if response.contains("requires pairing")
                    || response.contains("already pending")
        )),
        "revoked {platform} user should be forced back through pairing"
    );
}

fn run_pairing_sequence_step(
    step: u64,
    state: &Arc<DaemonState>,
    telegram_store: &ChannelStore,
    qq_store: &ChannelStore,
) {
    match step {
        0 => {
            let _ = state.resolve_actor_session("telegram:5188621876");
        }
        1 => {
            let _ = state.resolve_actor_session("qq:bot-user");
        }
        2 => {
            if telegram_store.is_paired("5188621876") {
                let _ = must(
                    telegram_store.set_pair_subscription("5188621876", true),
                    "telegram subscribe should succeed",
                );
            }
        }
        3 => {
            if telegram_store.is_paired("5188621876") {
                let _ = must(
                    telegram_store.set_pair_subscription("5188621876", false),
                    "telegram unsubscribe should succeed",
                );
            }
        }
        4 => {
            if qq_store.is_paired("bot-user") {
                let _ = must(
                    qq_store.set_pair_subscription("bot-user", true),
                    "qq subscribe should succeed",
                );
            }
        }
        5 => {
            if qq_store.is_paired("bot-user") {
                let _ = must(
                    qq_store.set_pair_subscription("bot-user", false),
                    "qq unsubscribe should succeed",
                );
            }
        }
        6 => {
            let _ = telegram_store.revoke_pair("5188621876");
            assert_revoke_requires_pairing(
                state,
                telegram_store,
                "5188621876",
                "Scott",
                "telegram",
            );
        }
        7 => {
            ensure_pair(telegram_store, "5188621876", "Scott", "TGRNG2");
        }
        8 => {
            let _ = qq_store.revoke_pair("bot-user");
            assert!(!qq_store.is_paired("bot-user"));
        }
        9 => {
            ensure_pair(qq_store, "bot-user", "ScottQQ", "QQRNG2");
        }
        10 => {
            let _ = state.resolve_client_session("http");
        }
        11 => {
            let _ = state.resolve_client_session("socket");
        }
        _ => unreachable!("rng step out of range"),
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

fn open_task_store(home: &std::path::Path) -> TaskStore {
    let data_dir = CortexPaths::from_instance_home(home).data_dir();
    must(
        fs::create_dir_all(&data_dir),
        "task data dir should initialize",
    );
    must(
        TaskStore::open(&data_dir.join("tasks.db")),
        "task store should initialize",
    )
}

fn open_audit_log(home: &std::path::Path) -> AuditLog {
    let data_dir = CortexPaths::from_instance_home(home).data_dir();
    must(
        fs::create_dir_all(&data_dir),
        "audit data dir should initialize",
    );
    must(
        AuditLog::open(&data_dir.join("audit.db")),
        "audit log should initialize",
    )
}

struct StoreSequenceHarness<'a> {
    state: &'a DaemonState,
    bindings: &'a ActorBindingsStore,
    task_store: &'a TaskStore,
    audit_log: &'a AuditLog,
    memory_counts: std::collections::BTreeMap<String, usize>,
    task_counts: std::collections::BTreeMap<String, usize>,
    audit_counts: std::collections::BTreeMap<String, usize>,
}

impl<'a> StoreSequenceHarness<'a> {
    fn new(
        state: &'a DaemonState,
        bindings: &'a ActorBindingsStore,
        task_store: &'a TaskStore,
        audit_log: &'a AuditLog,
    ) -> Self {
        Self {
            state,
            bindings,
            task_store,
            audit_log,
            memory_counts: std::collections::BTreeMap::new(),
            task_counts: std::collections::BTreeMap::new(),
            audit_counts: std::collections::BTreeMap::new(),
        }
    }

    fn transport_for_idx(idx: u64) -> &'static str {
        if idx.is_multiple_of(2) {
            "http"
        } else {
            "socket"
        }
    }

    fn run_step(&mut self, choice: u64, idx: u64) {
        match choice {
            0 | 1 => {
                let transport = Self::transport_for_idx(idx);
                let owner = self.state.transport_actor(transport);
                let mut memory = MemoryEntry::new(
                    format!("{transport}-memory-{idx}"),
                    format!("memory from {transport}"),
                    MemoryType::Project,
                    MemoryKind::Semantic,
                );
                memory.owner_actor = owner.clone();
                must(
                    self.state.memory_store().save(&memory),
                    "transport-owned memory should save",
                );
                *self.memory_counts.entry(owner).or_insert(0) += 1;
            }
            2 | 3 => {
                let transport = Self::transport_for_idx(idx);
                let owner = self.state.transport_actor(transport);
                let mut task = SharedTask::new(format!("{transport}-task-{idx}"));
                task.owner_actor = owner.clone();
                task.status = SharedTaskStatus::Pending;
                must(
                    self.task_store.save(&task),
                    "transport-owned task should save",
                );
                *self.task_counts.entry(owner).or_insert(0) += 1;
            }
            4 | 5 => {
                let transport = Self::transport_for_idx(idx);
                let owner = self.state.transport_actor(transport);
                let entry = AuditEntry::tool_execution(
                    format!("{transport}-session-{idx}"),
                    "inspect",
                    "sequence",
                    "ok",
                )
                .with_owner_actor(owner.clone());
                must(
                    self.audit_log.append(&entry),
                    "transport-owned audit should append",
                );
                *self.audit_counts.entry(owner).or_insert(0) += 1;
            }
            6 => {
                self.bindings.set_transport_actor("http", "user:scott");
                ReloadTarget::reload_config(self.state);
            }
            7 => {
                self.bindings.set_transport_actor("http", "user:bob");
                ReloadTarget::reload_config(self.state);
            }
            8 => {
                self.bindings.set_transport_actor("socket", "user:bob");
                ReloadTarget::reload_config(self.state);
            }
            9 => {
                self.bindings.set_transport_actor("socket", "user:scott");
                ReloadTarget::reload_config(self.state);
            }
            _ => unreachable!("rng.choose(10) returned out-of-range value"),
        }
    }
}

fn assert_store_visibility_counts(
    state: &DaemonState,
    home: &std::path::Path,
    task_store: &TaskStore,
    audit_log: &AuditLog,
    memory_counts: &std::collections::BTreeMap<String, usize>,
    task_counts: &std::collections::BTreeMap<String, usize>,
    audit_counts: &std::collections::BTreeMap<String, usize>,
) {
    let total_memory: usize = memory_counts.values().sum();
    let total_tasks: usize = task_counts.values().sum();
    let total_audit: usize = audit_counts.values().sum();

    for actor in ["user:scott", "user:bob", "local:default"] {
        let canonical = canonical_actor(home, actor);
        let expected_memory = if canonical == "local:default" {
            total_memory
        } else {
            *memory_counts.get(&canonical).unwrap_or(&0)
        };
        let expected_tasks = if canonical == "local:default" {
            total_tasks
        } else {
            *task_counts.get(&canonical).unwrap_or(&0)
        };
        let expected_audit = if canonical == "local:default" {
            total_audit
        } else {
            *audit_counts.get(&canonical).unwrap_or(&0)
        };

        let memories = must(
            state.memory_store().list_for_actor(actor),
            "actor memories should load",
        );
        let tasks = must(
            task_store.list_by_status_for_actor(SharedTaskStatus::Pending, actor),
            "actor tasks should load",
        );
        let audit = must(audit_log.query_by_actor(actor), "actor audit should load");

        assert_eq!(
            memories.len(),
            expected_memory,
            "memory ownership drifted for {actor}"
        );
        assert_eq!(
            tasks.len(),
            expected_tasks,
            "task ownership drifted for {actor}"
        );
        assert_eq!(
            audit.len(),
            expected_audit,
            "audit ownership drifted for {actor}"
        );
    }
}

fn assert_full_runtime_and_store_invariants(
    state: &DaemonState,
    home: &std::path::Path,
    actors: &[&str],
    transports: &[&str],
    task_store: &TaskStore,
    audit_log: &AuditLog,
    harness: &StoreSequenceHarness<'_>,
) {
    assert_runtime_invariants(state, home, actors, transports);
    assert_store_visibility_counts(
        state,
        home,
        task_store,
        audit_log,
        &harness.memory_counts,
        &harness.task_counts,
        &harness.audit_counts,
    );
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
async fn ws_and_stdio_reuse_visible_sessions_for_same_bound_actor() {
    let (_temp, _home, state) =
        build_state_with_bindings(&[], &[("ws", "user:scott"), ("stdio", "user:scott")]).await;

    let shared_session = state.resolve_actor_session("user:scott");
    assert_eq!(state.resolve_client_session("ws"), shared_session);
    assert_eq!(state.resolve_client_session("stdio"), shared_session);
}

#[tokio::test]
async fn socket_family_transports_reject_hidden_sessions() {
    let (_temp, _home, state) =
        build_state_with_bindings(&[], &[("ws", "user:scott"), ("socket", "user:bob")]).await;

    let scott_session = state.resolve_client_session("ws");
    let bob_session = state.resolve_client_session("socket");
    assert_ne!(scott_session, bob_session);

    assert!(state.transport_can_access_session("ws", &scott_session));
    assert!(!state.transport_can_access_session("ws", &bob_session));
    assert!(state.transport_can_access_session("socket", &bob_session));
    assert!(!state.transport_can_access_session("socket", &scott_session));
}

#[tokio::test]
async fn ws_transport_rebind_switches_new_resolution_without_relabeling_old_session() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("ws", "user:scott"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));

    let scott_session = state.resolve_client_session("ws");
    let bob_session = state.resolve_client_session("socket");
    assert_ne!(scott_session, bob_session);

    bindings.set_transport_actor("ws", "user:bob");
    ReloadTarget::reload_config(&state);

    assert_eq!(state.resolve_client_session("ws"), bob_session);
    assert!(state.transport_can_access_session("ws", &bob_session));
    assert!(!state.transport_can_access_session("ws", &scott_session));
    assert_eq!(
        state
            .visible_sessions_for_transport("ws")
            .into_iter()
            .map(|session| session.id.to_string())
            .collect::<Vec<_>>(),
        vec![bob_session]
    );
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

async fn run_seeded_actor_binding_sequence(seed: u64) {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[("http", "user:scott"), ("socket", "user:bob")],
    )
    .await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let state = Arc::new(state);
    let actors = [
        "telegram:5188621876",
        "qq:bot-user",
        "user:scott",
        "user:bob",
        "user:alex",
        "local:default",
    ];
    let transports = ["http", "socket"];
    let mut rng = SequenceRng::new(seed);

    for _ in 0..64 {
        match rng.choose(10) {
            0 => {
                let _ = state.resolve_actor_session("telegram:5188621876");
            }
            1 => {
                let _ = state.resolve_actor_session("qq:bot-user");
            }
            2 => {
                let _ = state.resolve_client_session("http");
            }
            3 => {
                let _ = state.resolve_client_session("socket");
            }
            4 => {
                bindings.set_actor_alias("qq:bot-user", "user:scott");
                ReloadTarget::reload_config(&*state);
            }
            5 => {
                bindings.set_actor_alias("qq:bot-user", "user:alex");
                ReloadTarget::reload_config(&*state);
            }
            6 => {
                bindings.set_transport_actor("socket", "user:scott");
                ReloadTarget::reload_config(&*state);
            }
            7 => {
                bindings.set_transport_actor("socket", "user:bob");
                ReloadTarget::reload_config(&*state);
            }
            8 => {
                let session = state.resolve_actor_session("telegram:5188621876");
                state.set_actor_session("telegram:5188621876", &session);
            }
            9 => {
                let session = state.resolve_actor_session("qq:bot-user");
                state.set_actor_session("qq:bot-user", &session);
            }
            _ => unreachable!("rng.choose(10) returned out-of-range value"),
        }

        assert_runtime_invariants(&state, &home, &actors, &transports);
    }
}

#[tokio::test]
async fn seeded_actor_binding_sequence_preserves_runtime_invariants() {
    run_seeded_actor_binding_sequence(0xC0A1_57A7_2026_0424).await;
}

#[tokio::test]
async fn actor_binding_sequence_preserves_runtime_invariants_across_multiple_seeds() {
    let seeds = [
        0xC0A1_57A7_2026_0424,
        0xA17A_0B1D_2026_0425,
        0xB17D_1A55_2026_0425,
    ];

    for seed in seeds {
        run_seeded_actor_binding_sequence(seed).await;
    }
}

async fn run_seeded_pairing_and_subscription_sequence(seed: u64) {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[("http", "user:scott"), ("socket", "user:bob")],
    )
    .await;
    let state = Arc::new(state);
    let telegram_store = ChannelStore::open(&home, "telegram");
    let qq_store = ChannelStore::open(&home, "qq");
    let actors = [
        "telegram:5188621876",
        "qq:bot-user",
        "user:scott",
        "user:bob",
        "local:default",
    ];
    let transports = ["http", "socket"];
    let mut rng = SequenceRng::new(seed);

    ensure_pair(&telegram_store, "5188621876", "Scott", "TGRNG1");
    ensure_pair(&qq_store, "bot-user", "ScottQQ", "QQRNG1");

    for _ in 0..48 {
        run_pairing_sequence_step(rng.choose(12), &state, &telegram_store, &qq_store);

        assert_runtime_invariants(&state, &home, &actors, &transports);
    }
}

#[tokio::test]
async fn seeded_pairing_and_subscription_sequence_preserves_runtime_invariants() {
    run_seeded_pairing_and_subscription_sequence(0x51A5_7E55_10AA_2026).await;
}

#[tokio::test]
async fn pairing_and_subscription_sequence_preserves_runtime_invariants_across_multiple_seeds() {
    let seeds = [
        0x51A5_7E55_10AA_2026,
        0xFA11_5123_2026_0425,
        0x5A85_C21B_2026_0425,
    ];

    for seed in seeds {
        run_seeded_pairing_and_subscription_sequence(seed).await;
    }
}

#[tokio::test]
async fn seeded_transport_owned_store_sequence_preserves_actor_visibility() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("http", "user:scott"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let task_store = open_task_store(&home);
    let audit_log = open_audit_log(&home);
    let mut rng = SequenceRng::new(0x5700_1234_ABCD_2026);
    let mut harness = StoreSequenceHarness::new(&state, &bindings, &task_store, &audit_log);

    for idx in 0..64_u64 {
        harness.run_step(rng.choose(10), idx);
        assert_store_visibility_counts(
            &state,
            &home,
            &task_store,
            &audit_log,
            &harness.memory_counts,
            &harness.task_counts,
            &harness.audit_counts,
        );
    }
}

async fn run_seeded_end_to_end_ownership_sequence(seed: u64) {
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
    let task_store = open_task_store(&home);
    let audit_log = open_audit_log(&home);
    let mut harness = StoreSequenceHarness::new(&state, &bindings, &task_store, &audit_log);
    let actors = [
        "telegram:5188621876",
        "qq:bot-user",
        "user:scott",
        "user:bob",
        "user:alex",
        "local:default",
    ];
    let transports = ["http", "socket"];
    let mut rng = SequenceRng::new(seed);

    ensure_pair(&telegram_store, "5188621876", "Scott", "SEQTG3");
    ensure_pair(&qq_store, "bot-user", "ScottQQ", "SEQQQ3");

    for idx in 0..96_u64 {
        match rng.choose(16) {
            0 => {
                let _ = state.resolve_actor_session("telegram:5188621876");
            }
            1 => {
                let _ = state.resolve_actor_session("qq:bot-user");
            }
            2 => {
                let _ = state.resolve_client_session("http");
            }
            3 => {
                let _ = state.resolve_client_session("socket");
            }
            4 => {
                bindings.set_actor_alias("qq:bot-user", "user:scott");
                ReloadTarget::reload_config(&*state);
            }
            5 => {
                bindings.set_actor_alias("qq:bot-user", "user:alex");
                ReloadTarget::reload_config(&*state);
            }
            6 => {
                bindings.set_transport_actor("socket", "user:scott");
                ReloadTarget::reload_config(&*state);
            }
            7 => {
                bindings.set_transport_actor("socket", "user:bob");
                ReloadTarget::reload_config(&*state);
            }
            8 => {
                let _ = telegram_store.set_pair_subscription("5188621876", true);
            }
            9 => {
                let _ = telegram_store.set_pair_subscription("5188621876", false);
            }
            10 => {
                let _ = qq_store.set_pair_subscription("bot-user", true);
            }
            11 => {
                let _ = qq_store.set_pair_subscription("bot-user", false);
            }
            12 => {
                let _ = telegram_store.revoke_pair("5188621876");
                ensure_pair(&telegram_store, "5188621876", "Scott", "SEQTG4");
            }
            13 => {
                let _ = qq_store.revoke_pair("bot-user");
                ensure_pair(&qq_store, "bot-user", "ScottQQ", "SEQQQ4");
            }
            14 | 15 => {
                harness.run_step(rng.choose(10), idx);
            }
            _ => unreachable!("rng.choose(16) returned out-of-range value"),
        }

        assert_full_runtime_and_store_invariants(
            &state,
            &home,
            &actors,
            &transports,
            &task_store,
            &audit_log,
            &harness,
        );
    }
}

#[tokio::test]
async fn seeded_end_to_end_ownership_sequence_preserves_invariants() {
    run_seeded_end_to_end_ownership_sequence(0x1357_2468_2026_0425).await;
}

#[tokio::test]
async fn end_to_end_ownership_sequence_preserves_invariants_across_multiple_seeds() {
    let seeds = [
        0x1357_2468_2026_0425,
        0xA11A_5EED_2026_0425,
        0x5C07_7BAD_2026_0425,
        0xFEED_C0DE_2026_0425,
    ];

    for seed in seeds {
        run_seeded_end_to_end_ownership_sequence(seed).await;
    }
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
async fn transport_bound_actor_owns_runtime_memories() {
    let (_temp, home, state) = build_state_with_bindings(
        &[("telegram:5188621876", "user:scott")],
        &[("http", "user:scott"), ("socket", "user:bob")],
    )
    .await;

    let mut http_memory = MemoryEntry::new(
        "http note",
        "saved over http",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    http_memory.owner_actor = state.transport_actor("http");

    let mut socket_memory = MemoryEntry::new(
        "socket note",
        "saved over socket",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    socket_memory.owner_actor = state.transport_actor("socket");

    must(
        state.memory_store().save(&http_memory),
        "http-owned memory should save",
    );
    must(
        state.memory_store().save(&socket_memory),
        "socket-owned memory should save",
    );

    let scott_memories = must(
        state.memory_store().list_for_actor("user:scott"),
        "scott memories should load",
    );
    let bob_memories = must(
        state.memory_store().list_for_actor("user:bob"),
        "bob memories should load",
    );
    let telegram_memories = must(
        state
            .memory_store()
            .list_for_actor(&canonical_actor(&home, "telegram:5188621876")),
        "telegram canonical actor memories should load",
    );
    let admin_memories = must(
        state.memory_store().list_for_actor("local:default"),
        "admin memories should load",
    );

    assert_eq!(scott_memories.len(), 1);
    assert_eq!(scott_memories[0].owner_actor, "user:scott");
    assert_eq!(telegram_memories.len(), 1);
    assert_eq!(telegram_memories[0].owner_actor, "user:scott");
    assert_eq!(bob_memories.len(), 1);
    assert_eq!(bob_memories[0].owner_actor, "user:bob");
    assert_eq!(admin_memories.len(), 2);
}

#[tokio::test]
async fn transport_rebind_changes_new_memory_ownership_without_relabeling_old_memories() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("http", "user:alice"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));

    let mut first = MemoryEntry::new(
        "first http note",
        "before rebind",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    first.owner_actor = state.transport_actor("http");
    must(
        state.memory_store().save(&first),
        "first memory should save",
    );

    bindings.set_transport_actor("http", "user:bob");
    ReloadTarget::reload_config(&state);

    let mut second = MemoryEntry::new(
        "second http note",
        "after rebind",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    second.owner_actor = state.transport_actor("http");
    must(
        state.memory_store().save(&second),
        "second memory should save",
    );

    let alice_memories = must(
        state.memory_store().list_for_actor("user:alice"),
        "alice memories should load",
    );
    let bob_memories = must(
        state.memory_store().list_for_actor("user:bob"),
        "bob memories should load",
    );

    assert_eq!(alice_memories.len(), 1);
    assert_eq!(alice_memories[0].description, "before rebind");
    assert_eq!(alice_memories[0].owner_actor, "user:alice");

    assert_eq!(bob_memories.len(), 1);
    assert_eq!(bob_memories[0].description, "after rebind");
    assert_eq!(bob_memories[0].owner_actor, "user:bob");
}

#[tokio::test]
async fn transport_bound_actor_owns_runtime_tasks() {
    let (_temp, home, state) = build_state_with_bindings(
        &[("telegram:5188621876", "user:scott")],
        &[("http", "user:scott"), ("socket", "user:bob")],
    )
    .await;
    let task_dir = CortexPaths::from_instance_home(&home).data_dir();
    must(
        fs::create_dir_all(&task_dir),
        "task data dir should initialize",
    );
    let store = must(
        TaskStore::open(&task_dir.join("tasks.db")),
        "task store should initialize",
    );

    let mut http_task = SharedTask::new("http-owned task");
    http_task.owner_actor = state.transport_actor("http");
    http_task.status = SharedTaskStatus::Pending;

    let mut socket_task = SharedTask::new("socket-owned task");
    socket_task.owner_actor = state.transport_actor("socket");
    socket_task.status = SharedTaskStatus::Pending;

    must(store.save(&http_task), "http-owned task should save");
    must(store.save(&socket_task), "socket-owned task should save");

    let scott_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "user:scott"),
        "scott tasks should load",
    );
    let bob_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "user:bob"),
        "bob tasks should load",
    );
    let telegram_tasks = must(
        store.list_by_status_for_actor(
            SharedTaskStatus::Pending,
            &canonical_actor(&home, "telegram:5188621876"),
        ),
        "telegram canonical actor tasks should load",
    );
    let admin_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "local:default"),
        "admin tasks should load",
    );

    assert_eq!(scott_tasks.len(), 1);
    assert_eq!(scott_tasks[0].owner_actor, "user:scott");
    assert_eq!(telegram_tasks.len(), 1);
    assert_eq!(telegram_tasks[0].owner_actor, "user:scott");
    assert_eq!(bob_tasks.len(), 1);
    assert_eq!(bob_tasks[0].owner_actor, "user:bob");
    assert_eq!(admin_tasks.len(), 2);
}

#[tokio::test]
async fn transport_rebind_changes_new_task_ownership_without_relabeling_old_tasks() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("http", "user:alice"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let task_dir = CortexPaths::from_instance_home(&home).data_dir();
    must(
        fs::create_dir_all(&task_dir),
        "task data dir should initialize",
    );
    let store = must(
        TaskStore::open(&task_dir.join("tasks.db")),
        "task store should initialize",
    );

    let mut first = SharedTask::new("first http task");
    first.owner_actor = state.transport_actor("http");
    first.status = SharedTaskStatus::Pending;
    must(store.save(&first), "first task should save");

    bindings.set_transport_actor("http", "user:bob");
    ReloadTarget::reload_config(&state);

    let mut second = SharedTask::new("second http task");
    second.owner_actor = state.transport_actor("http");
    second.status = SharedTaskStatus::Pending;
    must(store.save(&second), "second task should save");

    let alice_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "user:alice"),
        "alice tasks should load",
    );
    let bob_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "user:bob"),
        "bob tasks should load",
    );

    assert_eq!(alice_tasks.len(), 1);
    assert_eq!(alice_tasks[0].description, "first http task");
    assert_eq!(alice_tasks[0].owner_actor, "user:alice");

    assert_eq!(bob_tasks.len(), 1);
    assert_eq!(bob_tasks[0].description, "second http task");
    assert_eq!(bob_tasks[0].owner_actor, "user:bob");
}

#[tokio::test]
async fn transport_rebind_changes_new_audit_ownership_without_relabeling_old_entries() {
    let (_temp, home, state) =
        build_state_with_bindings(&[], &[("http", "user:alice"), ("socket", "user:bob")]).await;
    let bindings = ActorBindingsStore::from_paths(&CortexPaths::from_instance_home(&home));
    let audit_dir = CortexPaths::from_instance_home(&home).data_dir();
    must(
        fs::create_dir_all(&audit_dir),
        "audit data dir should initialize",
    );
    let log = must(
        AuditLog::open(&audit_dir.join("audit.db")),
        "audit log should initialize",
    );

    let first = AuditEntry::tool_execution("alice-session", "read", "before rebind", "ok")
        .with_owner_actor(state.transport_actor("http"));
    must(log.append(&first), "first audit entry should append");

    bindings.set_transport_actor("http", "user:bob");
    ReloadTarget::reload_config(&state);

    let second = AuditEntry::permission_decision("bob-session", "write", "after rebind", "ok")
        .with_owner_actor(state.transport_actor("http"));
    must(log.append(&second), "second audit entry should append");

    let alice_entries = must(
        log.query_by_actor("user:alice"),
        "alice audit entries should load",
    );
    let bob_entries = must(
        log.query_by_actor("user:bob"),
        "bob audit entries should load",
    );

    assert_eq!(alice_entries.len(), 1);
    assert_eq!(alice_entries[0].owner_actor, "user:alice");
    assert_eq!(alice_entries[0].action, "before rebind");

    assert_eq!(bob_entries.len(), 1);
    assert_eq!(bob_entries[0].owner_actor, "user:bob");
    assert_eq!(bob_entries[0].action, "after rebind");
}

async fn run_generated_pairing_sequence(steps: &[u8]) {
    let (_temp, home, state) = build_state_with_bindings(
        &[
            ("telegram:5188621876", "user:scott"),
            ("qq:bot-user", "user:scott"),
        ],
        &[
            ("telegram", "telegram:5188621876"),
            ("qq", "qq:bot-user"),
            ("http", "user:scott"),
            ("socket", "user:scott"),
        ],
    )
    .await;
    let state = Arc::new(state);
    let telegram_store = ChannelStore::open(&home, "telegram");
    let qq_store = ChannelStore::open(&home, "qq");
    ensure_pair(&telegram_store, "5188621876", "Scott", "TGPROP");
    ensure_pair(&qq_store, "bot-user", "ScottQQ", "QQPROP");

    for step in steps {
        run_pairing_sequence_step(u64::from(*step % 12), &state, &telegram_store, &qq_store);
        assert_runtime_invariants(
            &state,
            &home,
            &[
                "user:scott",
                "telegram:5188621876",
                "qq:bot-user",
                "local:default",
            ],
            &["telegram", "qq", "http", "socket"],
        );
    }
}

proptest! {
    #![proptest_config(ProptestConfig {
        cases: 16,
        .. ProptestConfig::default()
    })]

    #[test]
    fn generated_pairing_and_subscription_sequences_preserve_runtime_invariants(
        steps in prop::collection::vec(any::<u8>(), 1..40)
    ) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap_or_else(|err| panic!("tokio runtime should initialize: {err}"));
        runtime.block_on(run_generated_pairing_sequence(&steps));
    }
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
