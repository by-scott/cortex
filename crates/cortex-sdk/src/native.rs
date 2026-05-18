use crate::{InvocationContext, MultiToolPlugin, Tool, ToolCapabilities, ToolResult, ToolRuntime};
use serde::Serialize;
use std::ffi::c_void;

/// Native ABI-owned byte buffer.
///
/// All strings and JSON values that cross the stable native ABI boundary use
/// this representation. Buffers returned by the plugin must be released by
/// calling the table's `buffer_free` function.
#[repr(C)]
pub struct CortexBuffer {
    /// Pointer to UTF-8 bytes.
    pub ptr: *mut u8,
    /// Number of initialized bytes at `ptr`.
    pub len: usize,
    /// Allocation capacity needed to reconstruct and free the buffer.
    pub cap: usize,
}

impl CortexBuffer {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            len: 0,
            cap: 0,
        }
    }
}

impl From<String> for CortexBuffer {
    fn from(value: String) -> Self {
        let mut bytes = value.into_bytes();
        let buffer = Self {
            ptr: bytes.as_mut_ptr(),
            len: bytes.len(),
            cap: bytes.capacity(),
        };
        std::mem::forget(bytes);
        buffer
    }
}

impl CortexBuffer {
    /// Read this buffer as UTF-8.
    ///
    /// # Errors
    /// Returns a UTF-8 error when the buffer contains invalid UTF-8 bytes.
    ///
    /// # Safety
    /// The caller must ensure `ptr` is valid for `len` bytes and remains alive
    /// for the duration of this call.
    pub const unsafe fn as_str(&self) -> Result<&str, std::str::Utf8Error> {
        if self.ptr.is_null() || self.len == 0 {
            return Ok("");
        }
        // SAFETY: upheld by the caller.
        let bytes = unsafe { std::slice::from_raw_parts(self.ptr.cast_const(), self.len) };
        std::str::from_utf8(bytes)
    }
}

/// Free a buffer allocated by this SDK.
///
/// # Safety
/// The buffer must have been returned by this SDK's ABI helpers and must not be
/// freed more than once.
pub unsafe extern "C" fn cortex_buffer_free(buffer: CortexBuffer) {
    if buffer.ptr.is_null() {
        return;
    }
    // SAFETY: the caller guarantees this buffer came from `CortexBuffer::from_string`.
    unsafe {
        drop(Vec::from_raw_parts(buffer.ptr, buffer.len, buffer.cap));
    }
}

/// Host table supplied to a native plugin during initialization.
#[repr(C)]
pub struct CortexHostApi {
    /// Runtime-supported native ABI version.
    pub abi_version: u32,
}

/// Function table exported by a native plugin.
#[repr(C)]
pub struct CortexPluginApi {
    /// Plugin-supported native ABI version.
    pub abi_version: u32,
    /// Opaque plugin state owned by the plugin.
    pub plugin: *mut c_void,
    /// Return [`crate::PluginInfo`] encoded as JSON.
    pub plugin_info: Option<unsafe extern "C" fn(*mut c_void) -> CortexBuffer>,
    /// Return the number of tools exposed by the plugin.
    pub tool_count: Option<unsafe extern "C" fn(*mut c_void) -> usize>,
    /// Return one tool descriptor encoded as JSON.
    pub tool_descriptor: Option<unsafe extern "C" fn(*mut c_void, usize) -> CortexBuffer>,
    /// Execute a tool. The name, input, and invocation context are UTF-8 JSON
    /// buffers except `tool_name`, which is a UTF-8 string.
    pub tool_execute: Option<
        unsafe extern "C" fn(*mut c_void, CortexBuffer, CortexBuffer, CortexBuffer) -> CortexBuffer,
    >,
    /// Drop plugin-owned state.
    pub plugin_drop: Option<unsafe extern "C" fn(*mut c_void)>,
    /// Free buffers returned by plugin functions.
    pub buffer_free: Option<unsafe extern "C" fn(CortexBuffer)>,
}

impl CortexPluginApi {
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            abi_version: 0,
            plugin: std::ptr::null_mut(),
            plugin_info: None,
            tool_count: None,
            tool_descriptor: None,
            tool_execute: None,
            plugin_drop: None,
            buffer_free: None,
        }
    }
}

#[derive(Serialize)]
struct ToolDescriptor<'a> {
    name: &'a str,
    description: &'a str,
    input_schema: serde_json::Value,
    timeout_secs: Option<u64>,
    capabilities: ToolCapabilities,
}

struct NoopToolRuntime {
    invocation: InvocationContext,
}

impl ToolRuntime for NoopToolRuntime {
    fn invocation(&self) -> &InvocationContext {
        &self.invocation
    }

    fn emit_progress(&self, _message: &str) {}

    fn emit_observer(&self, _source: Option<&str>, _content: &str) {}
}

