use serde::{Deserialize, Serialize};

use crate::{OwnedScope, SessionId, TurnId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageRecord {
    pub scope: OwnedScope,
    pub turn_id: TurnId,
    pub session_id: SessionId,
    pub model: String,
    pub usage: TokenUsage,
}

impl TokenUsage {
    #[must_use]
    pub const fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
        }
    }

    #[must_use]
    pub const fn total(self) -> u64 {
        self.input_tokens.saturating_add(self.output_tokens)
    }

    #[must_use]
    pub const fn saturating_add(self, other: Self) -> Self {
        Self {
            input_tokens: self.input_tokens.saturating_add(other.input_tokens),
            output_tokens: self.output_tokens.saturating_add(other.output_tokens),
        }
    }
}

impl UsageRecord {
    #[must_use]
    pub fn new(
        scope: OwnedScope,
        turn_id: TurnId,
        session_id: SessionId,
        model: impl Into<String>,
        usage: TokenUsage,
    ) -> Self {
        Self {
            scope,
            turn_id,
            session_id,
            model: model.into(),
            usage,
        }
    }
}
