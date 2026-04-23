use std::process::Command;

#[test]
fn cli_scaffolds_only_process_plugins() {
    let temp = tempfile::tempdir().expect("tempdir");
    let status = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .arg("--new-process-plugin")
        .arg("sample")
        .current_dir(temp.path())
        .status()
        .expect("run cortex");
    assert!(status.success());

    let plugin_dir = temp.path().join("cortex-plugin-sample");
    assert!(plugin_dir.join("manifest.toml").is_file());
    assert!(plugin_dir.join("bin/sample-tool").is_file());
    assert!(!plugin_dir.join("Cargo.toml").exists());

    let rejected = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .arg("--new-plugin")
        .arg("sample-native")
        .current_dir(temp.path())
        .status()
        .expect("run cortex");
    assert!(!rejected.success());
}
