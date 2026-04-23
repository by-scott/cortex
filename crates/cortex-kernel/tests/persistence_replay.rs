use cortex_kernel::{Journal, JournalSideEffectProvider, MemoryStore};
use cortex_types::{
    CorrelationId, Event, MemoryEntry, MemoryKind, MemoryType, Payload, SideEffectKind, TurnId,
};

#[test]
fn journal_replay_digest_is_stable_after_reopen() {
    let temp = tempfile::tempdir().expect("tempdir");
    let db = temp.path().join("journal.db");
    let turn = TurnId::new();
    let correlation = CorrelationId::new();

    {
        let journal = Journal::open(&db).expect("open journal");
        journal
            .append(&Event::new(turn, correlation, Payload::TurnStarted))
            .expect("append start");
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
            .expect("append side effect");
    }

    let journal = Journal::open(&db).expect("reopen journal");
    let events = journal.recent_events(10).expect("recent events");
    let mut first_provider = JournalSideEffectProvider::from_events(&events);
    let mut second_provider = JournalSideEffectProvider::from_events(&events);
    assert_eq!(
        cortex_kernel::replay::replay_determinism_digest(&events, &mut first_provider),
        cortex_kernel::replay::replay_determinism_digest(&events, &mut second_provider)
    );
}

#[test]
fn actor_scoped_memory_store_filters_non_admin_actors() {
    let temp = tempfile::tempdir().expect("tempdir");
    let store = MemoryStore::open(temp.path()).expect("open memory store");
    let mut own = MemoryEntry::new("alpha", "own", MemoryType::Project, MemoryKind::Semantic);
    own.owner_actor = "telegram:1".to_string();
    let mut other = MemoryEntry::new("beta", "other", MemoryType::Project, MemoryKind::Semantic);
    other.owner_actor = "telegram:2".to_string();
    store.save(&own).expect("save own");
    store.save(&other).expect("save other");

    assert_eq!(
        store
            .list_for_actor("telegram:1")
            .expect("list actor")
            .len(),
        1
    );
    assert_eq!(
        store
            .list_for_actor("local:default")
            .expect("list admin")
            .len(),
        2
    );
}
