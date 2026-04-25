use serde_json::json;

use crate::daemon::batch_payload;
use crate::rpc::{self, RpcRequest};

fn request(id: serde_json::Value) -> RpcRequest {
    RpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "memory/list".to_string(),
        id,
        params: json!({}),
    }
}

#[test]
fn batch_payload_rejects_empty_batches() {
    let payload = batch_payload(std::iter::empty(), |_| unreachable!("no requests"));
    let value = payload.unwrap_or_else(|| panic!("empty batch should return invalid request"));
    assert!(
        value.get("error").is_some(),
        "payload should contain error: {value:?}"
    );
    assert_eq!(value.get("id"), Some(&serde_json::Value::Null));
}

#[test]
fn batch_payload_omits_notification_successes() {
    let requests = [request(serde_json::Value::Null), request(json!(7))];
    let payload = batch_payload(requests.iter(), |req| {
        rpc::success(req.id.clone(), json!({ "ok": true }))
    });
    let value = payload.unwrap_or_else(|| panic!("mixed batch should keep non-notifications"));
    let items = value
        .as_array()
        .unwrap_or_else(|| panic!("mixed batch should serialize as array: {value:?}"));
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].get("id"), Some(&json!(7)));
}

#[test]
fn batch_payload_returns_none_for_notification_only_batches() {
    let requests = [request(serde_json::Value::Null)];
    let payload = batch_payload(requests.iter(), |req| {
        rpc::success(req.id.clone(), json!({ "ok": true }))
    });
    assert!(
        payload.is_none(),
        "notification-only batch should not emit a payload"
    );
}
