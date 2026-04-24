use crate::deploy::{
    SYSTEM_CORTEX_HOME, cmd_permission, cmd_plugin, parse_install_permission_level,
    read_enabled_plugins, refresh_user_launcher_for_home, resolve_cortex_home,
    resolve_paths_from_args, service_name, update_install_permission_level,
};
use std::fs;
use std::path::{Path, PathBuf};

fn write_text(path: &Path, text: &str) {
    if let Err(err) = fs::write(path, text) {
        panic!("failed to write {}: {err}", path.display());
    }
}

fn make_temp_instance() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("failed to create tempdir: {err}"),
    };
    let base = temp.path().join("cortex-home");
    let instance_home = base.join("default");
    if let Err(err) = fs::create_dir_all(&instance_home) {
        panic!(
            "failed to create instance directory {}: {err}",
            instance_home.display()
        );
    }
    write_text(&instance_home.join("config.toml"), "");
    (temp, base, instance_home)
}

fn make_plugin_dir(root: &Path, name: &str) -> PathBuf {
    let plugin_dir = root.join(format!("cortex-plugin-{name}"));
    if let Err(err) = fs::create_dir_all(&plugin_dir) {
        panic!(
            "failed to create plugin directory {}: {err}",
            plugin_dir.display()
        );
    }
    write_text(
        &plugin_dir.join("manifest.toml"),
        &format!(
            "name = \"{name}\"\nversion = \"1.2.0\"\ndescription = \"test plugin\"\ncortex_version = \"1.2.0\"\n\n[capabilities]\nprovides = [\"tools\"]\n"
        ),
    );
    plugin_dir
}

#[test]
fn resolve_paths_prefers_home_flag() {
    let args = vec![
        "plugin".to_string(),
        "list".to_string(),
        "--home".to_string(),
        "/tmp/custom-cortex".to_string(),
        "--id".to_string(),
        "demo".to_string(),
    ];

    let paths = resolve_paths_from_args(&args);

    assert_eq!(paths.base_dir(), Path::new("/tmp/custom-cortex"));
    assert_eq!(paths.instance_home(), Path::new("/tmp/custom-cortex/demo"));
}

#[test]
fn service_name_separates_home_and_instance() {
    let default_base = PathBuf::from(resolve_cortex_home());
    let custom_base = PathBuf::from("/tmp/cortex-other-home");

    assert_eq!(service_name(&default_base, None, false), "cortex");
    assert_eq!(service_name(&default_base, Some("qa"), false), "cortex@qa");

    let custom_default = service_name(&custom_base, None, false);
    let custom_named = service_name(&custom_base, Some("qa"), false);
    assert_ne!(custom_default, "cortex");
    assert_ne!(custom_named, "cortex@qa");
    assert!(custom_default.starts_with("cortex-"));
    assert!(custom_named.starts_with("cortex-"));
    assert!(custom_named.ends_with("@qa"));
}

#[test]
fn service_name_separates_system_home_and_instance() {
    let default_base = PathBuf::from(SYSTEM_CORTEX_HOME);
    let custom_base = PathBuf::from("/srv/cortex-alt");

    assert_eq!(service_name(&default_base, None, true), "cortex");
    assert_eq!(service_name(&default_base, Some("ops"), true), "cortex@ops");

    let custom_default = service_name(&custom_base, None, true);
    let custom_named = service_name(&custom_base, Some("ops"), true);
    assert_ne!(custom_default, "cortex");
    assert_ne!(custom_named, "cortex@ops");
    assert!(custom_default.starts_with("cortex-"));
    assert!(custom_named.starts_with("cortex-"));
    assert!(custom_named.ends_with("@ops"));
}

