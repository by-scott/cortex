use cortex_types::{AssistantResponse, ResponsePart, TextFormat};

#[must_use]
pub fn assistant_response_from_text(text: &str) -> AssistantResponse {
    AssistantResponse {
        text: text.to_string(),
        format: TextFormat::Markdown,
        parts: if text.trim().is_empty() {
            Vec::new()
        } else {
            vec![ResponsePart::Text {
                text: text.to_string(),
                format: TextFormat::Markdown,
            }]
        },
    }
}
