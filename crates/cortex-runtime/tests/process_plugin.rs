use cortex_runtime::{PluginRegistry, ToolRegistry, plugin_loader};
use cortex_types::config::PluginsConfig;

fn must<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("{context}: {err}"),
    }
}

fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = match std::fs::metadata(path) {
            Ok(metadata) => metadata.permissions(),
            Err(err) => panic!("metadata should load: {err}"),
        };
        permissions.set_mode(0o755);
        if let Err(err) = std::fs::set_permissions(path, permissions) {
            panic!("chmod should succeed: {err}");
        }
    }
}

#[test]
fn process_plugin_registers_and_executes_manifest_tool() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let plugin_dir = temp.path().join("plugins").join("process-plugin");
    let bin_dir = plugin_dir.join("bin");
    if let Err(err) = std::fs::create_dir_all(&bin_dir) {
        panic!("create bin should succeed: {err}");
    }
    let tool_path = bin_dir.join("echo-tool");
    if let Err(err) = std::fs::write(
        &tool_path,
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"ok\",\"is_error\":false}'\n",
    ) {
        panic!("write tool should succeed: {err}");
    }
    make_executable(&tool_path);
    if let Err(err) = std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
name = "process-plugin"
version = "0.1.0"
description = "process plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "external_echo"
description = "echo through process isolation"
command = "bin/echo-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    ) {
        panic!("write manifest should succeed: {err}");
    }

    let config = PluginsConfig {
        dir: "plugins".to_string(),
        enabled: vec!["process-plugin".to_string()],
    };
    let mut plugins = PluginRegistry::new();
    let mut tools = ToolRegistry::new();
    let (loaded, warnings) =
        plugin_loader::load_plugins(temp.path(), &config, &mut plugins, &mut tools);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);
    let Some(tool) = tools.get("external_echo") else {
        panic!("registered tool should exist");
    };
    let result = must(
        tool.execute(serde_json::json!({ "value": "hello" })),
        "tool execution should succeed",
    );
    assert_eq!(result.output, "ok");
    assert!(!result.is_error);
}
