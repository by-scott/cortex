pub(super) fn structured_response_payload(
    response: &str,
) -> (
    String,
    cortex_types::TextFormat,
    Vec<cortex_types::ResponsePart>,
) {
    let structured = crate::media::output::assistant_response_from_text(response);
    (structured.text, structured.format, structured.parts)
}

pub(super) fn structured_response_payload_from_output(
    output: &crate::turn_executor::TurnOutput,
) -> (
    String,
    cortex_types::TextFormat,
    Vec<cortex_types::ResponsePart>,
) {
    (
        output.response_text.clone().unwrap_or_default(),
        cortex_types::TextFormat::Markdown,
        output.response_parts.clone(),
    )
}

/// Validate turn input: reject empty input and malformed session IDs.
pub(super) fn validate_turn_input(session_id: &str, input: &str) -> Result<(), String> {
    if input.trim().is_empty() {
        return Err("input must not be empty".into());
    }
    validate_session_id(session_id)
}

/// Session ID: max 256 chars, alphanumeric + hyphen + underscore + dot.
pub(super) fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() {
        return Err("session_id must not be empty".into());
    }
    if session_id.len() > 256 {
        return Err("session_id exceeds 256 characters".into());
    }
    if !session_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "session_id must contain only alphanumeric, hyphen, underscore, or dot characters"
                .into(),
        );
    }
    Ok(())
}

pub(super) fn extract_final_response_text(
    output: &crate::turn_executor::TurnOutput,
) -> Result<String, String> {
    output
        .response_text
        .clone()
        .filter(|text| !text.trim().is_empty())
        .or_else(|| {
            if output.response_parts.is_empty() {
                Some("Turn cancelled.".to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| "turn completed without a user-visible assistant response".to_string())
}

pub(super) fn synthesize_empty_turn_output(
    mut output: crate::turn_executor::TurnOutput,
) -> crate::turn_executor::TurnOutput {
    let text = "Turn cancelled.".to_string();
    output.response_text = Some(text.clone());
    output.response_parts = vec![cortex_types::ResponsePart::Text {
        text,
        format: cortex_types::TextFormat::Markdown,
    }];
    output
}

pub(super) fn images_to_inline(images: &[cortex_types::web::ImageData]) -> Vec<(String, String)> {
    images
        .iter()
        .map(|img| (img.media_type.clone(), img.data.clone()))
        .collect()
}

pub(super) fn rpc_param_images(params: &serde_json::Value) -> Vec<(String, String)> {
    params
        .get("images")
        .cloned()
        .and_then(|value| serde_json::from_value::<Vec<cortex_types::web::ImageData>>(value).ok())
        .map(|images| images_to_inline(&images))
        .unwrap_or_default()
}

pub(super) fn rpc_param_attachments(params: &serde_json::Value) -> Vec<cortex_types::Attachment> {
    params
        .get("attachments")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default()
}

pub(super) fn encode_json_stream_event(
    event: &cortex_turn::orchestrator::TurnStreamEvent,
) -> Option<(&'static str, String)> {
    let (event_type, payload) = match event {
        cortex_turn::orchestrator::TurnStreamEvent::Text {
            lane: cortex_turn::orchestrator::StreamLane::UserVisible,
            content,
            ..
        } => (
            "text",
            serde_json::json!({
                "event": "text",
                "data": {"content": content}
            }),
        ),
        cortex_turn::orchestrator::TurnStreamEvent::Text {
            lane: cortex_turn::orchestrator::StreamLane::Observer,
            source,
            content,
        } => (
            "observer",
            serde_json::json!({
                "event": "observer",
                "data": {"source": source, "content": content}
            }),
        ),
        cortex_turn::orchestrator::TurnStreamEvent::Boundary(_)
        | cortex_turn::orchestrator::TurnStreamEvent::ToolProgress(_) => return None,
    };
    serde_json::to_string(&payload)
        .ok()
        .map(|json| (event_type, json))
}
