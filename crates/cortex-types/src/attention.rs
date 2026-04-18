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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display() {
        assert_eq!(AttentionChannel::Emergency.to_string(), "emergency");
    }

    #[test]
    fn json_roundtrip() {
        let ch = AttentionChannel::Maintenance;
        let json = serde_json::to_string(&ch).unwrap();
        let back: AttentionChannel = serde_json::from_str(&json).unwrap();
        assert_eq!(ch, back);
    }
}
