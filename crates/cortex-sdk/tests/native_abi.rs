use cortex_sdk::prelude::*;

#[derive(Default)]
struct AbiTestPlugin;

impl MultiToolPlugin for AbiTestPlugin {
    fn plugin_info(&self) -> PluginInfo {
        PluginInfo {
            name: "abi-test".to_string(),
            version: "0.1.0".to_string(),
            description: "ABI test plugin".to_string(),
        }
    }

    fn create_tools(&self) -> Vec<Box<dyn Tool>> {
        vec![Box::new(EchoTool), Box::new(FailTool)]
    }
}

struct EchoTool;
struct FailTool;

impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn description(&self) -> &'static str {
        "Echo text for native ABI tests."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "text": { "type": "string" }
            },
            "required": ["text"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let text = input
            .get("text")
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| ToolError::InvalidInput("missing text".to_string()))?;
        Ok(ToolResult::success(text))
    }
}

impl Tool for FailTool {
    fn name(&self) -> &'static str {
        "fail"
    }

    fn description(&self) -> &'static str {
        "Return an execution error for native ABI tests."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object"
        })
    }

    fn execute(&self, _input: serde_json::Value) -> Result<ToolResult, ToolError> {
        Err(ToolError::ExecutionFailed("forced failure".to_string()))
    }
}

cortex_sdk::export_plugin!(AbiTestPlugin);

fn init_api() -> CortexPluginApi {
    let host = CortexHostApi {
        abi_version: NATIVE_ABI_VERSION,
    };
    let mut api = CortexPluginApi::empty();
    let status = unsafe { cortex_plugin_init(&raw const host, &raw mut api) };
    assert_eq!(status, 0);
    api
}

fn require_callback<T>(callback: Option<T>, name: &str) -> T {
    callback.map_or_else(
        || panic!("{name} callback should be present"),
        std::convert::identity,
    )
}

#[test]
fn export_macro_exposes_stable_native_abi_table() {
    let api = init_api();
    assert_eq!(api.abi_version, NATIVE_ABI_VERSION);
    assert!(!api.plugin.is_null());

    let plugin_info = require_callback(api.plugin_info, "plugin_info");
    let tool_count = require_callback(api.tool_count, "tool_count");
    let tool_descriptor = require_callback(api.tool_descriptor, "tool_descriptor");
    let tool_execute = require_callback(api.tool_execute, "tool_execute");
    let buffer_free = require_callback(api.buffer_free, "buffer_free");

    let info = take_buffer(unsafe { plugin_info(api.plugin) }, buffer_free);
    assert_eq!(info["name"], "abi-test");

    let count = unsafe { tool_count(api.plugin) };
    assert_eq!(count, 2);

    let descriptor = take_buffer(unsafe { tool_descriptor(api.plugin, 0) }, buffer_free);
    assert_eq!(descriptor["name"], "echo");
    let fail_descriptor = take_buffer(unsafe { tool_descriptor(api.plugin, 1) }, buffer_free);
    assert_eq!(fail_descriptor["name"], "fail");

    let input = r#"{"text":"hello"}"#;
    let invocation = r#"{"tool_name":"echo","session_id":null,"actor":null,"source":null,"execution_scope":"foreground"}"#;
    let result = take_buffer(
        unsafe {
            tool_execute(
                api.plugin,
                borrowed_buffer("echo"),
                borrowed_buffer(input),
                borrowed_buffer(invocation),
            )
        },
        buffer_free,
    );
    assert_eq!(result["output"], "hello");
    assert_eq!(result["is_error"], false);

    let plugin_drop = require_callback(api.plugin_drop, "plugin_drop");
    unsafe { plugin_drop(api.plugin) };
}

#[test]
fn export_macro_rejects_null_and_mismatched_host_abi() {
    let mut api = CortexPluginApi::empty();

    let null_host_status = unsafe { cortex_plugin_init(std::ptr::null(), &raw mut api) };
    assert_eq!(null_host_status, -1);

    let host = CortexHostApi {
        abi_version: NATIVE_ABI_VERSION + 1,
    };
    let mismatch_status = unsafe { cortex_plugin_init(&raw const host, &raw mut api) };
    assert_eq!(mismatch_status, -2);
}

#[test]
fn export_macro_surfaces_tool_lookup_and_input_errors_as_results() {
    let api = init_api();

    let tool_execute = require_callback(api.tool_execute, "tool_execute");
    let tool_descriptor = require_callback(api.tool_descriptor, "tool_descriptor");
    let plugin_info = require_callback(api.plugin_info, "plugin_info");
    let tool_count = require_callback(api.tool_count, "tool_count");
    let buffer_free = require_callback(api.buffer_free, "buffer_free");

    assert_null_descriptor_and_info(tool_descriptor, plugin_info, tool_count, api.plugin);
    assert_unknown_tool_error(tool_execute, buffer_free, api.plugin);
    assert_invalid_input_error(tool_execute, buffer_free, api.plugin);
    assert_invalid_invocation_error(tool_execute, buffer_free, api.plugin);
    assert_execution_failure_error(tool_execute, buffer_free, api.plugin);
    assert_null_state_error(tool_execute, buffer_free);
    assert_invalid_utf8_error(tool_execute, buffer_free, api.plugin);

    let plugin_drop = require_callback(api.plugin_drop, "plugin_drop");
    unsafe { plugin_drop(api.plugin) };
}

