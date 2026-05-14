use cortex_runtime::rpc::{invalid_request, success};

#[test]
fn rpc_success_has_jsonrpc_shape() {
    let response = success(serde_json::json!(7), serde_json::json!({"ok": true}));

    assert_eq!(response.jsonrpc, "2.0");
    assert_eq!(response.id, Some(serde_json::json!(7)));
    assert_eq!(response.result, Some(serde_json::json!({"ok": true})));
    assert!(response.error.is_none());
}

#[test]
fn rpc_invalid_request_is_structured_error() {
    let response = invalid_request("bad request");
    let error = response
        .error
        .expect("invalid request should include error");

    assert_eq!(response.jsonrpc, "2.0");
    assert_eq!(error.code, -32_600);
    assert_eq!(error.message, "bad request");
}
