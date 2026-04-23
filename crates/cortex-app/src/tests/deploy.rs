use crate::deploy::{
    SYSTEM_CORTEX_HOME, cmd_plugin, read_enabled_plugins, resolve_cortex_home,
    resolve_paths_from_args, service_name,
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
