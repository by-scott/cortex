use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalLevel {
    Strategic,
    Tactical,
    Immediate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GoalStatus {
    Active,
    Completed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: String,
    pub description: String,
    pub created_at: DateTime<Utc>,
    pub status: GoalStatus,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GoalStack {
    pub strategic: Option<Goal>,
    pub tactical: Option<Goal>,
    pub immediate: Option<Goal>,
}

impl Goal {
    #[must_use]
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            description: description.into(),
            created_at: Utc::now(),
            status: GoalStatus::Active,
        }
    }
}

impl GoalStack {
    #[must_use]
    pub fn format(&self) -> String {
        let mut parts = Vec::new();
        if let Some(g) = &self.strategic {
            parts.push(format!("[Strategic] {}", g.description));
        }
        if let Some(g) = &self.tactical {
            parts.push(format!("[Tactical] {}", g.description));
        }
        if let Some(g) = &self.immediate {
            parts.push(format!("[Immediate] {}", g.description));
        }
        if parts.is_empty() {
            String::from("No active goals.")
        } else {
            parts.join("\n")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_stack_format() {
        let stack = GoalStack {
            strategic: Some(Goal::new("build cortex")),
            ..GoalStack::default()
        };
        assert!(stack.format().contains("[Strategic]"));
    }

    #[test]
    fn empty_stack_format() {
        let stack = GoalStack::default();
        assert_eq!(stack.format(), "No active goals.");
    }

    #[test]
    fn json_roundtrip() {
        let stack = GoalStack::default();
        let json = serde_json::to_string(&stack).unwrap();
        let back: GoalStack = serde_json::from_str(&json).unwrap();
        assert!(back.strategic.is_none());
    }
}