#[test]
fn plugin_commands_respect_home_and_instance_enablement() {
    let (_temp, base, instance_home) = make_temp_instance();
    let plugin_dir = make_plugin_dir(base.parent().unwrap_or(&base), "sample");
    let base_text = base.to_string_lossy().to_string();
    let plugin_text = plugin_dir.to_string_lossy().to_string();

    if let Err(err) = cmd_plugin(&[
        "plugin".to_string(),
        "install".to_string(),
        plugin_text,
        "--home".to_string(),
        base_text.clone(),
    ]) {
        panic!("plugin install should succeed: {err}");
    }

    let enabled = read_enabled_plugins(&instance_home);
    assert_eq!(enabled, vec!["sample".to_string()]);
    assert!(base.join("plugins/sample/manifest.toml").is_file());

    if let Err(err) = cmd_plugin(&[
        "plugin".to_string(),
        "disable".to_string(),
        "sample".to_string(),
        "--home".to_string(),
        base_text.clone(),
    ]) {
        panic!("plugin disable should succeed: {err}");
    }
    assert!(read_enabled_plugins(&instance_home).is_empty());

    if let Err(err) = cmd_plugin(&[
        "plugin".to_string(),
        "enable".to_string(),
        "sample".to_string(),
        "--home".to_string(),
        base_text,
    ]) {
        panic!("plugin enable should succeed: {err}");
    }
    assert_eq!(
        read_enabled_plugins(&instance_home),
        vec!["sample".to_string()]
    );
}

#[test]
fn plugin_commands_respect_global_home_flag_before_subcommand() {
    let (_temp, base, instance_home) = make_temp_instance();
    let plugin_dir = make_plugin_dir(base.parent().unwrap_or(&base), "sample");
    let base_text = base.to_string_lossy().to_string();
    let plugin_text = plugin_dir.to_string_lossy().to_string();

    if let Err(err) = cmd_plugin(&[
        "--home".to_string(),
        base_text,
        "plugin".to_string(),
        "install".to_string(),
        plugin_text,
    ]) {
        panic!("plugin install with global home should succeed: {err}");
    }

    assert!(base.join("plugins/sample/manifest.toml").is_file());
    assert_eq!(
        read_enabled_plugins(&instance_home),
        vec!["sample".to_string()]
    );
}

#[test]
fn launcher_refresh_skips_self_referential_binary_path() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("failed to create tempdir: {err}"),
    };
    let launcher_dir = temp.path().join(".local/bin");
    if let Err(err) = fs::create_dir_all(&launcher_dir) {
        panic!(
            "failed to create launcher directory {}: {err}",
            launcher_dir.display()
        );
    }
    let launcher_path = launcher_dir.join("cortex");
    write_text(&launcher_path, "#!/bin/sh\nexit 0\n");

    if let Err(err) =
        refresh_user_launcher_for_home(temp.path(), launcher_path.to_string_lossy().as_ref())
    {
        panic!("launcher refresh should succeed: {err}");
    }

    let metadata = match fs::symlink_metadata(&launcher_path) {
        Ok(value) => value,
        Err(err) => panic!(
            "failed to stat launcher path {}: {err}",
            launcher_path.display()
        ),
    };
    assert!(!metadata.file_type().is_symlink());
}

#[test]
fn install_permission_level_parses_cli_values() {
    let balanced_level = match parse_install_permission_level(&[
        "install".to_string(),
        "--permission-level".to_string(),
        "balanced".to_string(),
    ]) {
        Ok(Some(level)) => level,
        Ok(None) => panic!("permission level should parse from cli"),
        Err(err) => panic!("cli permission level parse failed: {err}"),
    };
    assert_eq!(format!("{balanced_level:?}"), "Review");

    let open_level = match parse_install_permission_level(&[
        "install".to_string(),
        "--permission-level".to_string(),
        "open".to_string(),
    ]) {
        Ok(Some(level)) => level,
        Ok(None) => panic!("open permission level should parse from cli"),
        Err(err) => panic!("open permission level parse failed: {err}"),
    };
    assert_eq!(format!("{open_level:?}"), "RequireConfirmation");
}

#[test]
fn install_permission_level_updates_risk_section() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("failed to create tempdir: {err}"),
    };
    let config_path = temp.path().join("config.toml");
    write_text(
        &config_path,
        "[api]\nprovider = \"zai\"\n\n[risk]\nauto_approve_up_to = \"Allow\"\n",
    );

    if let Err(err) =
        update_install_permission_level(&config_path, cortex_types::RiskLevel::RequireConfirmation)
    {
        panic!("permission level update should succeed: {err}");
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(value) => value,
        Err(err) => panic!("failed to read config {}: {err}", config_path.display()),
    };
    assert!(content.contains("[risk]"));
    assert!(content.contains("auto_approve_up_to = \"RequireConfirmation\""));
}

