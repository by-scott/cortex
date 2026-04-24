use cortex_kernel::{
    AuditEntry, AuditEventType, AuditLog, EmbeddingStore, Journal, JournalSideEffectProvider,
    MemoryStore, TaskStore,
};
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryType, Payload, SharedTask,
    SharedTaskStatus, SideEffectKind, TurnId,
};

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn journal_replay_digest_is_stable_after_reopen() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let db = temp.path().join("journal.db");
    let turn = TurnId::new();
    let correlation = CorrelationId::new();

    {
        let journal = match Journal::open(&db) {
            Ok(value) => value,
            Err(err) => panic!("open journal should succeed: {err}"),
        };
        must(
            journal.append(&Event::new(turn, correlation, Payload::TurnStarted)),
            "append start should succeed",
        );
        journal
            .append(&Event::new(
                turn,
                correlation,
                Payload::SideEffectRecorded {
                    kind: SideEffectKind::ExternalIo,
                    key: "tool:read".to_string(),
                    value: "recorded".to_string(),
                },
            ))
            .map_or_else(
                |err| panic!("append side effect should succeed: {err}"),
                |_offset| (),
            );
    }

    let journal = must(Journal::open(&db), "reopen journal should succeed");
    let events = must(journal.recent_events(10), "recent events should succeed");
    let mut first_provider = JournalSideEffectProvider::from_events(&events);
    let mut second_provider = JournalSideEffectProvider::from_events(&events);
    assert_eq!(
        cortex_kernel::replay::replay_determinism_digest(&events, &mut first_provider),
        cortex_kernel::replay::replay_determinism_digest(&events, &mut second_provider)
    );
}

#[test]
fn actor_scoped_memory_store_filters_non_admin_actors() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let store = must(
        MemoryStore::open(temp.path()),
        "open memory store should succeed",
    );
    let mut own = MemoryEntry::new("alpha", "own", MemoryType::Project, MemoryKind::Semantic);
    own.owner_actor = "telegram:1".to_string();
    let mut other = MemoryEntry::new("beta", "other", MemoryType::Project, MemoryKind::Semantic);
    other.owner_actor = "telegram:2".to_string();
    must(store.save(&own), "save own should succeed");
    must(store.save(&other), "save other should succeed");

    assert_eq!(
        match store.list_for_actor("telegram:1") {
            Ok(value) => value.len(),
            Err(err) => panic!("list actor should succeed: {err}"),
        },
        1
    );
    assert_eq!(
        match store.list_for_actor("local:default") {
            Ok(value) => value.len(),
            Err(err) => panic!("list admin should succeed: {err}"),
        },
        2
    );

    let loaded = must(
        store.load_for_actor(&own.id, "telegram:1"),
        "owner should load own memory",
    );
    assert_eq!(loaded.owner_actor, "telegram:1");
    assert!(
        store.load_for_actor(&other.id, "telegram:1").is_err(),
        "non-owner should not load another actor's memory"
    );

    must(
        store.delete_for_actor(&own.id, "telegram:1"),
        "owner should delete own memory",
    );
    assert!(
        store.delete_for_actor(&other.id, "telegram:1").is_err(),
        "non-owner should not delete another actor's memory"
    );
}

#[test]
fn actor_scoped_task_store_filters_load_list_and_delete() {
    let store = must(TaskStore::in_memory(), "open task store should succeed");
    let mut own = SharedTask::new("own task");
    own.owner_actor = "telegram:1".to_string();
    own.status = SharedTaskStatus::Pending;
    let mut other = SharedTask::new("other task");
    other.owner_actor = "telegram:2".to_string();
    other.status = SharedTaskStatus::Pending;

    must(store.save(&own), "save own task should succeed");
    must(store.save(&other), "save other task should succeed");

    let actor_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "telegram:1"),
        "actor task list should succeed",
    );
    assert_eq!(actor_tasks.len(), 1);
    assert_eq!(actor_tasks[0].owner_actor, "telegram:1");

    let admin_tasks = must(
        store.list_by_status_for_actor(SharedTaskStatus::Pending, "local:default"),
        "admin task list should succeed",
    );
    assert_eq!(admin_tasks.len(), 2);

    let loaded = must(
        store.load_for_actor(&own.id, "telegram:1"),
        "owner should load own task",
    );
    assert_eq!(loaded.owner_actor, "telegram:1");
    assert!(
        store.load_for_actor(&other.id, "telegram:1").is_err(),
        "non-owner should not load another actor's task"
    );

    assert!(
        must(
            store.delete_for_actor(&own.id, "telegram:1"),
            "owner should delete own task",
        ),
        "delete_for_actor should report removed own task"
    );
    assert!(
        store.delete_for_actor(&other.id, "telegram:1").is_err(),
        "non-owner should not delete another actor's task"
    );
}

#[test]
fn actor_scoped_audit_log_filters_query_surface() {
    let log = must(AuditLog::in_memory(), "open audit log should succeed");
    let own = AuditEntry::tool_execution("session-own", "read", "load", "ok")
        .with_owner_actor("telegram:1");
    let other = AuditEntry::permission_decision("session-other", "write", "confirm", "denied")
        .with_owner_actor("telegram:2");

    must(log.append(&own), "append own audit entry should succeed");
    must(
        log.append(&other),
        "append other audit entry should succeed",
    );

    let actor_entries = must(
        log.query_by_actor("telegram:1"),
        "actor audit query should succeed",
    );
    assert_eq!(actor_entries.len(), 1);
    assert_eq!(actor_entries[0].owner_actor, "telegram:1");
    assert_eq!(actor_entries[0].event_type, AuditEventType::ToolExecution);

    let admin_entries = must(
        log.query_by_actor("local:default"),
        "admin audit query should succeed",
    );
    assert_eq!(admin_entries.len(), 2);
}

#[test]
fn embedding_vectors_inherit_visibility_through_memory_ids() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let memory_store = must(
        MemoryStore::open(&temp.path().join("memory")),
        "open memory store should succeed",
    );
    let embedding_store = must(
        EmbeddingStore::open(&temp.path().join("embeddings.db")),
        "open embedding store should succeed",
    );
    must(
        embedding_store.ensure_vector_table(2),
        "vector table should initialize",
    );

    let mut own = MemoryEntry::new(
        "actor-owned embedding",
        "own embedding",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    own.owner_actor = "telegram:1".to_string();
    let mut other = MemoryEntry::new(
        "other embedding",
        "other embedding",
        MemoryType::Project,
        MemoryKind::Semantic,
    );
    other.owner_actor = "telegram:2".to_string();
    must(memory_store.save(&own), "save own memory should succeed");
    must(
        memory_store.save(&other),
        "save other memory should succeed",
    );

    must(
        embedding_store.upsert_vector(&own.id, &[1.0, 0.0]),
        "upsert own vector should succeed",
    );
    must(
        embedding_store.upsert_vector(&other.id, &[0.0, 1.0]),
        "upsert other vector should succeed",
    );

    let hits = embedding_store.search_vectors(&[1.0, 0.0], 10);
    assert_eq!(
        hits.len(),
        2,
        "vector store should return ids without actor metadata"
    );

    let visible_to_actor: Vec<String> = hits
        .iter()
        .filter_map(|(memory_id, _distance)| {
            memory_store
                .load_for_actor(memory_id, "telegram:1")
                .ok()
                .map(|entry| entry.id)
        })
        .collect();

    assert_eq!(visible_to_actor, vec![own.id]);
    assert!(
        memory_store
            .load_for_actor(&other.id, "telegram:1")
            .is_err(),
        "embedding lookup must not bypass actor-scoped memory visibility"
    );
}