fn assert_null_descriptor_and_info(
    tool_descriptor: unsafe extern "C" fn(*mut std::ffi::c_void, usize) -> CortexBuffer,
    plugin_info: unsafe extern "C" fn(*mut std::ffi::c_void) -> CortexBuffer,
    tool_count: unsafe extern "C" fn(*mut std::ffi::c_void) -> usize,
    plugin: *mut std::ffi::c_void,
) {
    let descriptor = unsafe { tool_descriptor(plugin, 99) };
    assert_eq!(descriptor.len, 0);
    assert!(descriptor.ptr.is_null());
    let null_info = unsafe { plugin_info(std::ptr::null_mut()) };
    assert_eq!(null_info.len, 0);
    assert!(null_info.ptr.is_null());
    assert_eq!(unsafe { tool_count(std::ptr::null_mut()) }, 0);
}

fn assert_unknown_tool_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
    plugin: *mut std::ffi::c_void,
) {
    let unknown_tool = take_buffer(
        unsafe {
            tool_execute(
                plugin,
                borrowed_buffer("missing"),
                borrowed_buffer("{}"),
                borrowed_buffer(
                    r#"{"tool_name":"missing","session_id":null,"actor":null,"source":null,"execution_scope":"foreground"}"#,
                ),
            )
        },
        buffer_free,
    );
    assert_eq!(unknown_tool["is_error"], true);
    assert!(
        unknown_tool["output"]
            .as_str()
            .is_some_and(|value| value.contains("does not expose tool"))
    );
}

fn assert_invalid_input_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
    plugin: *mut std::ffi::c_void,
) {
    let invalid_input = take_buffer(
        unsafe {
            tool_execute(
                plugin,
                borrowed_buffer("echo"),
                borrowed_buffer("{}"),
                borrowed_buffer(
                    r#"{"tool_name":"echo","session_id":null,"actor":null,"source":null,"execution_scope":"foreground"}"#,
                ),
            )
        },
        buffer_free,
    );
    assert_eq!(invalid_input["is_error"], true);
    assert!(
        invalid_input["output"]
            .as_str()
            .is_some_and(|value| value.contains("missing text"))
    );
}

fn assert_invalid_invocation_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
    plugin: *mut std::ffi::c_void,
) {
    let invalid_invocation = take_buffer(
        unsafe {
            tool_execute(
                plugin,
                borrowed_buffer("echo"),
                borrowed_buffer(r#"{"text":"hello"}"#),
                borrowed_buffer("not-json"),
            )
        },
        buffer_free,
    );
    assert_eq!(invalid_invocation["is_error"], true);
    assert!(
        invalid_invocation["output"]
            .as_str()
            .is_some_and(|value| value.contains("invalid invocation JSON"))
    );
}

fn assert_null_state_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
) {
    let null_state = take_buffer(
        unsafe {
            tool_execute(
                std::ptr::null_mut(),
                borrowed_buffer("echo"),
                borrowed_buffer("{}"),
                borrowed_buffer("{}"),
            )
        },
        buffer_free,
    );
    assert_eq!(null_state["is_error"], true);
    assert!(
        null_state["output"]
            .as_str()
            .is_some_and(|value| value.contains("native plugin state is null"))
    );
}

fn assert_execution_failure_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
    plugin: *mut std::ffi::c_void,
) {
    let failed = take_buffer(
        unsafe {
            tool_execute(
                plugin,
                borrowed_buffer("fail"),
                borrowed_buffer("{}"),
                borrowed_buffer(
                    r#"{"tool_name":"fail","session_id":null,"actor":null,"source":null,"execution_scope":"foreground"}"#,
                ),
            )
        },
        buffer_free,
    );
    assert_eq!(failed["is_error"], true);
    assert!(
        failed["output"]
            .as_str()
            .is_some_and(|value| value.contains("forced failure"))
    );
}

fn assert_invalid_utf8_error(
    tool_execute: unsafe extern "C" fn(
        *mut std::ffi::c_void,
        CortexBuffer,
        CortexBuffer,
        CortexBuffer,
    ) -> CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
    plugin: *mut std::ffi::c_void,
) {
    let invalid_utf8 = take_buffer(
        unsafe {
            tool_execute(
                plugin,
                invalid_utf8_buffer(),
                borrowed_buffer("{}"),
                borrowed_buffer("{}"),
            )
        },
        buffer_free,
    );
    assert_eq!(invalid_utf8["is_error"], true);
    assert!(
        invalid_utf8["output"]
            .as_str()
            .is_some_and(|value| value.contains("invalid tool name"))
    );
}

const fn borrowed_buffer(value: &str) -> CortexBuffer {
    CortexBuffer {
        ptr: value.as_ptr().cast_mut(),
        len: value.len(),
        cap: 0,
    }
}

fn invalid_utf8_buffer() -> CortexBuffer {
    static BYTES: &[u8] = &[0xff, 0xfe];
    CortexBuffer {
        ptr: BYTES.as_ptr().cast_mut(),
        len: BYTES.len(),
        cap: 0,
    }
}

fn take_buffer(
    buffer: CortexBuffer,
    buffer_free: unsafe extern "C" fn(CortexBuffer),
) -> serde_json::Value {
    let text = match unsafe { buffer.as_str() } {
        Ok(value) => value.to_string(),
        Err(err) => panic!("buffer should be UTF-8: {err}"),
    };
    unsafe { buffer_free(buffer) };
    match serde_json::from_str(&text) {
        Ok(value) => value,
        Err(err) => panic!("buffer should contain JSON: {err}"),
    }
}
