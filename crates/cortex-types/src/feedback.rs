use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackTarget {
    Style,
    Fact,
    ToolChoice,
    Memory,
    Evidence,
    PermissionJudgment,
    Prompt,
    Skill,
    Policy,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FeedbackKind {
    Correction,
    Preference,
    Approval,
    Rejection,
    TaskSuccess,
    TaskFailure,
    SafetyBoundary,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackAttribution {
    pub feedback_id: String,
    pub actor: String,
    pub turn_id: String,
    pub kind: FeedbackKind,
    pub target: FeedbackTarget,
    pub target_ref: String,
    pub rationale: String,
    pub replay_query: String,
    pub expected_future_behavior: String,
    pub durable: bool,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackReplayCheck {
    pub feedback_id: String,
    pub similar_task: String,
    pub applied: bool,
    pub evidence: String,
    pub checked_at: DateTime<Utc>,
}

impl FeedbackAttribution {
    #[must_use]
    pub fn from_user_text(
        actor: impl Into<String>,
        turn_id: impl Into<String>,
        text: impl Into<String>,
        expected_future_behavior: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let expected_future_behavior = expected_future_behavior.into();
        let target = classify_feedback_target(&text);
        let kind = classify_feedback_kind(&text);
        Self {
            feedback_id: uuid::Uuid::now_v7().to_string(),
            actor: actor.into(),
            turn_id: turn_id.into(),
            kind,
            target,
            target_ref: target.label().to_string(),
            rationale: text.clone(),
            replay_query: replay_query_for(target, &text),
            expected_future_behavior,
            durable: matches!(
                kind,
                FeedbackKind::Correction | FeedbackKind::Preference | FeedbackKind::SafetyBoundary
            ),
            recorded_at: Utc::now(),
        }
    }

    #[must_use]
    pub fn replay_check(
        &self,
        similar_task: impl Into<String>,
        observed_behavior: impl Into<String>,
    ) -> FeedbackReplayCheck {
        let similar_task = similar_task.into();
        let observed_behavior = observed_behavior.into();
        let task_related = has_meaningful_overlap(&self.replay_query, &similar_task);
        let applied = task_related
            && has_meaningful_overlap(&self.expected_future_behavior, &observed_behavior);
        FeedbackReplayCheck {
            feedback_id: self.feedback_id.clone(),
            similar_task,
            applied,
            evidence: observed_behavior,
            checked_at: Utc::now(),
        }
    }
}

impl FeedbackTarget {
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Style => "style",
            Self::Fact => "fact",
            Self::ToolChoice => "tool_choice",
            Self::Memory => "memory",
            Self::Evidence => "evidence",
            Self::PermissionJudgment => "permission_judgment",
            Self::Prompt => "prompt",
            Self::Skill => "skill",
            Self::Policy => "policy",
            Self::Unknown => "unknown",
        }
    }
}

#[must_use]
pub fn classify_feedback_target(text: &str) -> FeedbackTarget {
    let lower = text.to_ascii_lowercase();
    if contains_any(
        &lower,
        &["permission", "approve", "deny", "confirmation", "risk"],
    ) {
        return FeedbackTarget::PermissionJudgment;
    }
    if contains_any(
        &lower,
        &["tool", "command", "bash", "search", "read before"],
    ) {
        return FeedbackTarget::ToolChoice;
    }
    if contains_any(&lower, &["evidence", "citation", "source", "unsupported"]) {
        return FeedbackTarget::Evidence;
    }
    if contains_any(&lower, &["memory", "remember", "forget", "preference"]) {
        return FeedbackTarget::Memory;
    }
    if contains_any(&lower, &["policy", "allowed", "forbidden", "rule"]) {
        return FeedbackTarget::Policy;
    }
    if contains_any(&lower, &["prompt", "system instruction", "behavioral"]) {
        return FeedbackTarget::Prompt;
    }
    if contains_any(&lower, &["skill", "repertoire"]) {
        return FeedbackTarget::Skill;
    }
    if contains_any(&lower, &["fact", "incorrect", "wrong", "inaccurate"]) {
        return FeedbackTarget::Fact;
    }
    if contains_any(
        &lower,
        &["style", "tone", "wording", "concise", "verbose", "format"],
    ) {
        return FeedbackTarget::Style;
    }
    FeedbackTarget::Unknown
}

#[must_use]
pub fn classify_feedback_kind(text: &str) -> FeedbackKind {
    let lower = text.to_ascii_lowercase();
    if contains_any(&lower, &["must not", "never", "forbidden", "unsafe"]) {
        return FeedbackKind::SafetyBoundary;
    }
    if contains_any(
        &lower,
        &["wrong", "incorrect", "should", "instead", "correct"],
    ) {
        return FeedbackKind::Correction;
    }
    if contains_any(&lower, &["prefer", "preference", "like", "style"]) {
        return FeedbackKind::Preference;
    }
    if contains_any(&lower, &["approved", "good", "worked"]) {
        return FeedbackKind::Approval;
    }
    if contains_any(&lower, &["failed", "bad", "did not work"]) {
        return FeedbackKind::TaskFailure;
    }
    FeedbackKind::Unknown
}

fn replay_query_for(target: FeedbackTarget, text: &str) -> String {
    format!("{} feedback: {}", target.label(), text)
}

fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}

fn has_meaningful_overlap(expected: &str, observed: &str) -> bool {
    let observed = observed.to_ascii_lowercase();
    meaningful_terms(expected)
        .iter()
        .any(|term| observed.contains(term))
}

fn meaningful_terms(text: &str) -> Vec<String> {
    text.to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|term| term.len() >= 4)
        .map(str::to_string)
        .collect()
}
