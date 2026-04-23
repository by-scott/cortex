use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CausalRelation {
    Triggers,
    Enables,
    Contributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalLink {
    pub cause_event: String,
    pub effect_event: String,
    pub relation: CausalRelation,
    pub confidence: f64,
    pub temporal_delta_ms: i64,
    pub evidence: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChain {
    pub id: String,
    pub links: Vec<CausalLink>,
    pub root_cause: String,
    pub final_effect: String,
    pub overall_confidence: f64,
    pub summary: Option<String>,
}

impl fmt::Display for CausalRelation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Triggers => write!(f, "triggers"),
            Self::Enables => write!(f, "enables"),
            Self::Contributes => write!(f, "contributes to"),
        }
    }
}

impl CausalLink {
    #[must_use]
    pub fn new(
        cause: impl Into<String>,
        effect: impl Into<String>,
        relation: CausalRelation,
        confidence: f64,
    ) -> Self {
        Self {
            cause_event: cause.into(),
            effect_event: effect.into(),
            relation,
            confidence: confidence.clamp(0.0, 1.0),
            temporal_delta_ms: 0,
            evidence: None,
        }
    }

    #[must_use]
    pub const fn with_temporal_delta(mut self, delta_ms: i64) -> Self {
        self.temporal_delta_ms = delta_ms;
        self
    }

    #[must_use]
    pub fn with_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.evidence = Some(evidence.into());
        self
    }
}

impl CausalChain {
    #[must_use]
    pub fn from_links(links: Vec<CausalLink>) -> Self {
        let root_cause = links
            .first()
            .map_or_else(String::new, |l| l.cause_event.clone());
        let final_effect = links
            .last()
            .map_or_else(String::new, |l| l.effect_event.clone());
        let overall_confidence = links.iter().map(|l| l.confidence).product();
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            links,
            root_cause,
            final_effect,
            overall_confidence,
            summary: None,
        }
    }

    #[must_use]
    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    #[must_use]
    pub const fn link_count(&self) -> usize {
        self.links.len()
    }

    #[must_use]
    pub fn format(&self) -> String {
        use std::fmt::Write;
        if self.links.is_empty() {
            return String::from("(empty causal chain)");
        }
        let mut out = String::new();
        for (i, link) in self.links.iter().enumerate() {
            if i == 0 {
                out.push_str(&link.cause_event);
            }
            out.push_str(" --[");
            out.push_str(&link.relation.to_string());
            out.push_str("]--> ");
            out.push_str(&link.effect_event);
        }
        let _ = write!(
            out,
            " (confidence: {:.0}%)",
            self.overall_confidence * 100.0
        );
        if let Some(s) = &self.summary {
            let _ = write!(out, "\n  Summary: {s}");
        }
        out
    }
}
