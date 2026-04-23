use cortex_runtime::{PluginRegistry, ToolRegistry, plugin_loader};
use cortex_types::config::PluginsConfig;

fn make_executable(path: &std::path::Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(path).expect("metadata").permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).expect("chmod");
    }
}

#[test]
fn process_plugin_registers_and_executes_manifest_tool() {
    let temp = tempfile::tempdir().expect("tempdir");
    let plugin_dir = temp.path().join("plugins").join("process-plugin");
    let bin_dir = plugin_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).expect("create bin");
    let tool_path = bin_dir.join("echo-tool");
    std::fs::write(
        &tool_path,
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"ok\",\"is_error\":false}'\n",
    )
    .expect("write tool");
    make_executable(&tool_path);
    std::fs::write(
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
    )
    .expect("write manifest");

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
    let result = tools
        .get("external_echo")
        .expect("registered tool")
        .execute(serde_json::json!({ "value": "hello" }))
        .expect("tool execution");
    assert_eq!(result.output, "ok");
    assert!(!result.is_error);
}
