use cortex_sdk::{Attachment, ToolResult};

#[test]
fn tool_result_can_carry_text_and_media_without_runtime_types() {
    let image = Attachment {
        media_type: "image".to_string(),
        mime_type: "image/png".to_string(),
        url: "file:///tmp/image.png".to_string(),
        caption: Some("generated preview".to_string()),
        size: Some(1024),
    };
    let result = ToolResult::success("created").with_media(image);

    assert_eq!(result.output, "created");
    assert_eq!(result.media.len(), 1);
    assert!(!result.is_error);
}
