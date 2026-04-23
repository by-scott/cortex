use std::process::Command;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

#[test]
fn cli_scaffolds_only_process_plugins() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let status = must(
        Command::new(env!("CARGO_BIN_EXE_cortex"))
            .arg("--new-process-plugin")
            .arg("sample")
            .current_dir(temp.path())
            .status(),
        "run cortex should succeed",
    );
    assert!(status.success());

    let plugin_dir = temp.path().join("cortex-plugin-sample");
    assert!(plugin_dir.join("manifest.toml").is_file());
    assert!(plugin_dir.join("bin/sample-tool").is_file());
    assert!(!plugin_dir.join("Cargo.toml").exists());

    let rejected = must(
        Command::new(env!("CARGO_BIN_EXE_cortex"))
            .arg("--new-plugin")
            .arg("sample-native")
            .current_dir(temp.path())
            .status(),
        "run cortex should succeed",
    );
    assert!(!rejected.success());
}