#[doc(hidden)]
pub struct NativePluginState {
    plugin: Box<dyn MultiToolPlugin>,
    tools: Vec<Box<dyn Tool>>,
}

impl NativePluginState {
    #[must_use]
    pub fn new(plugin: Box<dyn MultiToolPlugin>) -> Self {
        let tools = plugin.create_tools();
        Self { plugin, tools }
    }
}

fn json_buffer<T: Serialize>(value: &T) -> CortexBuffer {
    match serde_json::to_string(value) {
        Ok(json) => CortexBuffer::from(json),
        Err(err) => CortexBuffer::from(
            serde_json::json!({
                "output": format!("native ABI serialization error: {err}"),
                "media": [],
                "is_error": true
            })
            .to_string(),
        ),
    }
}

#[doc(hidden)]
pub unsafe extern "C" fn native_plugin_info(state: *mut c_void) -> CortexBuffer {
    if state.is_null() {
        return CortexBuffer::empty();
    }
    // SAFETY: the pointer is created by `export_plugin!` and remains owned by
    // the plugin until `native_plugin_drop`.
    let state = unsafe { &*state.cast::<NativePluginState>() };
    json_buffer(&state.plugin.plugin_info())
}

#[doc(hidden)]
pub unsafe extern "C" fn native_tool_count(state: *mut c_void) -> usize {
    if state.is_null() {
        return 0;
    }
    // SAFETY: see `native_plugin_info`.
    let state = unsafe { &*state.cast::<NativePluginState>() };
    state.tools.len()
}

#[doc(hidden)]
pub unsafe extern "C" fn native_tool_descriptor(state: *mut c_void, index: usize) -> CortexBuffer {
    if state.is_null() {
        return CortexBuffer::empty();
    }
    // SAFETY: see `native_plugin_info`.
    let state = unsafe { &*state.cast::<NativePluginState>() };
    let Some(tool) = state.tools.get(index) else {
        return CortexBuffer::empty();
    };
    let descriptor = ToolDescriptor {
        name: tool.name(),
        description: tool.description(),
        input_schema: tool.input_schema(),
        timeout_secs: tool.timeout_secs(),
        capabilities: tool.capabilities(),
    };
    json_buffer(&descriptor)
}

#[doc(hidden)]
pub unsafe extern "C" fn native_tool_execute(
    state: *mut c_void,
    tool_name: CortexBuffer,
    input_json: CortexBuffer,
    invocation_json: CortexBuffer,
) -> CortexBuffer {
    if state.is_null() {
        return json_buffer(&ToolResult::error("native plugin state is null"));
    }
    // SAFETY: inbound buffers are supplied by the runtime and valid for this call.
    let tool_name = match unsafe { tool_name.as_str() } {
        Ok(value) => value,
        Err(err) => return json_buffer(&ToolResult::error(format!("invalid tool name: {err}"))),
    };
    // SAFETY: inbound buffers are supplied by the runtime and valid for this call.
    let input_json = match unsafe { input_json.as_str() } {
        Ok(value) => value,
        Err(err) => return json_buffer(&ToolResult::error(format!("invalid input JSON: {err}"))),
    };
    // SAFETY: inbound buffers are supplied by the runtime and valid for this call.
    let invocation_json = match unsafe { invocation_json.as_str() } {
        Ok(value) => value,
        Err(err) => {
            return json_buffer(&ToolResult::error(format!(
                "invalid invocation JSON: {err}"
            )));
        }
    };
    let input = match serde_json::from_str(input_json) {
        Ok(value) => value,
        Err(err) => return json_buffer(&ToolResult::error(format!("invalid input JSON: {err}"))),
    };
    let invocation = match serde_json::from_str(invocation_json) {
        Ok(value) => value,
        Err(err) => {
            return json_buffer(&ToolResult::error(format!(
                "invalid invocation JSON: {err}"
            )));
        }
    };
    // SAFETY: see `native_plugin_info`.
    let state = unsafe { &*state.cast::<NativePluginState>() };
    let Some(tool) = state.tools.iter().find(|tool| tool.name() == tool_name) else {
        return json_buffer(&ToolResult::error(format!(
            "native plugin does not expose tool '{tool_name}'"
        )));
    };
    let runtime = NoopToolRuntime { invocation };
    match tool.execute_with_runtime(input, &runtime) {
        Ok(result) => json_buffer(&result),
        Err(err) => json_buffer(&ToolResult::error(format!("tool error: {err}"))),
    }
}

#[doc(hidden)]
pub unsafe extern "C" fn native_plugin_drop(state: *mut c_void) {
    if state.is_null() {
        return;
    }
    // SAFETY: pointer ownership is transferred from `export_plugin!` to this
    // function exactly once by the runtime.
    unsafe {
        drop(Box::from_raw(state.cast::<NativePluginState>()));
    }
}
