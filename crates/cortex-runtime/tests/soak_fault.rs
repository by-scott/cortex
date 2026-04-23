use cortex_kernel::replay::replay_determinism_digest;
use cortex_kernel::{Journal, MemoryStore};
use cortex_runtime::{PluginRegistry, ToolRegistry, plugin_loader};
use cortex_types::config::PluginsConfig;
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryType, Payload, SideEffectKind, TurnId,
};

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
