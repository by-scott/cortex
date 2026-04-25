use chrono::Utc;
use cortex_kernel::{AuditEventType, AuditLog, TaskStore};

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn audit_log_defaults_legacy_rows_to_local_default_owner() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let db = temp.path().join("audit.db");
    let conn = match rusqlite::Connection::open(&db) {
        Ok(value) => value,
        Err(err) => panic!("sqlite connection should open: {err}"),
    };
    if let Err(err) = conn.execute_batch(
        "CREATE TABLE audit_entries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp TEXT NOT NULL,
            session_id TEXT NOT NULL,
            event_type TEXT NOT NULL,
            tool_name TEXT NOT NULL,
            action TEXT NOT NULL,
            outcome TEXT NOT NULL,
            details TEXT
        );",
    ) {
        panic!("legacy audit schema should initialize: {err}");
    }
    if let Err(err) = conn.execute(
        "INSERT INTO audit_entries
            (timestamp, session_id, event_type, tool_name, action, outcome, details)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            Utc::now().to_rfc3339(),
            "session-1",
            "tool_execution",
            "project_map",
            "invoke",
            "ok",
            Option::<String>::None,
        ],
    ) {
        panic!("legacy audit row should insert: {err}");
    }

    let log = must(
        AuditLog::open(&db),
        "audit log should reopen legacy database",
    );
    let entries = must(
        log.query_by_actor("local:default"),
        "legacy audit rows should load for local default owner",
    );
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].owner_actor, "local:default");
    assert_eq!(entries[0].event_type, AuditEventType::ToolExecution);
    assert!(
        must(
            log.query_by_actor("telegram:1"),
            "non-owner actor query should succeed"
        )
        .is_empty(),
        "legacy audit rows should not leak to non-owner actors"
    );
}

#[test]
fn task_store_defaults_legacy_rows_to_local_default_owner() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let db = temp.path().join("tasks.db");
    let conn = match rusqlite::Connection::open(&db) {
        Ok(value) => value,
        Err(err) => panic!("sqlite connection should open: {err}"),
    };
    if let Err(err) = conn.execute_batch(
        "CREATE TABLE shared_tasks (
            id TEXT PRIMARY KEY,
            parent_task_id TEXT,
            description TEXT NOT NULL,
            status TEXT NOT NULL DEFAULT 'Pending',
            assigned_instance TEXT,
            priority INTEGER NOT NULL DEFAULT 5,
            result TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deadline TEXT
        );
        CREATE TABLE task_assignments (
            task_id TEXT NOT NULL,
            target_instance TEXT NOT NULL,
            assigned_at TEXT NOT NULL,
            deadline TEXT,
            PRIMARY KEY (task_id, target_instance)
        );",
    ) {
        panic!("legacy task schema should initialize: {err}");
    }
    let timestamp = Utc::now().to_rfc3339();
    if let Err(err) = conn.execute(
        "INSERT INTO shared_tasks
            (id, parent_task_id, description, status, assigned_instance, priority, result, created_at, updated_at, deadline)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            "task-1",
            Option::<String>::None,
            "legacy task",
            "Pending",
            Option::<String>::None,
            5,
            Option::<String>::None,
            timestamp,
            Utc::now().to_rfc3339(),
            Option::<String>::None,
        ],
    ) {
        panic!("legacy task row should insert: {err}");
    }

    let store = must(
        TaskStore::open(&db),
        "task store should reopen legacy database",
    );
    let task = must(
        store.load_for_actor("task-1", "local:default"),
        "legacy task should load for local default owner",
    );
    assert_eq!(task.owner_actor, "local:default");
    assert_eq!(task.description, "legacy task");
    assert!(
        store.load_for_actor("task-1", "telegram:1").is_err(),
        "legacy task rows should not leak to non-owner actors"
    );
}
