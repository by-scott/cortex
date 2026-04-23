use cortex_runtime::{MultiToolPlugin, PluginInfo, PluginRegistry, ToolPlugin, ToolRegistry};
use cortex_sdk::{Tool, ToolError, ToolResult};
use cortex_types::config::PluginsConfig;
use cortex_types::plugin::{NativePluginIsolation, PluginManifest, ProcessToolConfig};
use std::path::{Path, PathBuf};

use crate::plugin_manager::{PLUGIN_MANIFEST_FILE, PLUGIN_PROMPTS_DIR, PLUGIN_SKILLS_DIR};

/// Loaded plugin libraries and metadata -- must be kept alive for the duration
/// of the program so that dynamically-loaded symbols remain valid.
pub struct LoadedPlugins {
    /// Shared libraries that must outlive every symbol obtained from them.
    pub libraries: Vec<libloading::Library>,
    /// Successfully loaded manifests.
    pub manifests: Vec<PluginManifest>,
    /// Skill directories discovered from plugins with `capabilities.skills`.
    pub skill_dirs: Vec<PathBuf>,
    /// Prompt directories discovered from plugins with `capabilities.prompts`.
    pub prompt_dirs: Vec<PathBuf>,
}

impl LoadedPlugins {
    /// Returns the number of loaded native library files.
    #[must_use]
    pub const fn library_count(&self) -> usize {
        self.libraries.len()
    }
}

const fn make_loaded(
    libraries: Vec<libloading::Library>,
    manifests: Vec<PluginManifest>,
    skill_dirs: Vec<PathBuf>,
    prompt_dirs: Vec<PathBuf>,
) -> LoadedPlugins {
    LoadedPlugins {
        libraries,
        manifests,
        skill_dirs,
        prompt_dirs,
    }
}

/// Scan the plugins directory, load manifests, and register native tools.
///
/// For each subdirectory of `<cortex_home>/<config.dir>`:
/// 1. Read `manifest.toml`
/// 2. Skip if the plugin name is NOT in `config.enabled`
/// 3. If `capabilities` includes `tools` — load the native `.so`/`.dylib`
/// 4. If `capabilities` includes `skills` — collect `<dir>/skills/`
/// 5. If `capabilities` includes `prompts` — collect `<dir>/prompts/`
///
/// Returns the loaded plugins handle (libraries must stay alive) and warnings.
pub fn load_plugins(
    cortex_home: &Path,
    config: &PluginsConfig,
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
) -> (LoadedPlugins, Vec<String>) {
    let mut libraries = Vec::new();
    let mut manifests = Vec::new();
    let mut skill_dirs = Vec::new();
    let mut prompt_dirs = Vec::new();
    let mut warnings = Vec::new();

    // Plugins are installed globally at `~/.cortex/plugins/`, one level above
    // the instance home (`~/.cortex/default/`).  Check global first, then
    // fall back to instance-local for backward compatibility and testing.
    let instance_dir = cortex_home.join(&config.dir);
    let global_dir = cortex_home
        .parent()
        .map_or_else(|| instance_dir.clone(), |p| p.join(&config.dir));
    let base = if global_dir.is_dir() {
        global_dir
    } else {
        instance_dir
    };

    if !base.is_dir() {
        tracing::debug!(dir = %base.display(), "plugins directory does not exist, skipping");
        return (
            make_loaded(libraries, manifests, skill_dirs, prompt_dirs),
            warnings,
        );
    }

    let entries = match std::fs::read_dir(&base) {
        Ok(e) => e,
        Err(err) => {
            warnings.push(format!(
                "cannot read plugins directory {}: {err}",
                base.display()
            ));
            return (
                make_loaded(libraries, manifests, skill_dirs, prompt_dirs),
                warnings,
            );
        }
    };

    for dir_entry in entries.flatten() {
        let sub = dir_entry.path();
        if !sub.is_dir() {
            continue;
        }
        let result = process_plugin_dir(&sub, config, plugin_registry, tool_registry);
        if let Some(lib) = result.library {
            libraries.push(lib);
        }
        if let Some(manifest) = result.manifest {
            manifests.push(manifest);
        }
        if let Some(skill_dir) = result.skill_dir {
            skill_dirs.push(skill_dir);
        }
        if let Some(prompt_dir) = result.prompt_dir {
            prompt_dirs.push(prompt_dir);
        }
        if let Some(w) = result.warning {
            warnings.push(w);
        }
    }

    (
        make_loaded(libraries, manifests, skill_dirs, prompt_dirs),
        warnings,
    )
}

