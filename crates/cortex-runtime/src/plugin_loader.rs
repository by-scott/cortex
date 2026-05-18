use crate::{PluginInfo, PluginRegistry, ToolRegistry};
use cortex_sdk::{
    CortexBuffer, CortexHostApi, CortexPluginApi, Tool, ToolCapabilities, ToolError, ToolResult,
};
use cortex_types::config::PluginsConfig;
use cortex_types::plugin::{
    NativePluginIsolation, PluginManifest, PluginPackageMetadata, check_plugin_version,
};
use serde::Deserialize;
use std::ffi::c_void;
use std::path::{Path, PathBuf};

pub const PLUGIN_MANIFEST_FILE: &str = "manifest.toml";
pub const PLUGIN_PACKAGE_FILE: &str = "package.toml";
pub const PLUGIN_SKILLS_DIR: &str = "skills";
pub const PLUGIN_PROMPTS_DIR: &str = "prompts";
const HOST_CORTEX_VERSION: &str = env!("CARGO_PKG_VERSION");

mod process_tools;

use process_tools::{boxed_process_tool, load_process_tools, validate_process_tool};

fn should_scan_plugin_dir(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|value| value.to_str()) else {
        return false;
    };
    let is_backup = Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("bak"));
    !name.starts_with('.') && !is_backup
}

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

    let base = plugin_base_dir(cortex_home, config);

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
        if !sub.is_dir() || !should_scan_plugin_dir(&sub) {
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

#[must_use]
pub fn plugin_base_dir(cortex_home: &Path, config: &PluginsConfig) -> PathBuf {
    // Plugins are installed globally at `~/.cortex/plugins/`, one level above
    // the instance home (`~/.cortex/default/`).  Check global first, then
    // Fall back to instance-local plugin directories for test and development
    // instances.
    let instance_dir = cortex_home.join(&config.dir);
    let global_dir = cortex_home
        .parent()
        .map_or_else(|| instance_dir.clone(), |p| p.join(&config.dir));
    if global_dir.is_dir() {
        global_dir
    } else {
        instance_dir
    }
}

pub fn reload_process_plugin_tools(
    cortex_home: &Path,
    config: &PluginsConfig,
    tool_registry: &ToolRegistry,
) -> Vec<String> {
    let base = plugin_base_dir(cortex_home, config);
    let mut warnings = Vec::new();
    if !base.is_dir() {
        return warnings;
    }

    let entries = match std::fs::read_dir(&base) {
        Ok(entries) => entries,
        Err(err) => {
            warnings.push(format!(
                "cannot read plugins directory {}: {err}",
                base.display()
            ));
            return warnings;
        }
    };

    for dir_entry in entries.flatten() {
        let sub = dir_entry.path();
        if !sub.is_dir() || !should_scan_plugin_dir(&sub) {
            continue;
        }
        if let Err(err) = reload_process_plugin_dir(&sub, config, tool_registry) {
            warnings.push(err);
        }
    }
    warnings
}

fn reload_process_plugin_dir(
    sub: &Path,
    config: &PluginsConfig,
    tool_registry: &ToolRegistry,
) -> Result<(), String> {
    if !sub.join(PLUGIN_MANIFEST_FILE).is_file() {
        return Ok(());
    }
    let manifest = read_installed_manifest(sub)?;
    ensure_manifest_targets_host(&manifest)?;
    ensure_manifest_governance(&manifest)?;

    let Some(native) = &manifest.native else {
        if !config.enabled.iter().any(|e| e == &manifest.name) {
            tool_registry.unregister_plugin_tools(&manifest.name);
        }
        return Ok(());
    };
    if native.isolation != NativePluginIsolation::Process {
        if !config.enabled.iter().any(|e| e == &manifest.name) {
            return Ok(());
        }
        tracing::warn!(
            plugin = %manifest.name,
            "in-process plugin changes require daemon restart"
        );
        return Ok(());
    }
    if !config.enabled.iter().any(|e| e == &manifest.name) {
        tool_registry.unregister_plugin_tools(&manifest.name);
        return Ok(());
    }
    if native.tools.is_empty() {
        return Err(format!(
            "plugin '{}' requests process isolation but declares no [[native.tools]]",
            manifest.name
        ));
    }
    for tool in &native.tools {
        validate_process_tool(&manifest, sub, tool)?;
    }
    tool_registry.unregister_plugin_tools(&manifest.name);
    for tool in &native.tools {
        tool_registry.register_from_plugin_live(
            &manifest.name,
            boxed_process_tool(sub, &manifest.capabilities, tool),
        );
    }
    tracing::info!(
        plugin = %manifest.name,
        tools = native.tools.len(),
        "process-isolated plugin tools hot-reloaded"
    );
    Ok(())
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

    let manifest = match read_installed_manifest(sub) {
        Ok(manifest) => manifest,
        Err(err) => {
            return PluginDirResult {
                warning: Some(err),
                ..empty
            };
        }
    };
    if let Err(err) = ensure_manifest_targets_host(&manifest) {
        return PluginDirResult {
            warning: Some(err),
            ..empty
        };
    }
    if let Err(err) = ensure_manifest_governance(&manifest) {
        return PluginDirResult {
            warning: Some(err),
            ..empty
        };
    }

    if !config.enabled.iter().any(|e| e == &manifest.name) {
        tracing::debug!(plugin = %manifest.name, "plugin not in enabled list, skipping");
        return empty;
    }

    if let Err(err) = validate_native_boundary(&manifest) {
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

/// Read an installed plugin manifest and merge package metadata carried in
/// `package.toml`, when present.
///
/// # Errors
/// Returns an error if `manifest.toml` or `package.toml` cannot be read or
/// parsed.
pub fn read_installed_manifest(plugin_dir: &Path) -> Result<PluginManifest, String> {
    let manifest_path = plugin_dir.join(PLUGIN_MANIFEST_FILE);
    let manifest_text = std::fs::read_to_string(&manifest_path)
        .map_err(|err| format!("cannot read {}: {err}", manifest_path.display()))?;
    let mut manifest: PluginManifest = toml::from_str(&manifest_text)
        .map_err(|err| format!("invalid manifest {}: {err}", manifest_path.display()))?;
    merge_package_metadata(plugin_dir, &mut manifest)?;
    Ok(manifest)
}

fn merge_package_metadata(plugin_dir: &Path, manifest: &mut PluginManifest) -> Result<(), String> {
    let package_path = plugin_dir.join(PLUGIN_PACKAGE_FILE);
    if !package_path.is_file() {
        return Ok(());
    }
    let package_text = std::fs::read_to_string(&package_path)
        .map_err(|err| format!("cannot read {}: {err}", package_path.display()))?;
    let package: PluginPackageMetadata = toml::from_str(&package_text)
        .map_err(|err| format!("invalid package metadata {}: {err}", package_path.display()))?;
    manifest.package = package;
    Ok(())
}

fn ensure_manifest_targets_host(manifest: &PluginManifest) -> Result<(), String> {
    let version_check = check_plugin_version(manifest, HOST_CORTEX_VERSION);
    if version_check.accepted {
        Ok(())
    } else {
        Err(format!(
            "plugin '{}' targets a different cortex version than {}{}",
            manifest.name,
            HOST_CORTEX_VERSION,
            version_check
                .reason
                .as_deref()
                .map_or(String::new(), |reason| format!(": {reason}"))
        ))
    }
}

fn ensure_manifest_governance(manifest: &PluginManifest) -> Result<(), String> {
    manifest.validate_governance()
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

    let lib = unsafe { libloading::Library::new(&lib_path) }.map_err(|e| {
        format!(
            "failed to load native library '{}' from {}: {e}",
            manifest.name,
            lib_path.display()
        )
    })?;

    let plugin = load_stable_native_plugin(&lib, manifest)?;
    plugin_registry.register_tool_info(&plugin.info);
    let tool_count = plugin.tool_count;
    for tool in plugin.tools {
        tool_registry.register_from_plugin(&manifest.name, tool);
    }
    tracing::info!(
        plugin = %manifest.name,
        tools = tool_count,
        "stable native plugin loaded"
    );
    Ok(Some(lib))
}

type NativeInitFn = unsafe extern "C" fn(*const CortexHostApi, *mut CortexPluginApi) -> i32;

#[derive(Deserialize)]
struct NativeToolDescriptor {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    timeout_secs: Option<u64>,
    capabilities: ToolCapabilities,
}

struct LoadedStableNativePlugin {
    info: PluginInfo,
    tools: Vec<Box<dyn Tool>>,
    tool_count: usize,
}

struct StableNativePluginHandle {
    plugin: usize,
    plugin_info: unsafe extern "C" fn(*mut c_void) -> CortexBuffer,
    tool_count: unsafe extern "C" fn(*mut c_void) -> usize,
    tool_descriptor: unsafe extern "C" fn(*mut c_void, usize) -> CortexBuffer,
    tool_execute:
        unsafe extern "C" fn(*mut c_void, CortexBuffer, CortexBuffer, CortexBuffer) -> CortexBuffer,
    plugin_drop: unsafe extern "C" fn(*mut c_void),
    buffer_free: unsafe extern "C" fn(CortexBuffer),
}

impl StableNativePluginHandle {
    fn required_plugin_info(&self) -> Result<cortex_sdk::PluginInfo, String> {
        // SAFETY: the function pointer and plugin state come from a successful
        // `cortex_plugin_init` call and are owned by this handle.
        let buffer = unsafe { (self.plugin_info)(self.plugin()) };
        let json = self.take_buffer(buffer)?;
        serde_json::from_str(&json).map_err(|err| format!("invalid plugin info JSON: {err}"))
    }

    fn required_tool_count(&self) -> usize {
        // SAFETY: see `required_plugin_info`.
        unsafe { (self.tool_count)(self.plugin()) }
    }

    fn required_tool_descriptor(&self, index: usize) -> Result<NativeToolDescriptor, String> {
        // SAFETY: see `required_plugin_info`.
        let buffer = unsafe { (self.tool_descriptor)(self.plugin(), index) };
        let json = self.take_buffer(buffer)?;
        if json.is_empty() {
            return Err(format!(
                "native ABI returned an empty descriptor for tool index {index}"
            ));
        }
        serde_json::from_str(&json)
            .map_err(|err| format!("invalid native tool descriptor JSON at index {index}: {err}"))
    }

    fn execute(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        invocation: &cortex_sdk::InvocationContext,
    ) -> Result<ToolResult, ToolError> {
        let input_json = serde_json::to_string(input).map_err(|err| {
            ToolError::ExecutionFailed(format!("failed to encode native tool input: {err}"))
        })?;
        let invocation_json = serde_json::to_string(invocation).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "failed to encode native tool invocation context: {err}"
            ))
        })?;
        let tool_name_buffer = borrowed_buffer(tool_name);
        let input_buffer = borrowed_buffer(&input_json);
        let invocation_buffer = borrowed_buffer(&invocation_json);
        // SAFETY: the borrowed buffers live until the function returns; the
        // plugin must not retain inbound pointers beyond the call.
        let output = unsafe {
            (self.tool_execute)(
                self.plugin(),
                tool_name_buffer,
                input_buffer,
                invocation_buffer,
            )
        };
        let json = self
            .take_buffer(output)
            .map_err(ToolError::ExecutionFailed)?;
        serde_json::from_str(&json).map_err(|err| {
            ToolError::ExecutionFailed(format!(
                "native tool '{tool_name}' returned invalid result JSON: {err}"
            ))
        })
    }

    fn take_buffer(&self, buffer: CortexBuffer) -> Result<String, String> {
        // SAFETY: the buffer was returned by this plugin's ABI table.
        let text = unsafe { buffer.as_str() }
            .map_err(|err| format!("native ABI returned non-UTF8 data: {err}"))?
            .to_string();
        // SAFETY: the table's `buffer_free` owns buffers returned by table
        // functions and must be called exactly once.
        unsafe { (self.buffer_free)(buffer) };
        Ok(text)
    }

    const fn plugin(&self) -> *mut c_void {
        self.plugin as *mut c_void
    }
}

