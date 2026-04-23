use cortex_types::{ContentBlock, Message};

const DECISION_KEYWORDS: &[&str] = &[
    "决定",
    "选择",
    "确认",
    "决策",
    "采用",
    "方案",
    "结论",
    "decide",
    "choose",
    "confirm",
    "approve",
    "reject",
    "agree",
    "disagree",
    "adopt",
    "proposal",
    "conclusion",
    "important",
    "critical",
    "must",
    "breaking",
];

const TOOL_WEIGHT: f64 = 0.4;
const DECISION_WEIGHT: f64 = 0.3;
const LENGTH_WEIGHT: f64 = 0.2;
const RECENCY_WEIGHT: f64 = 0.1;
const LENGTH_DIVISOR: f64 = 200.0;
const DECISION_DIVISOR: f64 = 3.0;

/// Compute importance score for a message (0.0 to 1.0).
#[must_use]
pub fn importance_score(message: &Message, recency: f64) -> f64 {
    let tool = tool_signal(message);
    let decision = decision_signal(message);
    let length = length_signal(message);
    let score = tool.mul_add(TOOL_WEIGHT, decision * DECISION_WEIGHT)
        + length.mul_add(LENGTH_WEIGHT, recency * RECENCY_WEIGHT);
    score.clamp(0.0, 1.0)
}

/// 1.0 if message contains tool blocks, else 0.0.
#[must_use]
pub fn tool_signal(message: &Message) -> f64 {
    if message.content.iter().any(|b| {
        matches!(
            b,
            ContentBlock::ToolUse { .. } | ContentBlock::ToolResult { .. }
        )
    }) {
        1.0
    } else {
        0.0
    }
}

/// Decision keyword density (count / 3, clamped to 1.0).
#[must_use]
pub fn decision_signal(message: &Message) -> f64 {
    let text = message.text_content().to_lowercase();
    let count = DECISION_KEYWORDS
        .iter()
        .filter(|kw| text.contains(**kw))
        .count();
    (f64::from(u32::try_from(count).unwrap_or(u32::MAX)) / DECISION_DIVISOR).min(1.0)
}

/// Length signal (chars / 200, clamped to 1.0).
#[must_use]
pub fn length_signal(message: &Message) -> f64 {
    let len = message.text_content().len();
    (f64::from(u32::try_from(len).unwrap_or(u32::MAX)) / LENGTH_DIVISOR).min(1.0)
}

/// Score all messages. Recency ranges from 0.0 (oldest) to 1.0 (newest).
#[must_use]
pub fn score_messages(messages: &[Message]) -> Vec<f64> {
    if messages.is_empty() {
        return Vec::new();
    }
    let last_idx = messages.len().saturating_sub(1);
    messages
        .iter()
        .enumerate()
        .map(|(i, msg)| {
            let recency = if last_idx == 0 {
                1.0
            } else {
                f64::from(u32::try_from(i).unwrap_or(u32::MAX))
                    / f64::from(u32::try_from(last_idx).unwrap_or(u32::MAX))
            };
            importance_score(msg, recency)
        })
        .collect()
}
