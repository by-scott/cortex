use crate::rpc;

pub(super) fn batch_payload<'a, I, F>(requests: I, handler: F) -> Option<serde_json::Value>
where
    I: IntoIterator<Item = &'a rpc::RpcRequest>,
    F: FnMut(&'a rpc::RpcRequest) -> rpc::RpcResponse,
{
    let mut requests = requests.into_iter().peekable();
    if requests.peek().is_none() {
        return Some(
            serde_json::to_value(rpc::invalid_request(
                "Invalid Request: batch must not be empty",
            ))
            .unwrap_or_default(),
        );
    }
    batch_responses(requests.map(handler))
}

fn batch_responses<I>(responses: I) -> Option<serde_json::Value>
where
    I: IntoIterator<Item = rpc::RpcResponse>,
{
    let mut collected: Vec<rpc::RpcResponse> = responses
        .into_iter()
        .filter(|response| {
            !(response.id.as_ref().is_some_and(serde_json::Value::is_null)
                && response.error.is_none())
        })
        .collect();

    if collected.is_empty() {
        return None;
    }
    if collected.len() == 1
        && collected[0]
            .id
            .as_ref()
            .is_some_and(serde_json::Value::is_null)
        && collected[0].error.is_some()
    {
        return Some(serde_json::to_value(collected.remove(0)).unwrap_or_default());
    }
    Some(serde_json::to_value(collected).unwrap_or_default())
}
