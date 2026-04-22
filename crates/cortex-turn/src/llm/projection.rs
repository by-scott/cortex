use std::collections::HashSet;

use cortex_types::{ContentBlock, Message, Role};

use super::normalize::normalize_messages_for_api;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProjectionDiagnostics {
    pub inserted_user_anchor: bool,
    pub synthetic_tool_results: usize,
    pub orphan_tool_results: usize,
    pub duplicate_tool_uses: usize,
    pub empty_messages_removed: usize,
}

#[derive(Debug, Clone)]
pub struct LlmProjection {
    pub messages: Vec<Message>,
    pub diagnostics: ProjectionDiagnostics,
}

#[must_use]
pub fn project_messages_for_llm(messages: &[Message]) -> LlmProjection {
    let normalized = normalize_messages_for_api(messages);
    let mut diagnostics = ProjectionDiagnostics::default();
    let mut projected = repair_tool_pairs(normalized, &mut diagnostics);
    projected.retain(|message| {
        let keep = message_has_content(message);
        if !keep {
            diagnostics.empty_messages_removed += 1;
        }
        keep
    });
    ensure_user_anchor(&mut projected, &mut diagnostics);
    LlmProjection {
        messages: projected,
        diagnostics,
    }
}

fn repair_tool_pairs(
    messages: Vec<Message>,
    diagnostics: &mut ProjectionDiagnostics,
) -> Vec<Message> {
    let mut repaired = Vec::with_capacity(messages.len());
    let mut seen_tool_uses = HashSet::new();
    let mut pending_tool_uses = Vec::new();

    for message in messages {
        if message.role == Role::User {
            let mut content = Vec::with_capacity(message.content.len());
            for block in message.content {
                match block {
                    ContentBlock::ToolResult {
                        tool_use_id,
                        content: result_content,
                        is_error,
                    } => {
                        if let Some(pos) = pending_tool_uses
                            .iter()
                            .position(|id: &String| id == &tool_use_id)
                        {
                            pending_tool_uses.remove(pos);
                            content.push(ContentBlock::ToolResult {
                                tool_use_id,
                                content: result_content,
                                is_error,
                            });
                        } else {
                            diagnostics.orphan_tool_results += 1;
                            content.push(ContentBlock::Text {
                                text: format!(
                                    "[orphan_tool_result:{tool_use_id}] {result_content}"
                                ),
                            });
                        }
                    }
                    other => content.push(other),
                }
            }
            repaired.push(Message { content, ..message });
            continue;
        }

        if message.role == Role::Assistant && !pending_tool_uses.is_empty() {
            repaired.push(synthetic_tool_result_message(
                std::mem::take(&mut pending_tool_uses),
                diagnostics,
            ));
        }

        let mut content = Vec::with_capacity(message.content.len());
        for block in message.content {
            match block {
                ContentBlock::ToolUse { id, name, input } => {
                    if seen_tool_uses.insert(id.clone()) {
                        pending_tool_uses.push(id.clone());
                        content.push(ContentBlock::ToolUse { id, name, input });
                    } else {
                        diagnostics.duplicate_tool_uses += 1;
                        content.push(ContentBlock::Text {
                            text: format!("[duplicate_tool_use:{id}]"),
                        });
                    }
                }
                other => content.push(other),
            }
        }
        repaired.push(Message { content, ..message });
    }

    if !pending_tool_uses.is_empty() {
        repaired.push(synthetic_tool_result_message(
            pending_tool_uses,
            diagnostics,
        ));
    }

    repaired
}

fn synthetic_tool_result_message(
    tool_use_ids: Vec<String>,
    diagnostics: &mut ProjectionDiagnostics,
) -> Message {
    diagnostics.synthetic_tool_results += tool_use_ids.len();
    Message {
        role: Role::User,
        content: tool_use_ids
            .into_iter()
            .map(|tool_use_id| ContentBlock::ToolResult {
                tool_use_id,
                content: "(tool result unavailable)".to_string(),
                is_error: true,
            })
            .collect(),
        attachments: Vec::new(),
    }
}

fn ensure_user_anchor(messages: &mut Vec<Message>, diagnostics: &mut ProjectionDiagnostics) {
    if messages
        .first()
        .is_some_and(|message| message.role == Role::User)
    {
        return;
    }
    diagnostics.inserted_user_anchor = true;
    messages.insert(0, Message::user("(Earlier conversation omitted.)"));
}

fn message_has_content(message: &Message) -> bool {
    !message.attachments.is_empty()
        || message.content.iter().any(|block| match block {
            ContentBlock::Text { text } => !text.trim().is_empty(),
            ContentBlock::ToolUse { .. }
            | ContentBlock::ToolResult { .. }
            | ContentBlock::Image { .. } => true,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inserts_user_anchor_before_leading_assistant() {
        let projection = project_messages_for_llm(&[Message::assistant("hello")]);

        assert!(projection.diagnostics.inserted_user_anchor);
        assert_eq!(projection.messages[0].role, Role::User);
        assert_eq!(projection.messages[1].role, Role::Assistant);
    }

    #[test]
    fn repairs_missing_tool_result() {
        let projection = project_messages_for_llm(&[Message {
            role: Role::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "t1".into(),
                name: "bash".into(),
                input: serde_json::json!({}),
            }],
            attachments: Vec::new(),
        }]);

        assert_eq!(projection.diagnostics.synthetic_tool_results, 1);
        assert!(projection.messages.iter().any(|message| {
            message
                .content
                .iter()
                .any(|block| matches!(block, ContentBlock::ToolResult { tool_use_id, .. } if tool_use_id == "t1"))
        }));
    }

    #[test]
    fn converts_orphan_tool_result_to_text() {
        let projection = project_messages_for_llm(&[Message {
            role: Role::User,
            content: vec![ContentBlock::ToolResult {
                tool_use_id: "missing".into(),
                content: "late".into(),
                is_error: false,
            }],
            attachments: Vec::new(),
        }]);

        assert_eq!(projection.diagnostics.orphan_tool_results, 1);
        assert!(
            projection.messages[0]
                .text_content()
                .contains("orphan_tool_result")
        );
    }
}
