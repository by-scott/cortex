use cortex_types::{ContentBlock, Message, Role};

pub fn normalize_messages_for_api(messages: &[Message]) -> Vec<Message> {
    let mut normalized = Vec::with_capacity(messages.len());
    for message in messages {
        if message.role == Role::User
            && !message.has_tool_blocks()
            && normalized
                .last()
                .is_some_and(|prev: &Message| prev.role == Role::User && !prev.has_tool_blocks())
        {
            merge_user_message_into_last(&mut normalized, message);
        } else {
            normalized.push(message.clone());
        }
    }
    for message in &mut normalized {
        if message.role == Role::User {
            dedupe_repeated_user_images(message);
            collapse_generated_attachment_prompts(message);
        }
    }
    strip_older_user_images(&mut normalized);
    normalized.retain(message_has_api_content);
    normalized
}

pub fn sanitize_history_for_text_only_turn(messages: &mut Vec<Message>) {
    for message in messages.iter_mut() {
        if message.role != Role::User {
            continue;
        }

        strip_user_images(message);
    }
    messages.retain(message_has_api_content);
}

pub fn max_tokens_for_api(
    max_tokens: usize,
    messages: &[Message],
    vision_max_output_tokens: usize,
) -> usize {
    if messages.iter().any(Message::has_images) {
        let cap = if vision_max_output_tokens == 0 {
            cortex_types::config::DEFAULT_VISION_MAX_OUTPUT_TOKENS
        } else {
            vision_max_output_tokens
        };
        max_tokens.min(cap)
    } else {
        max_tokens
    }
}

const GENERATED_IMAGE_PROMPT: &str = "The user sent an image. Describe what you see.";
const STRUCTURED_IMAGE_PROMPT: &str =
    "The previous user message is an image attachment. Describe what you see in the image.";
const GENERATED_VIDEO_PROMPT: &str = "The user sent a video. Describe the content.";
const GENERATED_AUDIO_PROMPT: &str = "The user sent an audio message. Transcribe or summarize it.";
const GENERATED_FILE_PROMPT: &str = "The user sent a file. Identify it and help with it.";

fn message_has_api_content(message: &Message) -> bool {
    if !message.attachments.is_empty() {
        return true;
    }
    message.content.iter().any(|block| match block {
        ContentBlock::Text { text } => !text.trim().is_empty(),
        ContentBlock::ToolUse { .. }
        | ContentBlock::ToolResult { .. }
        | ContentBlock::Image { .. } => true,
    })
}

fn merge_user_message_into_last(normalized: &mut [Message], next: &Message) {
    let Some(last) = normalized.last_mut() else {
        return;
    };
    let needs_separator = has_visible_text(last) && has_visible_text(next);
    if needs_separator {
        last.content.push(ContentBlock::Text {
            text: "\n".to_string(),
        });
    }
    last.content.extend(next.content.iter().cloned());
    last.attachments.extend(next.attachments.iter().cloned());
}

fn has_visible_text(message: &Message) -> bool {
    message.content.iter().any(|block| match block {
        ContentBlock::Text { text } => !text.trim().is_empty(),
        _ => false,
    })
}

fn dedupe_repeated_user_images(message: &mut Message) {
    let mut seen = std::collections::HashSet::new();
    message.content.retain(|block| match block {
        ContentBlock::Image { media_type, data } => seen.insert((media_type.clone(), data.clone())),
        _ => true,
    });
}

fn collapse_generated_attachment_prompts(message: &mut Message) {
    if message.role != Role::User {
        return;
    }
    let mut seen_prompts = std::collections::HashSet::new();
    let mut collapsed = Vec::with_capacity(message.content.len());
    let mut previous_was_newline = false;

    for block in &message.content {
        match block {
            ContentBlock::Text { text } if text == "\n" => {
                if !previous_was_newline && !collapsed.is_empty() {
                    collapsed.push(block.clone());
                    previous_was_newline = true;
                }
            }
            ContentBlock::Text { text } if is_generated_attachment_prompt(text) => {
                if seen_prompts.insert(text.clone()) {
                    if previous_was_newline
                        && collapsed.last().is_some_and(
                            |last| matches!(last, ContentBlock::Text { text } if text == "\n"),
                        )
                    {
                        let _ = collapsed.pop();
                    }
                    collapsed.push(block.clone());
                    previous_was_newline = false;
                }
            }
            _ => {
                collapsed.push(block.clone());
                previous_was_newline = false;
            }
        }
    }

    while collapsed
        .last()
        .is_some_and(|last| matches!(last, ContentBlock::Text { text } if text == "\n"))
    {
        let _ = collapsed.pop();
    }

    message.content = collapsed;
}

fn is_generated_attachment_prompt(text: &str) -> bool {
    matches!(
        text,
        GENERATED_IMAGE_PROMPT
            | STRUCTURED_IMAGE_PROMPT
            | GENERATED_VIDEO_PROMPT
            | GENERATED_AUDIO_PROMPT
            | GENERATED_FILE_PROMPT
    )
}

fn strip_older_user_images(messages: &mut [Message]) {
    let latest_image_idx = messages
        .iter()
        .enumerate()
        .rev()
        .find_map(|(idx, message)| {
            (message.role == Role::User
                && message
                    .content
                    .iter()
                    .any(|block| matches!(block, ContentBlock::Image { .. })))
            .then_some(idx)
        });

    let Some(latest_image_idx) = latest_image_idx else {
        return;
    };

    for (idx, message) in messages.iter_mut().enumerate() {
        if idx == latest_image_idx || message.role != Role::User {
            continue;
        }

        let had_images = message
            .content
            .iter()
            .any(|block| matches!(block, ContentBlock::Image { .. }));
        if !had_images {
            continue;
        }

        strip_user_images(message);
    }
}

fn strip_user_images(message: &mut Message) {
    message
        .content
        .retain(|block| !matches!(block, ContentBlock::Image { .. }));
    message
        .attachments
        .retain(|attachment| attachment.media_type != "image");
    message.content.retain(|block| {
        !matches!(block, ContentBlock::Text { text } if is_generated_attachment_prompt(text))
    });
    while message
        .content
        .last()
        .is_some_and(|last| matches!(last, ContentBlock::Text { text } if text.trim().is_empty()))
    {
        let _ = message.content.pop();
    }
}
