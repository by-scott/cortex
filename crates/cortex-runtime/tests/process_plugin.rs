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

fn write_process_plugin(
    root: &std::path::Path,
    plugin_dir_name: &str,
    script_body: &str,
    native_tools_body: &str,
) -> std::path::PathBuf {
    let plugin_dir = root.join("plugins").join(plugin_dir_name);
    let bin_dir = plugin_dir.join("bin");
    if let Err(err) = std::fs::create_dir_all(&bin_dir) {
        panic!("create bin should succeed: {err}");
    }
    let tool_path = bin_dir.join("echo-tool");
    if let Err(err) = std::fs::write(&tool_path, script_body) {
        panic!("write tool should succeed: {err}");
    }
    make_executable(&tool_path);
    let manifest = format!(
        r#"
name = "{plugin_dir_name}"
version = "0.1.0"
description = "process plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

{native_tools_body}
"#
    );
    if let Err(err) = std::fs::write(plugin_dir.join("manifest.toml"), manifest) {
        panic!("write manifest should succeed: {err}");
    }
    plugin_dir
}

fn load_process_plugins(
    root: &std::path::Path,
    enabled: &[&str],
) -> (
    cortex_runtime::plugin_loader::LoadedPlugins,
    Vec<String>,
    PluginRegistry,
    ToolRegistry,
) {
    let config = PluginsConfig {
        dir: "plugins".to_string(),
        enabled: enabled.iter().map(|name| (*name).to_string()).collect(),
    };
    let mut plugins = PluginRegistry::new();
    let mut tools = ToolRegistry::new();
    let (loaded, warnings) = plugin_loader::load_plugins(root, &config, &mut plugins, &mut tools);
    (loaded, warnings, plugins, tools)
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

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["process-plugin"]);

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

#[test]
fn loader_ignores_backup_plugin_directories() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let plugins_root = temp.path().join("plugins");
    let live_plugin_dir = plugins_root.join("process-plugin");
    let backup_plugin_dir = plugins_root.join("process-plugin.bak");

    for plugin_dir in [&live_plugin_dir, &backup_plugin_dir] {
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
    }

    let (loaded, warnings, _plugins, _tools) =
        load_process_plugins(temp.path(), &["process-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);
    assert_eq!(loaded.manifests[0].name, "process-plugin");
}

#[test]
fn process_plugin_rejects_command_paths_that_escape_plugin_directory() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "escaped-plugin",
        "#!/bin/sh\nprintf '{\"output\":\"ok\",\"is_error\":false}'\n",
        r#"
[[native.tools]]
name = "escaped_echo"
description = "should not load"
command = "../outside-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    );
    let outside_tool = temp.path().join("plugins").join("outside-tool");
    if let Err(err) = std::fs::write(
        &outside_tool,
        "#!/bin/sh\nprintf '{\"output\":\"outside\",\"is_error\":false}'\n",
    ) {
        panic!("write outside tool should succeed: {err}");
    }
    make_executable(&outside_tool);

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["escaped-plugin"]);

    assert!(loaded.manifests.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("escapes plugin directory"),
        "{warnings:?}"
    );
    assert!(tools.get("escaped_echo").is_none());
}

#[test]
fn process_plugin_allows_host_paths_only_when_explicitly_opted_in() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let host_tool = temp.path().join("host-tool");
    if let Err(err) = std::fs::write(
        &host_tool,
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"host\",\"is_error\":false}'\n",
    ) {
        panic!("write host tool should succeed: {err}");
    }
    make_executable(&host_tool);

    let tool_path = host_tool.display().to_string();
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "host-path-plugin",
        "#!/bin/sh\nprintf '{\"output\":\"unused\",\"is_error\":false}'\n",
        &format!(
            r#"
[[native.tools]]
name = "host_echo"
description = "host path tool"
command = "{tool_path}"
allow_host_paths = true
timeout_secs = 1
input_schema = {{ type = "object" }}
"#
        ),
    );

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["host-path-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);
    let Some(tool) = tools.get("host_echo") else {
        panic!("registered host-path tool should exist");
    };
    let result = must(
        tool.execute(serde_json::json!({ "value": "hello" })),
        "host-path tool execution should succeed",
    );
    assert_eq!(result.output, "host");
    assert!(!result.is_error);
}

#[test]
fn process_plugin_timeout_and_output_limits_surface_as_tool_errors() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "limited-plugin",
        "#!/bin/sh\nif [ \"$1\" = \"sleep\" ]; then sleep 2; printf '{\"output\":\"late\",\"is_error\":false}'\nelse head -c 80 /dev/zero | tr '\\0' 'a'; fi\n",
        r#"
