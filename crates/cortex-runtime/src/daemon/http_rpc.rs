use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Json};

use crate::rpc;

use super::{HttpState, rpc_batch};

pub(super) async fn handle_http_rpc(
    State(state): State<HttpState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Require JSON content type for RPC requests
    let ct = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ct.is_empty() && !ct.contains("json") {
        let resp = rpc::RpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::Value::Null),
            result: None,
            error: Some(rpc::RpcError {
                code: -32700,
                message: format!("Unsupported Content-Type: {ct} (expected application/json)"),
                data: None,
            }),
        };
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(serde_json::to_value(resp).unwrap_or_default()),
        )
            .into_response();
    }
    // Try batch (JSON array) first
    if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&body) {
        let Some(payload) = rpc_batch::batch_payload(batch.iter(), |request| {
            state.handler.handle_for_client(request, "http")
        }) else {
            return StatusCode::NO_CONTENT.into_response();
        };
        return (StatusCode::OK, Json(payload)).into_response();
    }
    // Single request
    let response = match rpc::parse_request(&body) {
        Ok(req) => {
            let is_notification = req.id.is_null();
            let resp = state.handler.handle_for_client(&req, "http");
            if is_notification {
                return StatusCode::NO_CONTENT.into_response();
            }
            resp
        }
        Err(err_resp) => *err_resp,
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap_or_default()),
    )
        .into_response()
}
