use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum PressureLevel {
    Normal,
    Alert,
    Compress,
    Urgent,
    Degrade,
}

impl PressureLevel {
    /// Map occupancy ratio to pressure level using 4 thresholds.
    /// Default thresholds: `[0.60, 0.75, 0.85, 0.95]`
    #[must_use]
    pub const fn from_occupancy(occupancy: f64, thresholds: &[f64; 4]) -> Self {
        if occupancy >= thresholds[3] {
            Self::Degrade
        } else if occupancy >= thresholds[2] {
            Self::Urgent
        } else if occupancy >= thresholds[1] {
            Self::Compress
        } else if occupancy >= thresholds[0] {
            Self::Alert
        } else {
            Self::Normal
        }
    }

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Normal => "normal",
            Self::Alert => "alert",
            Self::Compress => "compress",
            Self::Urgent => "urgent",
            Self::Degrade => "degrade",
        }
    }
}

/// Estimate token count from text using chars/4 heuristic.
#[must_use]
pub const fn estimate_tokens(text: &str) -> usize {
    text.len().div_ceil(4)
}

/// Compute context occupancy ratio.
#[must_use]
pub fn compute_occupancy(used_tokens: usize, max_tokens: usize) -> f64 {
    if max_tokens == 0 {
        return 0.0;
    }
    let used = f64::from(u32::try_from(used_tokens).unwrap_or(u32::MAX));
    let max = f64::from(u32::try_from(max_tokens).unwrap_or(u32::MAX));
    used / max
}
