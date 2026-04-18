use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfidenceLevel {
    High,
    Medium,
    Low,
    Uncertain,
}

impl ConfidenceLevel {
    #[must_use]
    pub fn from_score(score: f64) -> Self {
        if score >= 0.8 {
            Self::High
        } else if score >= 0.5 {
            Self::Medium
        } else if score >= 0.2 {
            Self::Low
        } else {
            Self::Uncertain
        }
    }
}

impl fmt::Display for ConfidenceLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
            Self::Low => write!(f, "low"),
            Self::Uncertain => write!(f, "uncertain"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_score_boundaries() {
        assert_eq!(ConfidenceLevel::from_score(1.0), ConfidenceLevel::High);
        assert_eq!(ConfidenceLevel::from_score(0.8), ConfidenceLevel::High);
        assert_eq!(ConfidenceLevel::from_score(0.79), ConfidenceLevel::Medium);
        assert_eq!(ConfidenceLevel::from_score(0.5), ConfidenceLevel::Medium);
        assert_eq!(ConfidenceLevel::from_score(0.49), ConfidenceLevel::Low);
        assert_eq!(ConfidenceLevel::from_score(0.2), ConfidenceLevel::Low);
        assert_eq!(
            ConfidenceLevel::from_score(0.19),
            ConfidenceLevel::Uncertain
        );
        assert_eq!(ConfidenceLevel::from_score(0.0), ConfidenceLevel::Uncertain);
    }

    #[test]
    fn display() {
        assert_eq!(ConfidenceLevel::High.to_string(), "high");
        assert_eq!(ConfidenceLevel::Uncertain.to_string(), "uncertain");
    }
}
