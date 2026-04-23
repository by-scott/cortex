use cortex_kernel::{Journal, JournalSideEffectProvider, MemoryStore};
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryType, Payload, SideEffectKind, TurnId,
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
}