impl Drop for StableNativePluginHandle {
    fn drop(&mut self) {
        // SAFETY: this handle owns the plugin state returned by init.
        unsafe { (self.plugin_drop)(self.plugin()) };
        self.plugin = 0;
    }
}

struct StableNativeTool {
    handle: std::sync::Arc<StableNativePluginHandle>,
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
    timeout_secs: Option<u64>,
    capabilities: ToolCapabilities,
}

impl Tool for StableNativeTool {
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
        let invocation = cortex_sdk::InvocationContext {
            tool_name: self.name.to_string(),
            session_id: None,
            actor: None,
            source: None,
            execution_scope: cortex_sdk::ExecutionScope::Foreground,
        };
        self.handle.execute(self.name, &input, &invocation)
    }

    fn execute_with_runtime(
        &self,
        input: serde_json::Value,
        runtime: &dyn cortex_sdk::ToolRuntime,
    ) -> Result<ToolResult, ToolError> {
        self.handle.execute(self.name, &input, runtime.invocation())
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.timeout_secs
    }

    fn capabilities(&self) -> ToolCapabilities {
        self.capabilities.clone()
    }
}

fn load_stable_native_plugin(
    lib: &libloading::Library,
    manifest: &PluginManifest,
) -> Result<LoadedStableNativePlugin, String> {
    let init = unsafe {
        lib.get::<NativeInitFn>(b"cortex_plugin_init").map_err(|err| {
            format!(
                "plugin '{}' does not export required stable native symbol cortex_plugin_init: {err}",
                manifest.name
            )
        })?
    };
    let host = CortexHostApi {
        abi_version: cortex_sdk::NATIVE_ABI_VERSION,
    };
    let mut api = CortexPluginApi::empty();
    // SAFETY: `init` is the stable native ABI entry point. It writes a complete
    // function table into `api` or returns a non-zero error code.
    let status = unsafe { init(&raw const host, &raw mut api) };
    if status != 0 {
        return Err(format!(
            "plugin '{}' rejected native ABI initialization with status {status}",
            manifest.name
        ));
    }
    if api.abi_version != cortex_sdk::NATIVE_ABI_VERSION {
        return Err(format!(
            "plugin '{}' initialized native ABI version {} but daemon requires {}",
            manifest.name,
            api.abi_version,
            cortex_sdk::NATIVE_ABI_VERSION
        ));
    }
    if api.plugin.is_null() {
        return Err(format!(
            "plugin '{}' returned a null native plugin state",
            manifest.name
        ));
    }

    let handle = std::sync::Arc::new(
        stable_native_plugin_handle_from_api(&api)
            .map_err(|err| format!("plugin '{}' {err}", manifest.name))?,
    );
    let sdk_info = handle.required_plugin_info()?;
    let info = PluginInfo {
        name: sdk_info.name,
        version: sdk_info.version,
        description: sdk_info.description,
        plugin_type: cortex_types::PluginType::Tool,
    };
    let tool_count = handle.required_tool_count();
    let mut tools: Vec<Box<dyn Tool>> = Vec::with_capacity(tool_count);
    for index in 0..tool_count {
        let descriptor = handle.required_tool_descriptor(index)?;
        tools.push(Box::new(StableNativeTool {
            handle: handle.clone(),
            name: Box::leak(descriptor.name.into_boxed_str()),
            description: Box::leak(descriptor.description.into_boxed_str()),
            input_schema: descriptor.input_schema,
            timeout_secs: descriptor.timeout_secs,
            capabilities: descriptor.capabilities,
        }));
    }
    Ok(LoadedStableNativePlugin {
        info,
        tools,
        tool_count,
    })
}

