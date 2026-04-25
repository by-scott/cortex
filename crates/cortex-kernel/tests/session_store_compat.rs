use cortex_kernel::SessionStore;
use cortex_types::{Message, SessionId, SessionMetadata};
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn session_store_defaults_invalid_metadata_to_none() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = must(SessionStore::open(temp.path()), "session store should open");
    let session_id = SessionId::new();

    must(
        fs::write(
            temp.path().join(format!("{session_id}.json")),
            "{not valid json",
        ),
        "invalid session metadata should write",
    );

    assert!(
        store.load(&session_id).is_none(),
        "invalid legacy session metadata should default to None"
    );
}

#[test]
fn session_store_defaults_invalid_history_to_empty_messages() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = must(SessionStore::open(temp.path()), "session store should open");
    let session_id = SessionId::new();
    let metadata = SessionMetadata::new(session_id, 0);

    must(store.save(&metadata), "session metadata should save");
    must(
        fs::write(
            temp.path().join(format!("{session_id}.history")),
            b"not valid msgpack history",
        ),
        "invalid session history should write",
    );

    let history = store.load_history(&session_id);
    assert_eq!(
        history,
        Vec::<Message>::new(),
        "invalid legacy session history should default to an empty message list"
    );
}
