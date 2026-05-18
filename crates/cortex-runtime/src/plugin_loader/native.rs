use std::ffi::c_void;

use cortex_sdk::{
    CortexBuffer, CortexHostApi, CortexPluginApi, Tool, ToolCapabilities, ToolError, ToolResult,
};
use cortex_types::plugin::PluginManifest;
use serde::Deserialize;

use crate::PluginInfo;

type NativeInitFn = unsafe extern "C" fn(*const CortexHostApi, *mut CortexPluginApi) -> i32;

#[derive(Deserialize)]
struct NativeToolDescriptor {
    name: String,
    description: String,
    input_schema: serde_json::Value,
    timeout_secs: Option<u64>,
    capabilities: ToolCapabilities,
}

pub(super) struct LoadedStableNativePlugin {
    pub(super) info: PluginInfo,
    pub(super) tools: Vec<Box<dyn Tool>>,
    pub(super) tool_count: usize,
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

pub(super) fn load_stable_native_plugin(
    lib: &libloading::Library,
    manifest: &PluginManifest,
) -> Result<LoadedStableNativePlugin, String> {
    let init = unsafe {
        lib.get::<NativeInitFn>(b"cortex_plugin_init")
            .map_err(|err| {
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