/// Accumulator for a single plugin directory scan.
struct PluginDirResult {
    library: Option<libloading::Library>,
    manifest: Option<PluginManifest>,
    skill_dir: Option<PathBuf>,
    prompt_dir: Option<PathBuf>,
    warning: Option<String>,
}

/// Process a single plugin subdirectory.
fn process_plugin_dir(
    sub: &Path,
    config: &PluginsConfig,
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
) -> PluginDirResult {
    let empty = PluginDirResult {
        library: None,
        manifest: None,
        skill_dir: None,
        prompt_dir: None,
        warning: None,
    };

    let manifest_path = sub.join(PLUGIN_MANIFEST_FILE);
    if !manifest_path.is_file() {
        tracing::debug!(dir = %sub.display(), "no manifest file, skipping");
        return empty;
    }

    let manifest_text = match std::fs::read_to_string(&manifest_path) {
        Ok(t) => t,
        Err(err) => {
            return PluginDirResult {
                warning: Some(format!("cannot read {}: {err}", manifest_path.display())),
                ..empty
            };
        }
    };

    let manifest: PluginManifest = match toml::from_str(&manifest_text) {
        Ok(m) => m,
        Err(err) => {
            return PluginDirResult {
                warning: Some(format!(
                    "invalid manifest {}: {err}",
                    manifest_path.display()
                )),
                ..empty
            };
        }
    };

    if !config.enabled.iter().any(|e| e == &manifest.name) {
        tracing::debug!(plugin = %manifest.name, "plugin not in enabled list, skipping");
        return empty;
    }

    if let Err(err) = validate_native_sdk_version(&manifest) {
        return PluginDirResult {
            warning: Some(err),
            ..empty
        };
    }

    let mut library = None;
    if manifest.capabilities.tools() {
        match load_native_tools(sub, &manifest, plugin_registry, tool_registry) {
            Ok(lib) => library = lib,
            Err(w) => {
                return PluginDirResult {
                    warning: Some(w),
                    ..empty
                };
            }
        }
    }

    let skill_dir = if manifest.capabilities.skills() {
        let skills_path = sub.join(PLUGIN_SKILLS_DIR);
        if skills_path.is_dir() {
            Some(skills_path)
        } else {
            tracing::warn!(plugin = %manifest.name, "skills capability declared but no skills directory");
            None
        }
    } else {
        None
    };

    let prompt_dir = if manifest.capabilities.prompts() {
        let prompts_path = sub.join(PLUGIN_PROMPTS_DIR);
        if prompts_path.is_dir() {
            Some(prompts_path)
        } else {
            tracing::warn!(plugin = %manifest.name, "prompts capability declared but no prompts directory");
            None
        }
    } else {
        None
    };

    tracing::info!(plugin = %manifest.name, version = %manifest.version, "loaded plugin manifest");
    PluginDirResult {
        library,
        manifest: Some(manifest),
        skill_dir,
        prompt_dir,
        warning: None,
    }
}

