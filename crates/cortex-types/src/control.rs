use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ControlSignal {
    Continue,
    Retrieve,
    AskHuman,
    RequestPermission,
    CallTool,
    ConsolidateMemory,
    RepairDelivery,
    Stop,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct EvidenceSignal {
    pub support: f32,
    pub conflict: f32,
    pub risk: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Accumulator {
    pub drift: f32,
    pub boundary: f32,
    pub value: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ExpectedControlValue {
    pub benefit: f32,
    pub cost: f32,
    pub risk: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlDecision {
    pub signal: ControlSignal,
    pub confidence: f32,
    pub rationale: String,
}

impl Accumulator {
    #[must_use]
    pub const fn new(drift: f32, boundary: f32) -> Self {
        Self {
            drift,
            boundary,
            value: 0.0,
        }
    }

    #[must_use]
    pub fn step(mut self, evidence: EvidenceSignal) -> Self {
        let signed = evidence.support - evidence.conflict - evidence.risk;
        self.value = self
            .drift
            .mul_add(signed, self.value)
            .clamp(-self.boundary, self.boundary);
        self
    }

    #[must_use]
    pub fn confidence(&self) -> f32 {
        if self.boundary <= f32::EPSILON {
            return 0.0;
        }
        (self.value.abs() / self.boundary).clamp(0.0, 1.0)
    }
}

impl ExpectedControlValue {
    #[must_use]
    pub fn score(self) -> f32 {
        (self.benefit - self.cost - self.risk).clamp(-1.0, 1.0)
    }
}

impl ControlDecision {
    #[must_use]
    pub fn decide(accumulator: &Accumulator, value: ExpectedControlValue) -> Self {
        let score = value.score();
        let confidence = accumulator.confidence().max(score.abs());
        let signal = if value.risk >= 0.8 {
            ControlSignal::RequestPermission
        } else if accumulator.value <= -accumulator.boundary * 0.75 {
            ControlSignal::AskHuman
        } else if score > 0.35 {
            ControlSignal::Retrieve
        } else if score < -0.35 {
            ControlSignal::Stop
        } else {
            ControlSignal::Continue
        };
        Self {
            signal,
            confidence,
            rationale: format!("evc={score:.3},acc={:.3}", accumulator.value),
        }
    }
}
