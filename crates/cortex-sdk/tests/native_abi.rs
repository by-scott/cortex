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
        vec![Box::new(EchoTool)]
    }
}

struct EchoTool;

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

cortex_sdk::export_plugin!(AbiTestPlugin);

#[test]
fn export_macro_exposes_stable_native_abi_table() {
    let host = CortexHostApi {
        abi_version: NATIVE_ABI_VERSION,
    };
    let mut api = CortexPluginApi::empty();
    let status = unsafe { cortex_plugin_init(&raw const host, &raw mut api) };
    assert_eq!(status, 0);
    assert_eq!(api.abi_version, NATIVE_ABI_VERSION);
    assert!(!api.plugin.is_null());

    let Some(plugin_info) = api.plugin_info else {
        panic!("plugin_info callback should be present");
    };
    let Some(tool_count) = api.tool_count else {
        panic!("tool_count callback should be present");
    };
    let Some(tool_descriptor) = api.tool_descriptor else {
        panic!("tool_descriptor callback should be present");
    };
    let Some(tool_execute) = api.tool_execute else {
        panic!("tool_execute callback should be present");
    };
    let Some(buffer_free) = api.buffer_free else {
        panic!("buffer_free callback should be present");
    };

    let info = take_buffer(unsafe { plugin_info(api.plugin) }, buffer_free);
    assert_eq!(info["name"], "abi-test");

    let count = unsafe { tool_count(api.plugin) };
    assert_eq!(count, 1);

    let descriptor = take_buffer(unsafe { tool_descriptor(api.plugin, 0) }, buffer_free);
    assert_eq!(descriptor["name"], "echo");

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

    let Some(plugin_drop) = api.plugin_drop else {
        panic!("plugin_drop callback should be present");
    };
    unsafe { plugin_drop(api.plugin) };
}

const fn borrowed_buffer(value: &str) -> CortexBuffer {
    CortexBuffer {
        ptr: value.as_ptr().cast_mut(),
        len: value.len(),
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