/// Attempt to load a native shared library for a tools-capable plugin.
///
/// Returns `Ok(Some(lib))` on success, `Ok(None)` if the `.so` is missing
/// (logged as warning), or `Err(message)` on load failure.
fn load_native_tools(
    sub: &Path,
    manifest: &PluginManifest,
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
) -> Result<Option<libloading::Library>, String> {
    if manifest
        .native
        .as_ref()
        .is_some_and(|native| native.isolation == NativePluginIsolation::Process)
    {
        load_process_tools(sub, manifest, plugin_registry, tool_registry)?;
        return Ok(None);
    }

    let lib_path = resolve_library_path(sub, manifest);

    if !lib_path.exists() {
        tracing::warn!(
            plugin = %manifest.name,
            path = %lib_path.display(),
            "native library not found (plugin installed but .so not yet available)"
        );
        return Ok(None);
    }

    // Try multi-tool entry point first (`cortex_plugin_create_multi`),
    // then fall back to single-tool entry point (`cortex_plugin_create`).
    let lib = unsafe { libloading::Library::new(&lib_path) }.map_err(|e| {
        format!(
            "failed to load native library '{}' from {}: {e}",
            manifest.name,
            lib_path.display()
        )
    })?;

    // Attempt multi-tool plugin
    let multi_sym = b"cortex_plugin_create_multi";
    let multi_loaded =
        unsafe { lib.get::<unsafe extern "C" fn() -> *mut dyn MultiToolPlugin>(multi_sym) };

    if let Ok(create_fn) = multi_loaded {
        let plugin = unsafe { Box::from_raw(create_fn()) };
        let sdk_info = plugin.plugin_info();
        // Bridge SDK PluginInfo → internal PluginInfo
        let internal_info = PluginInfo {
            name: sdk_info.name,
            version: sdk_info.version,
            description: sdk_info.description,
            plugin_type: cortex_types::PluginType::Tool,
        };
        plugin_registry.register_tool_info(&internal_info);
        let tools = plugin.create_tools();
        let tool_count = tools.len();
        for tool in tools {
            tool_registry.register(tool);
        }
        tracing::info!(
            plugin = %manifest.name,
            tools = tool_count,
            "multi-tool plugin loaded"
        );
        return Ok(Some(lib));
    }

    // Fall back to single-tool entry point
    let entry_sym = manifest.native.as_ref().map_or_else(
        || {
            if manifest.entry_symbol.is_empty() {
                b"cortex_plugin_create" as &[u8]
            } else {
                manifest.entry_symbol.as_bytes()
            }
        },
        |n| n.entry.as_bytes(),
    );

    let plugin = unsafe {
        let create_fn: libloading::Symbol<unsafe extern "C" fn() -> *mut dyn ToolPlugin> =
            lib.get(entry_sym).map_err(|e| {
                format!(
                    "symbol lookup failed for '{}': {e}",
                    String::from_utf8_lossy(entry_sym)
                )
            })?;
        Box::from_raw(create_fn())
    };

    plugin_registry.register_tool(plugin.as_ref());
    tool_registry.register(plugin);
    Ok(Some(lib))
}

fn load_process_tools(
    sub: &Path,
    manifest: &PluginManifest,
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
) -> Result<(), String> {
    let Some(native) = &manifest.native else {
        return Err(format!(
            "plugin '{}' requests process isolation but has no [native] section",
            manifest.name
        ));
    };
    if native.tools.is_empty() {
        return Err(format!(
            "plugin '{}' requests process isolation but declares no [[native.tools]]",
            manifest.name
        ));
    }

    let internal_info = PluginInfo {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        plugin_type: cortex_types::PluginType::Tool,
    };
    plugin_registry.register_tool_info(&internal_info);

    for tool in &native.tools {
        validate_process_tool(manifest, sub, tool)?;
        tool_registry.register(Box::new(ProcessPluginTool::new(sub, tool)));
    }

    tracing::info!(
        plugin = %manifest.name,
        tools = native.tools.len(),
        "process-isolated plugin tools registered"
    );
    Ok(())
}

fn validate_process_tool(
    manifest: &PluginManifest,
    sub: &Path,
    tool: &ProcessToolConfig,
) -> Result<(), String> {
    if tool.name.trim().is_empty() {
        return Err(format!(
            "plugin '{}' declares a process tool with an empty name",
            manifest.name
        ));
    }
    if tool.description.trim().is_empty() {
        return Err(format!(
            "plugin '{}' process tool '{}' has an empty description",
            manifest.name, tool.name
        ));
    }
    let command = resolve_process_command(sub, &tool.command);
    if !command.is_file() {
        return Err(format!(
            "plugin '{}' process tool '{}' command not found: {}",
            manifest.name,
            tool.name,
            command.display()
        ));
    }
    Ok(())
}

fn resolve_process_command(sub: &Path, command: &str) -> PathBuf {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        path
    } else {
        sub.join(path)
    }
}

struct ProcessPluginTool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
    command: PathBuf,
    args: Vec<String>,
    timeout_secs: Option<u64>,
}

impl ProcessPluginTool {
    fn new(sub: &Path, config: &ProcessToolConfig) -> Self {
        Self {
            name: Box::leak(config.name.clone().into_boxed_str()),
            description: Box::leak(config.description.clone().into_boxed_str()),
            input_schema: config.input_schema.clone(),
            command: resolve_process_command(sub, &config.command),
            args: config.args.clone(),
            timeout_secs: config.timeout_secs,
        }
    }
}

