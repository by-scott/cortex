use cortex_kernel::{ActorBindingsStore, CortexPaths, RuntimeStateStore, load_config};
use cortex_types::config::ProviderRegistry;
use std::fs;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn load_config_replaces_legacy_defaults_toml_with_config_defaults_reference() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(
        fs::create_dir_all(paths.data_dir()),
        "data dir should initialize",
    );
    must(
        fs::write(paths.config_path(), "[api]\nprovider = \"zai\"\n"),
        "config.toml should write",
    );
    must(
        fs::write(
            paths.data_dir().join("defaults.toml"),
            "# legacy defaults reference\n[api]\nprovider = \"legacy\"\n",
        ),
        "legacy defaults.toml should write",
    );

    let providers = ProviderRegistry::default();
    let _ = load_config(&home, None, &providers);

    assert!(
        !paths.data_dir().join("defaults.toml").exists(),
        "legacy data/defaults.toml should be removed during config load"
    );
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
fn actor_bindings_store_defaults_missing_transport_section() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(fs::create_dir_all(&home), "instance home should initialize");
    must(
        fs::write(
            paths.actors_path(),
            "[aliases]\n\"telegram:5188621876\" = \"user:scott\"\n",
        ),
        "legacy actors.toml should write",
    );

    let store = ActorBindingsStore::from_paths(&paths);
    let aliases = store.actor_aliases();
    let transports = store.transport_actors();

    assert_eq!(
        aliases.get("telegram:5188621876"),
        Some(&"user:scott".to_string())
    );
    assert!(
        transports.is_empty(),
        "missing transports section should default to an empty map"
    );
}

#[test]
fn actor_bindings_store_defaults_missing_alias_section() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(fs::create_dir_all(&home), "instance home should initialize");
    must(
        fs::write(paths.actors_path(), "[transports]\nhttp = \"user:scott\"\n"),
        "legacy actors.toml should write",
    );

    let store = ActorBindingsStore::from_paths(&paths);
    let aliases = store.actor_aliases();
    let transports = store.transport_actors();

    assert!(
        aliases.is_empty(),
        "missing aliases section should default to an empty map"
    );
    assert_eq!(transports.get("http"), Some(&"user:scott".to_string()));
}

#[test]
fn runtime_state_store_defaults_invalid_client_sessions_to_empty_map() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(
        fs::create_dir_all(paths.data_dir()),
        "data dir should initialize",
    );
    must(
        fs::write(paths.client_sessions_path(), "{not valid json"),
        "invalid client_sessions.json should write",
    );

    let store = RuntimeStateStore::from_paths(&paths);
    assert!(
        store.client_sessions().is_empty(),
        "invalid client_sessions.json should default to an empty map"
    );
}

#[test]
fn runtime_state_store_defaults_invalid_actor_sessions_to_empty_map() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let home = temp.path().join("default");
    let paths = CortexPaths::from_instance_home(&home);
    must(
        fs::create_dir_all(paths.data_dir()),
        "data dir should initialize",
    );
    must(
        fs::write(paths.actor_sessions_path(), "{not valid json"),
        "invalid actor_sessions.json should write",
    );

    let store = RuntimeStateStore::from_paths(&paths);
    assert!(
        store.actor_sessions().is_empty(),
        "invalid actor_sessions.json should default to an empty map"
    );
}
