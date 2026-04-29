use crate::channels::store::{ChannelPolicy, ChannelStore, PairedUser};
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn paired_users_roundtrip_current_subscription_field() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "telegram");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    store.save_paired_users(&[PairedUser {
        user_id: "5188621876".to_string(),
        name: "Scott".to_string(),
        paired_at: "1714000000".to_string(),
        subscribe: true,
    }]);

    let paired = store.paired_users();
    assert_eq!(paired.len(), 1);
    assert_eq!(paired[0].user_id, "5188621876");
    assert!(paired[0].subscribe);
}

#[test]
fn policy_roundtrips_current_lists_and_limits() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "qq");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    store.save_policy(&ChannelPolicy {
        mode: "whitelist".to_string(),
        whitelist: vec!["user:one".to_string()],
        blacklist: vec!["user:blocked".to_string()],
        pair_code_ttl_secs: 120,
        max_pending: 4,
    });

    let policy = store.policy();
    assert_eq!(policy.mode, "whitelist");
    assert_eq!(policy.whitelist, vec!["user:one"]);
    assert_eq!(policy.blacklist, vec!["user:blocked"]);
    assert_eq!(policy.pair_code_ttl_secs, 120);
    assert_eq!(policy.max_pending, 4);
}

#[test]
fn update_offset_roundtrips_current_state() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let store = ChannelStore::open(temp.path(), "whatsapp");
    must(
        fs::create_dir_all(store.dir()),
        "channel store dir should initialize",
    );

    store.save_update_offset(42);

    assert_eq!(store.update_offset(), 42);
}