impl Tool for ProcessPluginTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let request = serde_json::json!({
            "tool": self.name,
            "input": input,
        });
        let mut child = std::process::Command::new(&self.command)
            .args(&self.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to spawn process-isolated tool '{}': {e}",
                    self.name
                ))
            })?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            serde_json::to_writer(&mut stdin, &request).map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to encode request for process-isolated tool '{}': {e}",
                    self.name
                ))
            })?;
            stdin.write_all(b"\n").map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to write request for process-isolated tool '{}': {e}",
                    self.name
                ))
            })?;
        }

        let output = child.wait_with_output().map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "process-isolated tool '{}' failed to wait: {e}",
                self.name
            ))
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(ToolResult::error(if stderr.is_empty() {
                format!(
                    "process-isolated tool '{}' exited with status {}",
                    self.name, output.status
                )
            } else {
                stderr
            }));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        decode_process_tool_result(self.name, stdout.trim())
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.timeout_secs
    }
}

fn decode_process_tool_result(tool_name: &str, stdout: &str) -> Result<ToolResult, ToolError> {
    if stdout.is_empty() {
        return Ok(ToolResult::success(""));
    }

    let value: serde_json::Value = serde_json::from_str(stdout).map_err(|e| {
        ToolError::ExecutionFailed(format!(
            "process-isolated tool '{tool_name}' returned invalid JSON: {e}"
        ))
    })?;

    if let Some(s) = value.as_str() {
        return Ok(ToolResult::success(s));
    }

    let output = value
        .get("output")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "process-isolated tool '{tool_name}' must return a JSON string or object with string field 'output'"
            ))
        })?;
    let is_error = value
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    if is_error {
        Ok(ToolResult::error(output))
    } else {
        Ok(ToolResult::success(output))
    }
}

/// Resolve the shared library path from manifest or naming convention.
fn resolve_library_path(sub: &Path, manifest: &PluginManifest) -> PathBuf {
    if let Some(ref native) = manifest.native {
        return sub.join(&native.library);
    }
    let lib_name = format!("lib{}", manifest.name.replace('-', "_"));
    let so_path = sub.join(format!("{lib_name}.so"));
    if so_path.exists() {
        return so_path;
    }
    let dylib_path = sub.join(format!("{lib_name}.dylib"));
    if dylib_path.exists() {
        return dylib_path;
    }
    so_path // Return .so path (will fail exists check in caller)
}

fn validate_native_sdk_version(manifest: &PluginManifest) -> Result<(), String> {
    let Some(native) = &manifest.native else {
        return Ok(());
    };
    if native.abi_revision != cortex_sdk::ABI_REVISION {
        return Err(format!(
            "plugin '{}' declares native ABI revision {} but daemon requires {}",
            manifest.name,
            native.abi_revision,
            cortex_sdk::ABI_REVISION
        ));
    }
    if native.sdk_version.trim().is_empty() {
        return Ok(());
    }
    let expected = major_minor(cortex_sdk::SDK_VERSION);
    let declared = major_minor(&native.sdk_version);
    if expected == declared {
        Ok(())
    } else {
        Err(format!(
            "plugin '{}' declares cortex-sdk {} but daemon requires {}",
            manifest.name,
            native.sdk_version,
            cortex_sdk::SDK_VERSION
        ))
    }
}

fn major_minor(version: &str) -> Option<(u64, u64)> {
    let mut parts = version.trim_start_matches('v').split('.');
    Some((parts.next()?.parse().ok()?, parts.next()?.parse().ok()?))
}