#[test]
fn install_permission_level_defaults_to_balanced_when_not_provided() {
    let level = match parse_install_permission_level(&["install".to_string()]) {
        Ok(level) => level,
        Err(err) => panic!("default permission level parse failed: {err}"),
    };
    assert!(level.is_none());
}

#[test]
fn testing_and_ops_docs_keep_docker_gate_commands() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let testing = match fs::read_to_string(repo_root.join("docs/testing.md")) {
        Ok(value) => value,
        Err(err) => panic!("failed to read docs/testing.md: {err}"),
    };
    let ops = match fs::read_to_string(repo_root.join("docs/ops.md")) {
        Ok(value) => value,
        Err(err) => panic!("failed to read docs/ops.md: {err}"),
    };
    let ops_zh = match fs::read_to_string(repo_root.join("docs/zh/ops.md")) {
        Ok(value) => value,
        Err(err) => panic!("failed to read docs/zh/ops.md: {err}"),
    };

    for doc in [&testing, &ops, &ops_zh] {
        assert!(
            doc.contains("docker compose run --rm dev cargo fmt --check"),
            "docs should keep Docker-based fmt gate"
        );
        assert!(
            doc.contains("docker compose run --rm dev cargo test --workspace"),
            "docs should keep Docker-based test gate"
        );
        assert!(
            doc.contains("docker compose run --rm dev cargo clippy --workspace --all-targets --"),
            "docs should keep Docker-based clippy gate"
        );
    }

    assert!(
        testing.contains("`crates/cortex-turn/tests/memory_tools.rs`"),
        "testing docs should mention the actor-scoped memory tool surface"
    );
    assert!(
        testing.contains("`memory_search` visibility with and without a runtime actor"),
        "testing docs should describe the memory tool ownership coverage"
    );
    assert!(
        testing.contains("embedding visibility inherited through memory ids"),
        "testing docs should mention embedding ownership inheritance"
    );
    assert!(
        testing.contains("`crates/cortex-runtime/src/tests/http_memory.rs`"),
        "testing docs should mention the HTTP memory ownership surface"
    );
    assert!(
        testing.contains("transport-actor ownership on `POST /api/memory`"),
        "testing docs should describe the HTTP memory write surface"
    );
    assert!(
        testing.contains("`crates/cortex-runtime/src/tests/http_sessions.rs`"),
        "testing docs should mention the HTTP session ownership surface"
    );
    assert!(
        testing.contains("hidden-session rejection on `GET /api/session/{id}`"),
        "testing docs should describe the hidden-session rejection surface"
    );
}

#[test]
fn permission_command_updates_instance_config() {
    let (_temp, base, instance_home) = make_temp_instance();
    let config_path = instance_home.join("config.toml");
    write_text(
        &config_path,
        "[risk]\nauto_approve_up_to = \"Review\"\nconfirmation_timeout_secs = 300\n",
    );

    if let Err(err) = cmd_permission(&[
        "open".to_string(),
        "--home".to_string(),
        base.to_string_lossy().to_string(),
    ]) {
        panic!("permission command should succeed: {err}");
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(value) => value,
        Err(err) => panic!("failed to read config {}: {err}", config_path.display()),
    };
    assert!(content.contains("auto_approve_up_to = \"RequireConfirmation\""));
}

#[test]
fn permission_command_accepts_real_cli_argv_shape() {
    let (_temp, base, instance_home) = make_temp_instance();
    let config_path = instance_home.join("config.toml");
    write_text(
        &config_path,
        "[risk]\nauto_approve_up_to = \"Review\"\nconfirmation_timeout_secs = 300\n",
    );

    if let Err(err) = cmd_permission(&[
        "permission".to_string(),
        "strict".to_string(),
        "--home".to_string(),
        base.to_string_lossy().to_string(),
    ]) {
        panic!("permission command with real argv shape should succeed: {err}");
    }

    let content = match fs::read_to_string(&config_path) {
        Ok(value) => value,
        Err(err) => panic!("failed to read config {}: {err}", config_path.display()),
    };
    assert!(content.contains("auto_approve_up_to = \"Allow\""));
}

