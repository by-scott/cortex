use cortex_kernel::{
    ActorBindingsStore, CortexPaths, RuntimeStateStore, load_config, parse_bool_like,
    update_config_toml_value,
};
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
fn config_toml_value_update_preserves_file_and_validates_result() {
    let temp = must(tempfile::tempdir(), "tempdir should open");
    let config_path = temp.path().join("config.toml");
    must(
        fs::write(
            &config_path,
            "# keep comment\n[api]\nprovider = \"ollama\"\n\n[turn]\nmax_tool_iterations = 32\n",
        ),
        "config.toml should write",
    );

    must(
        update_config_toml_value(&config_path, "turn", "strip_think_tags", "false"),
        "config update should succeed",
    );

    let content = must(fs::read_to_string(&config_path), "config should read");
    assert!(content.contains("# keep comment"));
    assert!(content.contains("strip_think_tags = false"));
    let parsed: toml::Value = must(toml::from_str(&content), "config should remain valid TOML");
    assert_eq!(parsed["turn"]["strip_think_tags"].as_bool(), Some(false));
}

#[test]
fn bool_like_parser_accepts_thinking_visibility_terms() {
    assert_eq!(parse_bool_like("show"), Some(true));
    assert_eq!(parse_bool_like("hide"), Some(false));
    assert_eq!(parse_bool_like("on"), Some(true));
    assert_eq!(parse_bool_like("off"), Some(false));
    assert_eq!(parse_bool_like("maybe"), None);
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
