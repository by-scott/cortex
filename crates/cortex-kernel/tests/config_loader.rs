use cortex_kernel::{ActorBindingsStore, CortexPaths, RuntimeStateStore, load_config};
use cortex_types::config::ProviderRegistry;
use std::collections::HashMap;
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn load_config_writes_current_config_defaults_reference() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(fs::create_dir_all(&home), "home dir should initialize");
    must(
        fs::write(paths.config_path(), "[api]\nprovider = \"zai\"\n"),
        "config.toml should write",
    );

    let providers = ProviderRegistry::default();
    let _ = load_config(&home, None, &providers);

    assert!(
        paths.config_defaults_path().exists(),
        "config load should regenerate config.defaults.toml"
    );
    let defaults = must(
        fs::read_to_string(paths.config_defaults_path()),
        "config.defaults.toml should load",
    );
    assert!(
        defaults.contains("Factory default configuration reference"),
        "config.defaults.toml should contain the factory reference header"
    );
}

#[test]
fn actor_bindings_store_roundtrips_current_sections() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(fs::create_dir_all(&home), "instance home should initialize");

    let store = ActorBindingsStore::from_paths(&paths);
    store.set_actor_alias("telegram:5188621876", "user:scott");
    store.set_transport_actor("telegram", "user:scott");

    let aliases = store.actor_aliases();
    let transports = store.transport_actors();

    assert_eq!(
        aliases.get("telegram:5188621876"),
        Some(&"user:scott".to_string())
    );
    assert_eq!(transports.get("telegram"), Some(&"user:scott".to_string()));
}

#[test]
fn runtime_state_store_roundtrips_current_session_maps() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(
        fs::create_dir_all(paths.data_dir()),
        "data dir should initialize",
    );

    let store = RuntimeStateStore::from_paths(&paths);
    let mut client_sessions = HashMap::new();
    client_sessions.insert("telegram:5188621876".to_string(), "session-1".to_string());
    store.save_client_sessions(&client_sessions);

    let mut actor_sessions = HashMap::new();
    actor_sessions.insert("user:scott".to_string(), "session-1".to_string());
    store.save_actor_sessions(&actor_sessions);

    assert_eq!(store.client_sessions(), client_sessions);
    assert_eq!(store.actor_sessions(), actor_sessions);
}