[[native.tools]]
name = "slow_echo"
description = "times out"
command = "bin/echo-tool"
args = ["sleep"]
timeout_secs = 1
input_schema = { type = "object" }

[[native.tools]]
name = "large_echo"
description = "too much output"
command = "bin/echo-tool"
max_output_bytes = 32
input_schema = { type = "object" }
"#,
    );

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["limited-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);

    let Some(slow_tool) = tools.get("slow_echo") else {
        panic!("slow tool should exist");
    };
    let slow_result = must(
        slow_tool.execute(serde_json::json!({})),
        "slow tool execution should complete with tool result",
    );
    assert!(slow_result.is_error);
    assert!(slow_result.output.contains("timed out"), "{slow_result:?}");

    let Some(large_tool) = tools.get("large_echo") else {
        panic!("large output tool should exist");
    };
    let large_result = must(
        large_tool.execute(serde_json::json!({})),
        "large output tool execution should complete with tool result",
    );
    assert!(large_result.is_error);
    assert!(
        large_result.output.contains("exceeded output limit"),
        "{large_result:?}"
    );
}

#[test]
fn process_plugin_surfaces_stderr_for_non_zero_exit() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "stderr-plugin",
        "#!/bin/sh\ncat >/dev/null\necho 'plugin failed cleanly' 1>&2\nexit 9\n",
        r#"
[[native.tools]]
name = "stderr_echo"
description = "surfaces stderr"
command = "bin/echo-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    );

    let (loaded, warnings, _plugins, tools) = load_process_plugins(temp.path(), &["stderr-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);

    let Some(tool) = tools.get("stderr_echo") else {
        panic!("stderr tool should exist");
    };
    let result = must(
        tool.execute(serde_json::json!({})),
        "stderr tool execution should complete with tool result",
    );
    assert!(result.is_error);
    assert_eq!(result.output, "plugin failed cleanly");
}

#[test]
fn process_plugin_rejects_invalid_json_tool_output() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "invalid-json-plugin",
        "#!/bin/sh\ncat >/dev/null\nprintf 'not-json'\n",
        r#"
[[native.tools]]
name = "invalid_json_echo"
description = "returns invalid json"
command = "bin/echo-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    );

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["invalid-json-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);

    let Some(tool) = tools.get("invalid_json_echo") else {
        panic!("invalid-json tool should exist");
    };
    let err = tool
        .execute(serde_json::json!({}))
        .expect_err("invalid JSON should raise execution error");
    assert!(err.to_string().contains("returned invalid JSON"), "{err}");
}

#[test]
fn process_plugin_rejects_working_dir_that_escapes_plugin_directory() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "escaped-working-dir-plugin",
        "#!/bin/sh\nprintf '{\"output\":\"ok\",\"is_error\":false}'\n",
        r#"
[[native.tools]]
name = "escaped_workdir_echo"
description = "should not load"
command = "bin/echo-tool"
working_dir = ".."
timeout_secs = 1
input_schema = { type = "object" }
"#,
    );

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["escaped-working-dir-plugin"]);

    assert!(loaded.manifests.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("working_dir escapes plugin directory"),
        "{warnings:?}"
    );
    assert!(tools.get("escaped_workdir_echo").is_none());
}

#[test]
fn process_plugin_inherits_only_declared_environment_and_uses_working_dir() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let plugin_dir = temp.path().join("plugins").join("env-plugin");
    let working_dir = plugin_dir.join("work");
    if let Err(err) = std::fs::create_dir_all(&working_dir) {
        panic!("create working_dir should succeed: {err}");
    }

    let _plugin_dir = write_process_plugin(
        temp.path(),
        "env-plugin",
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"cwd=%s allowed=%s blocked=%s explicit=%s\",\"is_error\":false}' \"$(pwd)\" \"${KEEP_ME:-missing}\" \"${BLOCK_ME:-missing}\" \"${EXPLICIT_ONLY:-missing}\" \n",
        r#"
[[native.tools]]
name = "env_echo"
description = "reports cwd and env"
command = "bin/echo-tool"
working_dir = "work"
inherit_env = ["KEEP_ME"]
timeout_secs = 1
input_schema = { type = "object" }

[native.tools.env]
EXPLICIT_ONLY = "set-by-manifest"
"#,
    );

    let (loaded, warnings, _plugins, tools) = load_process_plugins(temp.path(), &["env-plugin"]);

    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);

    // SAFETY: tests run in-process and restore env vars before returning.
    unsafe {
        std::env::set_var("KEEP_ME", "allowed-value");
        std::env::set_var("BLOCK_ME", "blocked-value");
    }

    let Some(tool) = tools.get("env_echo") else {
        panic!("env tool should exist");
    };
    let result = must(
        tool.execute(serde_json::json!({})),
        "env tool should execute",
    );

    // SAFETY: restore test process environment after the check.
    unsafe {
        std::env::remove_var("KEEP_ME");
        std::env::remove_var("BLOCK_ME");
    }

    assert_eq!(
        result.output,
        format!(
            "cwd={} allowed=allowed-value blocked=missing explicit=set-by-manifest",
            working_dir.display()
        )
    );
    assert!(!result.is_error);
}

