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

#[cfg(test)]
mod tests {
    use super::assistant_response_from_text;

    #[test]
    fn text_response_keeps_text_literal() {
        let response = assistant_response_from_text("hi [literal media path]");

        assert_eq!(response.parts.len(), 1);
        assert_eq!(response.plain_text(), "hi [literal media path]");
    }
}
