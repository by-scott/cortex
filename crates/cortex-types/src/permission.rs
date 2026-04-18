use serde::{Deserialize, Serialize};

const WEIGHTS: [f32; 4] = [0.3, 0.2, 0.3, 0.2];
const DEPTH_DECAY_FACTOR: f32 = 1.3;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RiskScore {
    pub tool_risk: f32,
    pub file_sensitivity: f32,
    pub blast_radius: f32,
    pub irreversibility: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum RiskLevel {
    Allow,
    Review,
    RequireConfirmation,
    Block,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PermissionDecision {
    Pending,
    Approved,
    Denied,
    TimedOut,
}

impl RiskScore {
    #[must_use]
    pub const fn new(
        tool_risk: f32,
        file_sensitivity: f32,
        blast_radius: f32,
        irreversibility: f32,
    ) -> Self {
        Self {
            tool_risk,
            file_sensitivity,
            blast_radius,
            irreversibility,
        }
    }

    #[must_use]
    pub fn composite_score(self) -> f32 {
        let axes = [
            self.tool_risk,
            self.file_sensitivity,
            self.blast_radius,
            self.irreversibility,
        ];
        let max = axes.iter().copied().fold(0.0_f32, f32::max);
        let weighted_avg: f32 = axes
            .iter()
            .zip(WEIGHTS.iter())
            .map(|(a, w)| a * w)
            .sum::<f32>()
            / WEIGHTS.iter().sum::<f32>();
        (max * weighted_avg).clamp(0.0, 1.0)
    }

    #[must_use]
    pub fn with_depth_decay(mut self, depth: usize) -> Self {
        let factor = DEPTH_DECAY_FACTOR.powi(i32::try_from(depth).unwrap_or(i32::MAX));
        self.tool_risk = (self.tool_risk * factor).min(1.0);
        self.blast_radius = (self.blast_radius * factor).min(1.0);
        self.irreversibility = (self.irreversibility * factor).min(1.0);
        self
    }
}

/// A request to confirm a high-risk tool invocation before execution.
#[derive(Debug, Clone)]
pub struct ConfirmationRequest {
    /// Name of the tool being invoked (e.g. "bash", "write").
    pub tool_name: String,
    /// Assessed risk level that triggered the confirmation.
    pub risk_level: RiskLevel,
    /// Human-readable description of what the tool will do.
    pub description: String,
}

/// The user's response to a confirmation request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfirmationResponse {
    /// User approved the operation.
    Approved,
    /// User denied the operation.
    Denied,
}

/// Callback trait for interactive tool confirmation.
///
/// Implementations bridge the risk gate to a user-facing channel:
/// - REPL mode: prompt on terminal and wait for y/n
/// - Pipe mode: always deny (non-interactive)
/// - Web mode: send confirmation event and await response
pub trait ConfirmationCallback: Send + Sync {
    /// Ask the user to confirm a high-risk operation.
    /// Returns `Approved` or `Denied`.
    fn confirm(&self, request: &ConfirmationRequest) -> ConfirmationResponse;
}

/// A callback that always denies — safe default for non-interactive contexts.
pub struct DenyAllConfirmation;

impl ConfirmationCallback for DenyAllConfirmation {
    fn confirm(&self, _request: &ConfirmationRequest) -> ConfirmationResponse {
        ConfirmationResponse::Denied
    }
}

impl RiskLevel {
    #[must_use]
    pub fn from_score(score: f32) -> Self {
        if score < 0.2 {
            Self::Allow
        } else if score < 0.5 {
            Self::Review
        } else if score < 0.8 {
            Self::RequireConfirmation
        } else {
            Self::Block
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn composite_in_range() {
        let s = RiskScore::new(0.5, 0.3, 0.7, 0.2);
        let c = s.composite_score();
        assert!(c > 0.0);
        assert!(c <= 1.0);
    }

    #[test]
    fn zero_risk() {
        let s = RiskScore::new(0.0, 0.0, 0.0, 0.0);
        let c = s.composite_score();
        assert!(c.abs() < f32::EPSILON);
    }

    #[test]
    fn risk_level_boundaries() {
        assert_eq!(RiskLevel::from_score(0.0), RiskLevel::Allow);
        assert_eq!(RiskLevel::from_score(0.19), RiskLevel::Allow);
        assert_eq!(RiskLevel::from_score(0.2), RiskLevel::Review);
        assert_eq!(RiskLevel::from_score(0.5), RiskLevel::RequireConfirmation);
        assert_eq!(RiskLevel::from_score(0.8), RiskLevel::Block);
    }

    #[test]
    fn depth_decay_increases_risk() {
        let base = RiskScore::new(0.3, 0.1, 0.3, 0.1);
        let depth1 = base.with_depth_decay(1);
        assert!(depth1.tool_risk > base.tool_risk);
        assert!(depth1.blast_radius > base.blast_radius);
    }

    #[test]
    fn deny_all_always_denies() {
        let cb = DenyAllConfirmation;
        let req = ConfirmationRequest {
            tool_name: "bash".into(),
            risk_level: RiskLevel::RequireConfirmation,
            description: "rm -rf /tmp/test".into(),
        };
        assert_eq!(cb.confirm(&req), ConfirmationResponse::Denied);
    }

    #[test]
    fn confirmation_request_carries_context() {
        let req = ConfirmationRequest {
            tool_name: "write".into(),
            risk_level: RiskLevel::Block,
            description: "overwrite /etc/passwd".into(),
        };
        assert_eq!(req.tool_name, "write");
        assert_eq!(req.risk_level, RiskLevel::Block);
        assert!(req.description.contains("passwd"));
    }
}
