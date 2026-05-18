use serde::{Deserialize, Serialize};

/// A JSON-RPC 2.0 request.
#[derive(Debug, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub id: serde_json::Value,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// A JSON-RPC 2.0 error object with optional structured data.
#[derive(Debug, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i32,
    pub message: String,
    /// Structured error metadata (category, recoverability, hints).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

pub(super) const PARSE_ERROR: i32 = -32_700;
pub(super) const INVALID_PARAMS: i32 = -32_602;
pub(super) const METHOD_NOT_FOUND: i32 = -32_601;
pub(super) const INVALID_REQUEST: i32 = -32_600;

pub(super) const SESSION_NOT_FOUND: i32 = 1000;
pub(super) const SESSION_ALREADY_ENDED: i32 = 1001;
pub(super) const OPERATOR_ONLY: i32 = 1002;
pub(super) const TURN_EXECUTION_FAILED: i32 = 1100;
pub(super) const COMMAND_DISPATCH_FAILED: i32 = 1200;
pub(super) const MEMORY_NOT_FOUND: i32 = 1300;
pub(super) const MEMORY_OPERATION_FAILED: i32 = 1301;
pub(super) const TASK_NOT_FOUND: i32 = 1400;
pub(super) const TASK_OPERATION_FAILED: i32 = 1401;
pub(super) const GOAL_NOT_FOUND: i32 = 1500;
pub(super) const GOAL_OPERATION_FAILED: i32 = 1501;

#[must_use]
pub fn success(id: serde_json::Value, result: serde_json::Value) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: Some(result),
        error: None,
    }
}

#[must_use]
pub fn error(id: serde_json::Value, code: i32, message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: None,
        }),
    }
}

#[must_use]
pub fn invalid_request(message: &str) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(serde_json::Value::Null),
        result: None,
        error: Some(RpcError {
            code: INVALID_REQUEST,
            message: message.into(),
            data: None,
        }),
    }
}

/// Create an application-level error with structured metadata.
#[must_use]
pub(super) fn app_error(
    id: serde_json::Value,
    code: i32,
    message: &str,
    category: &'static str,
    recoverable: bool,
    hint: &'static str,
) -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: Some(id),
        result: None,
        error: Some(RpcError {
            code,
            message: message.into(),
            data: Some(serde_json::json!({
                "category": category,
                "recoverable": recoverable,
                "hint": hint,
            })),
        }),
    }
}

#[must_use]
pub fn parse_error() -> RpcResponse {
    RpcResponse {
        jsonrpc: "2.0".into(),
        id: None,
        result: None,
        error: Some(RpcError {
            code: PARSE_ERROR,
            message: "Parse error".into(),
            data: None,
        }),
    }
}

/// Parse a JSON line into an `RpcRequest`, returning a parse error response on failure.
///
/// # Errors
///
/// Returns an `RpcResponse` with error code -32700 if the JSON is malformed.
pub fn parse_request(line: &str) -> Result<RpcRequest, Box<RpcResponse>> {
    serde_json::from_str::<RpcRequest>(line).map_err(|e| {
        Box::new(RpcResponse {
            jsonrpc: "2.0".into(),
            id: None,
            result: None,
            error: Some(RpcError {
                code: PARSE_ERROR,
                message: format!("Parse error: {e}"),
                data: None,
            }),
        })
    })
}