// ── Tests ──────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_config_loads_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PluginsConfig::default();
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert!(warnings.is_empty());
        assert_eq!(plugin_reg.count(), 0);
        assert_eq!(loaded.library_count(), 0);
        assert!(loaded.manifests.is_empty());
    }

    #[test]
    fn nonexistent_plugins_dir_produces_no_warnings() {
        let tmp = tempfile::tempdir().unwrap();
        let config = PluginsConfig {
            dir: "nonexistent_plugins".into(),
            enabled: Vec::new(),
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert!(warnings.is_empty());
        assert_eq!(loaded.library_count(), 0);
    }

    #[test]
    fn unenabled_plugin_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("my-plugin");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            "name = \"my-plugin\"\nversion = \"1.1.0\"\ndescription = \"test\"\n\n[capabilities]\nprovides = [\"tools\"]\n",
        )
        .unwrap();

        // Plugin exists on disk but is NOT in the enabled list → skipped.
        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: Vec::new(),
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert!(warnings.is_empty());
        assert!(loaded.manifests.is_empty());
    }

    #[test]
    fn skills_and_prompts_dirs_collected() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("content-plugin");
        std::fs::create_dir_all(pd.join(PLUGIN_SKILLS_DIR)).unwrap();
        std::fs::create_dir_all(pd.join(PLUGIN_PROMPTS_DIR)).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            "name = \"content-plugin\"\nversion = \"0.1.0\"\ndescription = \"provides skills and prompts\"\n\n[capabilities]\nprovides = [\"skills\", \"prompts\"]\n",
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["content-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert!(warnings.is_empty());
        assert_eq!(loaded.manifests.len(), 1);
        assert_eq!(loaded.manifests[0].name, "content-plugin");
        assert_eq!(loaded.skill_dirs.len(), 1);
        assert_eq!(loaded.prompt_dirs.len(), 1);
    }

    #[test]
    fn incompatible_sdk_version_blocks_enabled_native_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("native-plugin");
        std::fs::create_dir_all(pd.join("lib")).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            "name = \"native-plugin\"\nversion = \"0.1.0\"\ndescription = \"native\"\n\n[capabilities]\nprovides = [\"tools\"]\n\n[native]\nlibrary = \"lib/plugin.so\"\nsdk_version = \"99.0.0\"\n",
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["native-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("declares cortex-sdk"));
        assert!(loaded.manifests.is_empty());
        assert_eq!(loaded.library_count(), 0);
    }

    #[test]
    fn incompatible_native_abi_revision_blocks_enabled_plugin() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("native-plugin");
        std::fs::create_dir_all(pd.join("lib")).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            "name = \"native-plugin\"\nversion = \"0.1.0\"\ndescription = \"native\"\n\n[capabilities]\nprovides = [\"tools\"]\n\n[native]\nlibrary = \"lib/plugin.so\"\nabi_revision = 99\n",
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["native-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("native ABI revision"));
        assert!(loaded.manifests.is_empty());
        assert_eq!(loaded.library_count(), 0);
    }

    #[test]
    fn tools_without_so_produces_no_error() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("native-plugin");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            "name = \"native-plugin\"\nversion = \"0.1.0\"\ndescription = \"native tool without .so yet\"\n\n[capabilities]\nprovides = [\"tools\"]\n",
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["native-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert!(warnings.is_empty());
        assert_eq!(loaded.manifests.len(), 1);
        assert_eq!(loaded.library_count(), 0);
    }

    #[test]
    fn process_isolated_plugin_registers_proxy_tool() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("process-plugin");
        let bin = pd.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let tool_path = bin.join("echo-tool");
        std::fs::write(
            &tool_path,
            "#!/bin/sh\ncat >/dev/null\nprintf '{\"output\":\"isolated ok\",\"is_error\":false}'\n",
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut perms = std::fs::metadata(&tool_path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&tool_path, perms).unwrap();
        }
        std::fs::write(
            pd.join("manifest.toml"),
            r#"
name = "process-plugin"
version = "0.1.0"
description = "process isolated"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"

[[native.tools]]
name = "external_echo"
description = "echo through process isolation"
command = "bin/echo-tool"
input_schema = { type = "object" }
"#,
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["process-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(loaded.manifests.len(), 1);
        assert_eq!(loaded.library_count(), 0);
        assert_eq!(plugin_reg.count(), 1);
        let tool = tool_reg.get("external_echo").unwrap();
        let result = tool.execute(serde_json::json!({"text": "hello"})).unwrap();
        assert_eq!(result.output, "isolated ok");
        assert!(!result.is_error);
    }

    #[test]
    fn process_isolated_plugin_requires_declared_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("process-plugin");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(
            pd.join("manifest.toml"),
            r#"
name = "process-plugin"
version = "0.1.0"
description = "process isolated"

[capabilities]
provides = ["tools"]

[native]
isolation = "process"
"#,
        )
        .unwrap();

        let config = PluginsConfig {
            dir: "plugins".into(),
            enabled: vec!["process-plugin".into()],
        };
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);

        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("declares no [[native.tools]]"));
        assert!(loaded.manifests.is_empty());
        assert_eq!(tool_reg.len(), 0);
    }

    #[test]
    fn invalid_manifest_produces_warning() {
        let tmp = tempfile::tempdir().unwrap();
        let pd = tmp.path().join("plugins").join("bad-plugin");
        std::fs::create_dir_all(&pd).unwrap();
        std::fs::write(pd.join("manifest.toml"), "this is not valid toml {{{").unwrap();

        let config = PluginsConfig::default();
        let mut plugin_reg = PluginRegistry::new();
        let mut tool_reg = ToolRegistry::new();

        let (loaded, warnings) = load_plugins(tmp.path(), &config, &mut plugin_reg, &mut tool_reg);
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("invalid manifest"));
        assert!(loaded.manifests.is_empty());
    }
}
