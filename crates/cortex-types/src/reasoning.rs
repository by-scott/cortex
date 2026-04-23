use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningMode {
    ChainOfThought,
    TreeOfThought,
    HypothesisTest,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReasoningStepType {
    Premise,
    Inference,
    Hypothesis,
    Evidence,
    Conclusion,
    Branch,
    Evaluation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EvidenceStrength {
    Strong,
    Moderate,
    Weak,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningStep {
    pub index: usize,
    pub step_type: ReasoningStepType,
    pub content: String,
    pub confidence: f64,
    pub evidence_refs: Vec<String>,
    pub branch_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningChain {
    pub id: String,
    pub mode: ReasoningMode,
    pub steps: Vec<ReasoningStep>,
    pub conclusion: Option<String>,
    pub overall_confidence: f64,
}

impl fmt::Display for ReasoningMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ChainOfThought => write!(f, "CoT"),
            Self::TreeOfThought => write!(f, "ToT"),
            Self::HypothesisTest => write!(f, "HypothesisTest"),
        }
    }
}

impl fmt::Display for ReasoningStepType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl fmt::Display for EvidenceStrength {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self, f)
    }
}

impl ReasoningStep {
    #[must_use]
    pub fn new(index: usize, step_type: ReasoningStepType, content: impl Into<String>) -> Self {
        Self {
            index,
            step_type,
            content: content.into(),
            confidence: 0.5,
            evidence_refs: Vec::new(),
            branch_id: None,
        }
    }

    #[must_use]
    pub const fn with_confidence(mut self, confidence: f64) -> Self {
        self.confidence = if confidence < 0.0 {
            0.0
        } else if confidence > 1.0 {
            1.0
        } else {
            confidence
        };
        self
    }

    #[must_use]
    pub fn with_branch(mut self, branch_id: impl Into<String>) -> Self {
        self.branch_id = Some(branch_id.into());
        self
    }

    #[must_use]
    pub fn with_evidence(mut self, refs: Vec<String>) -> Self {
        self.evidence_refs = refs;
        self
    }
}

const CONCLUSION_WEIGHT: f64 = 2.0;
const ABANDON_MIN_STEPS: usize = 3;
const ABANDON_THRESHOLD: f64 = 0.2;

impl ReasoningChain {
    #[must_use]
    pub fn new(mode: ReasoningMode) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            mode,
            steps: Vec::new(),
            conclusion: None,
            overall_confidence: 0.0,
        }
    }

    pub fn add_step(&mut self, step: ReasoningStep) {
        self.steps.push(step);
        self.recalculate_confidence();
    }

    pub fn finalize(&mut self, conclusion: impl Into<String>, confidence: f64) {
        self.conclusion = Some(conclusion.into());
        self.overall_confidence = confidence.clamp(0.0, 1.0);
    }

    fn recalculate_confidence(&mut self) {
        if self.steps.is_empty() {
            self.overall_confidence = 0.0;
            return;
        }
        let (weighted_sum, total_weight) =
            self.steps.iter().fold((0.0, 0.0), |(sum, weight), step| {
                let w = if step.step_type == ReasoningStepType::Conclusion {
                    CONCLUSION_WEIGHT
                } else {
                    1.0
                };
                (step.confidence.mul_add(w, sum), weight + w)
            });
        self.overall_confidence = weighted_sum / total_weight;
    }

    #[must_use]
    pub fn should_abandon(&self) -> bool {
        if self.steps.len() < ABANDON_MIN_STEPS {
            return false;
        }
        self.steps
            .iter()
            .rev()
            .take(ABANDON_MIN_STEPS)
            .all(|s| s.confidence < ABANDON_THRESHOLD)
    }

    #[must_use]
    pub const fn step_count(&self) -> usize {
        self.steps.len()
    }

    #[must_use]
    pub fn format_context(&self) -> String {
        match self.mode {
            ReasoningMode::ChainOfThought => self.format_cot(),
            ReasoningMode::TreeOfThought => self.format_tot(),
            ReasoningMode::HypothesisTest => self.format_hypothesis(),
        }
    }

    fn format_cot(&self) -> String {
        use std::fmt::Write;
        let mut out = format!("[Reasoning Chain -- {}]\n", self.mode);
        for step in &self.steps {
            let _ = writeln!(
                out,
                "  {}. [{}] {} (confidence: {:.0}%)",
                step.index,
                step.step_type,
                step.content,
                step.confidence * 100.0
            );
        }
        if let Some(c) = &self.conclusion {
            let _ = writeln!(out, "  Conclusion: {c}");
        }
        out
    }

    fn format_tot(&self) -> String {
        use std::fmt::Write;
        let mut out = format!("[Reasoning Chain -- {}]\n", self.mode);
        for step in &self.steps {
            let branch = step
                .branch_id
                .as_deref()
                .map_or(String::new(), |b| format!(" [branch:{b}]"));
            let _ = writeln!(
                out,
                "  {}. [{}]{} {} (confidence: {:.0}%)",
                step.index,
                step.step_type,
                branch,
                step.content,
                step.confidence * 100.0
            );
        }
        out
    }

    fn format_hypothesis(&self) -> String {
        use std::fmt::Write;
        let mut out = format!("[Reasoning Chain -- {}]\n", self.mode);
        for step in &self.steps {
            let marker = if step.confidence >= 0.5 {
                "[supports]"
            } else {
                "[contradicts]"
            };
            let _ = writeln!(
                out,
                "  {}. [{}] {} {} (confidence: {:.0}%)",
                step.index,
                step.step_type,
                marker,
                step.content,
                step.confidence * 100.0
            );
        }
        out
    }
}
