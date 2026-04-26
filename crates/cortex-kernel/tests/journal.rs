use cortex_kernel::FileJournal;
use cortex_types::{
    ActorId, AuthContext, ClientId, Event, EventPayload, OwnedScope, TenantId, Visibility,
};

fn context(label: &'static str) -> AuthContext {
    AuthContext::new(
        TenantId::from_static(label),
        ActorId::from_static(label),
        ClientId::from_static(label),
    )
}

#[test]
fn journal_replay_filters_by_visibility() {
    let dir = tempfile::tempdir().unwrap();
    let journal = FileJournal::open(dir.path().join("events.jsonl")).unwrap();
    let owner = context("one");
    let other = context("two");
    let private = OwnedScope::private_for(&owner);
    let public = OwnedScope::new(
        owner.tenant_id.clone(),
        owner.actor_id.clone(),
        None,
        Visibility::Public,
    );

    journal
        .append(&Event::new(
            private,
            EventPayload::AccessDenied {
                reason: "private".to_string(),
            },
        ))
        .unwrap();
    journal
        .append(&Event::new(
            public,
            EventPayload::AccessDenied {
                reason: "public".to_string(),
            },
        ))
        .unwrap();

    assert_eq!(journal.replay_visible(&owner).unwrap().len(), 2);
    assert_eq!(journal.replay_visible(&other).unwrap().len(), 1);
}
