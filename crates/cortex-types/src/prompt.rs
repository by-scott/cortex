use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptLayer {
    Soul,
    Identity,
    Behavioral,
    User,
}

impl PromptLayer {
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Soul => "soul.md",
            Self::Identity => "identity.md",
            Self::Behavioral => "behavioral.md",
            Self::User => "user.md",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Soul, Self::Identity, Self::Behavioral, Self::User]
    }
}

impl fmt::Display for PromptLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Soul => write!(f, "soul"),
            Self::Identity => write!(f, "identity"),
            Self::Behavioral => write!(f, "behavioral"),
            Self::User => write!(f, "user"),
        }
    }
}