fn stable_native_plugin_handle_from_api(
    api: &CortexPluginApi,
) -> Result<StableNativePluginHandle, String> {
    Ok(StableNativePluginHandle {
        plugin: api.plugin as usize,
        plugin_info: api
            .plugin_info
            .ok_or_else(|| "native ABI table is missing plugin_info".to_string())?,
        tool_count: api
            .tool_count
            .ok_or_else(|| "native ABI table is missing tool_count".to_string())?,
        tool_descriptor: api
            .tool_descriptor
            .ok_or_else(|| "native ABI table is missing tool_descriptor".to_string())?,
        tool_execute: api
            .tool_execute
            .ok_or_else(|| "native ABI table is missing tool_execute".to_string())?,
        plugin_drop: api
            .plugin_drop
            .ok_or_else(|| "native ABI table is missing plugin_drop".to_string())?,
        buffer_free: api
            .buffer_free
            .ok_or_else(|| "native ABI table is missing buffer_free".to_string())?,
    })
}

const fn borrowed_buffer(value: &str) -> CortexBuffer {
    CortexBuffer {
        ptr: value.as_ptr().cast_mut(),
        len: value.len(),
        cap: 0,
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

fn validate_native_boundary(manifest: &PluginManifest) -> Result<(), String> {
    let Some(native) = &manifest.native else {
        return Ok(());
    };
    if native.isolation == NativePluginIsolation::Process {
        return Ok(());
    }
    if native.isolation != NativePluginIsolation::TrustedInProcess {
        return Err(format!(
            "plugin '{}' declares an unsupported native isolation boundary",
            manifest.name
        ));
    }
    if native.abi_version == Some(cortex_sdk::NATIVE_ABI_VERSION) {
        Ok(())
    } else {
        Err(format!(
            "plugin '{}' declares native ABI {:?} but daemon requires {}",
            manifest.name,
            native.abi_version,
            cortex_sdk::NATIVE_ABI_VERSION
        ))
    }
}