#[test]
fn permission_mode_docs_match_cli_surface() {
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
    let readme = read_doc(&repo_root.join("README.md"));
    let readme_zh = read_doc(&repo_root.join("README.zh.md"));
    let usage = read_doc(&repo_root.join("docs").join("usage.md"));
    let usage_zh = read_doc(&repo_root.join("docs").join("zh").join("usage.md"));
    let quickstart = read_doc(&repo_root.join("docs").join("quickstart.md"));
    let quickstart_zh = read_doc(&repo_root.join("docs").join("zh").join("quickstart.md"));
    let config = read_doc(&repo_root.join("docs").join("config.md"));
    let config_zh = read_doc(&repo_root.join("docs").join("zh").join("config.md"));

    let install_snippet = "strict|balanced|open";
    for mode in ["strict", "balanced", "open"] {
        assert!(
            usage.contains(install_snippet),
            "usage should advertise CLI permission modes"
        );
        assert!(
            quickstart.contains(&format!("cortex permission {mode}")),
            "quickstart should show the {mode} permission command"
        );
        assert!(
            readme.contains(&format!("`{mode}`")),
            "README should mention the {mode} permission mode"
        );
        assert!(
            readme_zh.contains(&format!("`{mode}`")),
            "README.zh should mention the {mode} permission mode"
        );
        assert!(
            usage_zh.contains(mode),
            "Chinese usage should mention the {mode} permission mode"
        );
        assert!(
            quickstart_zh.contains(&format!("cortex permission {mode}")),
            "Chinese quickstart should show the {mode} permission command"
        );
    }

    assert!(
        readme.contains("The default permission mode is `balanced`."),
        "README should describe the default permission mode"
    );
    assert!(
        readme_zh.contains("默认权限模式是 `balanced`。"),
        "README.zh should describe the default permission mode"
    );
    assert!(
        config.contains("`strict` / `balanced` / `open`"),
        "config docs should list install-time permission modes"
    );
    assert!(
        config.contains("`Review` is the default standard mode"),
        "config docs should describe the balanced/Review mapping"
    );
    assert!(
        config.contains("`Allow` is the stricter mode"),
        "config docs should describe the strict/Allow mapping"
    );
    assert!(
        config.contains("`RequireConfirmation` is the most permissive setting"),
        "config docs should describe the open/RequireConfirmation mapping"
    );
    assert!(
        config_zh.contains("默认标准模式是 `Review`"),
        "Chinese config docs should describe the balanced/Review mapping"
    );
    assert!(
        config_zh.contains("更严格的模式是 `Allow`"),
        "Chinese config docs should describe the strict/Allow mapping"
    );
    assert!(
        config_zh.contains("设为 `RequireConfirmation` 则是常规执行中最宽松的设置"),
        "Chinese config docs should describe the open/RequireConfirmation mapping"
    );
    assert!(
        parse_install_permission_level(&[
            "install".to_string(),
            "--permission-level".to_string(),
            "allow".to_string(),
        ])
        .is_ok_and(|level| level == Some(cortex_types::RiskLevel::Allow)),
        "CLI alias 'allow' should still map to strict mode"
    );
    assert!(
        parse_install_permission_level(&[
            "install".to_string(),
            "--permission-level".to_string(),
            "review".to_string(),
        ])
        .is_ok_and(|level| level == Some(cortex_types::RiskLevel::Review)),
        "CLI alias 'review' should still map to balanced mode"
    );
    assert!(
        parse_install_permission_level(&[
            "install".to_string(),
            "--permission-level".to_string(),
            "require-confirmation".to_string(),
        ])
        .is_ok_and(|level| level == Some(cortex_types::RiskLevel::RequireConfirmation)),
        "CLI alias 'require-confirmation' should still map to open mode"
    );
}

fn read_doc(path: &Path) -> String {
    match fs::read_to_string(path) {
        Ok(value) => value,
        Err(err) => panic!("failed to read {}: {err}", path.display()),
    }
}
