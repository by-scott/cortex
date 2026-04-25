use crate::channels::store::ChannelStore;
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn paired_users_without_subscribe_field_default_to_false() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "telegram");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    must(
        fs::write(
            store.dir().join("paired_users.json"),
            r#"[{"user_id":"5188621876","name":"Scott","paired_at":"1714000000"}]"#,
        ),
        "legacy paired_users.json should write",
    );

    let paired = store.paired_users();
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].user_id, "5188621876");
    assert!(
        !paired[0].subscribe,
        "legacy paired user should default subscribe=false"
    );
}

#[test]
fn policy_without_optional_lists_uses_defaults() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "qq");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    must(
        fs::write(store.dir().join("policy.json"), r#"{"mode":"pairing"}"#),
        "legacy policy should write",
    );

    let policy = store.policy();
    assert_eq!(policy.mode, "pairing");
    assert!(policy.whitelist.is_empty());
    assert!(policy.blacklist.is_empty());
    assert_eq!(policy.pair_code_ttl_secs, 300);
    assert_eq!(policy.max_pending, 10);
}

#[test]
fn missing_update_offset_state_defaults_to_zero() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "whatsapp");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    must(
        fs::write(store.dir().join("update_offset.json"), "{}"),
        "legacy update offset should write",
    );

    assert_eq!(store.update_offset(), 0);
}