#[test]
fn process_plugin_rejects_invalid_manifest_shape() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let plugin_dir = temp.path().join("plugins").join("invalid-plugin");
    if let Err(err) = std::fs::create_dir_all(&plugin_dir) {
        panic!("create plugin_dir should succeed: {err}");
    }
    if let Err(err) = std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
name = "invalid-plugin"
version = "0.1.0"
description = "invalid plugin"
cortex_version = "1.2.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"
unsupported = true
"#,
    ) {
        panic!("write manifest should succeed: {err}");
    }

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["invalid-plugin"]);

    assert!(loaded.manifests.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("invalid manifest"), "{warnings:?}");
    assert!(tools.get("invalid-plugin").is_none());
}

#[test]
fn process_plugin_rejects_incompatible_cortex_version() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let plugin_dir = temp.path().join("plugins").join("future-plugin");
    if let Err(err) = std::fs::create_dir_all(plugin_dir.join("bin")) {
        panic!("create plugin_dir should succeed: {err}");
    }
    if let Err(err) = std::fs::write(
        plugin_dir.join("bin").join("echo-tool"),
        "#!/bin/sh\nprintf '{\"output\":\"ok\",\"is_error\":false}'\n",
    ) {
        panic!("write tool should succeed: {err}");
    }
    make_executable(&plugin_dir.join("bin").join("echo-tool"));
    if let Err(err) = std::fs::write(
        plugin_dir.join("manifest.toml"),
        r#"
name = "future-plugin"
version = "0.1.0"
description = "requires newer cortex"
cortex_version = ">=9.9.0"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "future_echo"
description = "should not load"
command = "bin/echo-tool"
timeout_secs = 1
input_schema = { type = "object" }
"#,
    ) {
        panic!("write manifest should succeed: {err}");
    }

    let (loaded, warnings, _plugins, tools) = load_process_plugins(temp.path(), &["future-plugin"]);

    assert!(loaded.manifests.is_empty());
    assert_eq!(warnings.len(), 1);
    assert!(
        warnings[0].contains("incompatible with cortex"),
        "{warnings:?}"
    );
    assert!(tools.get("future_echo").is_none());
}

#[test]
fn process_plugin_allows_host_working_dir_only_when_explicitly_opted_in() {
    let temp = match tempfile::tempdir() {
        Ok(value) => value,
        Err(err) => panic!("tempdir should open: {err}"),
    };
    let host_workdir = temp.path().join("host-workdir");
    if let Err(err) = std::fs::create_dir_all(&host_workdir) {
        panic!("create host working_dir should succeed: {err}");
    }

    let workdir_path = host_workdir.display().to_string();
    let _plugin_dir = write_process_plugin(
        temp.path(),
        "host-workdir-plugin",
        "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"cwd=%s\",\"is_error\":false}' \"$(pwd)\"\n",
        &format!(
            r#"
[[native.tools]]
name = "host_workdir_echo"
description = "host working dir tool"
command = "bin/echo-tool"
working_dir = "{workdir_path}"
allow_host_paths = true
timeout_secs = 1
input_schema = {{ type = "object" }}
"#
        ),
    );

    let (loaded, warnings, _plugins, tools) =
        load_process_plugins(temp.path(), &["host-workdir-plugin"]);
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(loaded.manifests.len(), 1);

    let Some(tool) = tools.get("host_workdir_echo") else {
        panic!("host working_dir tool should exist");
    };
    let result = must(
        tool.execute(serde_json::json!({})),
        "host working_dir tool should execute",
    );
    assert_eq!(result.output, format!("cwd={}", host_workdir.display()));
    assert!(!result.is_error);
}
