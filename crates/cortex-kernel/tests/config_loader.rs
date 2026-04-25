use cortex_kernel::{CortexPaths, load_config};
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
