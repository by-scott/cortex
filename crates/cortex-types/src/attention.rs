use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttentionChannel {
    Foreground,
    Maintenance,
    Emergency,
}

impl fmt::Display for AttentionChannel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Foreground => write!(f, "foreground"),
            Self::Maintenance => write!(f, "maintenance"),
            Self::Emergency => write!(f, "emergency"),
        }
    }
}
