use cortex_kernel::replay::replay_determinism_digest;
use cortex_kernel::{AuditEntry, AuditLog, Journal, MemoryStore, SessionStore, TaskStore};
use cortex_runtime::{PluginRegistry, ToolRegistry, plugin_loader};
use cortex_types::config::PluginsConfig;
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryType, Payload, SideEffectKind, TurnId,
};
use cortex_types::{SessionId, SessionMetadata, SharedTask, SharedTaskStatus};

fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }
}

#[test]
fn replay_digest_survives_journal_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let db = tmp.path().join("cortex.db");
    let turn = TurnId::new();
    let corr = CorrelationId::new();
    {
        let journal = Journal::open(&db).unwrap();
        journal
            .append(&Event::new(turn, corr, Payload::TurnStarted))
            .unwrap();
        journal
            .append(&Event::new(
                turn,
                corr,
                Payload::SideEffectRecorded {
                    kind: SideEffectKind::WallClock,
                    key: "turn_start".into(),
                    value: "recorded".into(),
                },
            ))
            .unwrap();
    }

    let journal = Journal::open(&db).unwrap();
    let events = journal.recent_events(10).unwrap();
    let mut provider = cortex_kernel::JournalSideEffectProvider::from_events(&events);
    let digest_a = replay_determinism_digest(&events, &mut provider);
    let mut provider = cortex_kernel::JournalSideEffectProvider::from_events(&events);
    let digest_b = replay_determinism_digest(&events, &mut provider);

    assert_eq!(digest_a, digest_b);
}

#[test]
fn process_plugin_failure_is_contained_as_tool_error() {
    let tmp = tempfile::tempdir().unwrap();
    let pd = tmp.path().join("plugins").join("process-plugin");
    let bin = pd.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let tool_path = bin.join("bad-tool");
    std::fs::write(&tool_path, "#!/bin/sh\ncat >/dev/null\nprintf 'not-json'\n").unwrap();
    make_executable(&tool_path);
    std::fs::write(
        pd.join("manifest.toml"),
        r#"
name = "process-plugin"
version = "0.1.0"
description = "fault harness plugin"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "bad_tool"
description = "returns invalid JSON"
command = "bin/bad-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    )
    .unwrap();

    let config = PluginsConfig {
        dir: "plugins".into(),
        enabled: vec!["process-plugin".into()],
    };
    let mut plugin_registry = PluginRegistry::new();
    let mut tool_registry = ToolRegistry::new();
    let (_loaded, warnings) = plugin_loader::load_plugins(
        tmp.path(),
        &config,
        &mut plugin_registry,
        &mut tool_registry,
    );
    assert!(warnings.is_empty(), "{warnings:?}");

    let err = tool_registry
        .get("bad_tool")
        .unwrap()
        .execute(serde_json::json!({}))
        .unwrap_err();
    assert!(err.to_string().contains("invalid JSON"));
}

#[test]
fn process_plugin_host_path_escape_is_rejected_before_registration() {
    let tmp = tempfile::tempdir().unwrap();
    let pd = tmp.path().join("plugins").join("process-plugin");
    std::fs::create_dir_all(&pd).unwrap();
    std::fs::write(
        pd.join("manifest.toml"),
        r#"
name = "process-plugin"
version = "0.1.0"
description = "fault harness plugin"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "host_shell"
description = "host shell"
command = "/bin/sh"
input_schema = { type = "object" }
"#,
    )
    .unwrap();

    let config = PluginsConfig {
        dir: "plugins".into(),
        enabled: vec!["process-plugin".into()],
    };
    let mut plugin_registry = PluginRegistry::new();
    let mut tool_registry = ToolRegistry::new();
    let (_loaded, warnings) = plugin_loader::load_plugins(
        tmp.path(),
        &config,
        &mut plugin_registry,
        &mut tool_registry,
    );

    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("escapes plugin directory"));
    assert!(tool_registry.get("host_shell").is_none());
}

#[test]
fn actor_scoped_memory_survives_store_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let store = MemoryStore::open(tmp.path()).unwrap();
    let mut own = MemoryEntry::new("alpha", "own", MemoryType::Project, MemoryKind::Semantic);
    own.owner_actor = "telegram:1".into();
    let mut other = MemoryEntry::new("beta", "other", MemoryType::Project, MemoryKind::Semantic);
    other.owner_actor = "telegram:2".into();
    store.save(&own).unwrap();
    store.save(&other).unwrap();

    let reopened = MemoryStore::open(tmp.path()).unwrap();
    let visible = reopened.list_for_actor("telegram:1").unwrap();
    assert_eq!(visible.len(), 1);
    assert_eq!(visible[0].owner_actor, "telegram:1");
    assert_eq!(reopened.list_for_actor("local:default").unwrap().len(), 2);
}

#[test]
fn actor_scoped_session_task_and_audit_survive_store_reopen() {
    let tmp = tempfile::tempdir().unwrap();

    let sessions_dir = tmp.path().join("sessions");
    let session_store = SessionStore::open(&sessions_dir).unwrap();
    let mut own_session = SessionMetadata::new(SessionId::new(), 0);
    own_session.owner_actor = "telegram:1".into();
    let mut other_session = SessionMetadata::new(SessionId::new(), 1);
    other_session.owner_actor = "telegram:2".into();
    session_store.save(&own_session).unwrap();
    session_store.save(&other_session).unwrap();

    let task_path = tmp.path().join("tasks.db");
    let task_store = TaskStore::open(&task_path).unwrap();
    let mut own_task = SharedTask::new("own task");
    own_task.owner_actor = "telegram:1".into();
    let mut other_task = SharedTask::new("other task");
    other_task.owner_actor = "telegram:2".into();
    task_store.save(&own_task).unwrap();
    task_store.save(&other_task).unwrap();

    let audit_path = tmp.path().join("audit.db");
    let audit = AuditLog::open(&audit_path).unwrap();
    audit
        .append(
            &AuditEntry::tool_execution(own_session.id.to_string(), "read", "file", "ok")
                .with_owner_actor("telegram:1"),
        )
        .unwrap();
    audit
        .append(
            &AuditEntry::tool_execution(other_session.id.to_string(), "read", "file", "ok")
                .with_owner_actor("telegram:2"),
        )
        .unwrap();

    let session_store = SessionStore::open(&sessions_dir).unwrap();
    assert_eq!(session_store.list_for_actor("telegram:1").len(), 1);
    assert_eq!(session_store.list_for_actor("local:default").len(), 2);

    let task_store = TaskStore::open(&task_path).unwrap();
    assert_eq!(
        task_store
            .list_by_status_for_actor(SharedTaskStatus::Pending, "telegram:1")
            .unwrap()
            .len(),
        1
    );
    assert!(
        task_store
            .load_for_actor(&other_task.id, "telegram:1")
            .is_err()
    );

    let audit = AuditLog::open(&audit_path).unwrap();
    assert_eq!(audit.query_by_actor("telegram:1").unwrap().len(), 1);
    assert_eq!(audit.query_by_actor("local:default").unwrap().len(), 2);
}
