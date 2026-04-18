use cortex_types::{
    EvidenceStrength, Payload, ReasoningChain, ReasoningMode, ReasoningStep, ReasoningStepType,
};

/// Minimum input length (estimated tokens approximately chars/4) to consider activating reasoning.
const MIN_COMPLEXITY_CHARS: usize = 200;

/// Keywords that suggest diagnostic/root-cause reasoning leading to `HypothesisTest`.
const HYPOTHESIS_KEYWORDS: &[&str] = &[
    "why",
    "root cause",
    "debug",
    "investigate",
    "diagnose",
    "fault",
    "failure",
    "\u{4e3a}\u{4ec0}\u{4e48}",
    "\u{6839}\u{56e0}",
    "\u{8c03}\u{8bd5}",
    "\u{6392}\u{67e5}",
    "\u{8bca}\u{65ad}",
];

/// Keywords that suggest comparative/exploratory reasoning leading to `TreeOfThought`.
const TREE_KEYWORDS: &[&str] = &[
    "compare",
    "trade-off",
    "tradeoff",
    "best approach",
    "alternatives",
    "which option",
    "pros and cons",
    "\u{6bd4}\u{8f83}",
    "\u{6743}\u{8861}",
    "\u{54ea}\u{79cd}\u{65b9}\u{6848}",
    "\u{66ff}\u{4ee3}\u{65b9}\u{6848}",
    "\u{4f18}\u{7f3a}\u{70b9}",
];

/// Keywords that indicate complex reasoning is needed (beyond simple queries).
const COMPLEXITY_INDICATORS: &[&str] = &[
    "analyze",
    "explain why",
    "reason",
    "deduce",
    "because",
    "therefore",
    "if then",
    "step by step",
    "multi-step",
    "complex",
    "\u{5206}\u{6790}",
    "\u{63a8}\u{7406}",
    "\u{56e0}\u{4e3a}",
    "\u{6240}\u{4ee5}",
    "\u{9010}\u{6b65}",
    "\u{591a}\u{6b65}",
];

/// Engine that manages reasoning chain lifecycle within a turn's `TPN` phase.
pub struct ReasoningEngine {
    chain: Option<ReasoningChain>,
    step_counter: usize,
}

impl ReasoningEngine {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            chain: None,
            step_counter: 0,
        }
    }

    /// Determine if reasoning should be activated for this input.
    /// Requires both sufficient length and complexity indicator keywords.
    #[must_use]
    pub fn should_activate(input: &str) -> bool {
        if input.len() < MIN_COMPLEXITY_CHARS {
            return false;
        }
        let lower = input.to_lowercase();
        COMPLEXITY_INDICATORS.iter().any(|kw| lower.contains(kw))
            || HYPOTHESIS_KEYWORDS.iter().any(|kw| lower.contains(kw))
            || TREE_KEYWORDS.iter().any(|kw| lower.contains(kw))
    }

    /// Select the most appropriate reasoning mode based on input keywords.
    #[must_use]
    pub fn select_mode(input: &str) -> ReasoningMode {
        let lower = input.to_lowercase();
        if HYPOTHESIS_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
            return ReasoningMode::HypothesisTest;
        }
        if TREE_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
            return ReasoningMode::TreeOfThought;
        }
        ReasoningMode::ChainOfThought
    }

    /// Activate reasoning with the selected mode. Returns `ReasoningStarted` event.
    pub fn activate(&mut self, mode: ReasoningMode, input: &str) -> Payload {
        let chain = ReasoningChain::new(mode);
        let summary = if input.len() > 100 {
            format!("{}...", &input[..100])
        } else {
            input.to_string()
        };
        let event = Payload::ReasoningStarted {
            mode: mode.to_string(),
            input_summary: summary,
        };
        self.chain = Some(chain);
        self.step_counter = 0;
        event
    }

    /// Check if a reasoning chain is currently active.
    #[must_use]
    pub const fn is_active(&self) -> bool {
        self.chain.is_some()
    }

    /// Get a reference to the active chain (if any).
    #[must_use]
    pub const fn chain(&self) -> Option<&ReasoningChain> {
        self.chain.as_ref()
    }

    /// Format the current reasoning context for LLM injection.
    #[must_use]
    pub fn format_context(&self) -> Option<String> {
        self.chain.as_ref().and_then(|c| {
            let ctx = c.format_context();
            if ctx.is_empty() { None } else { Some(ctx) }
        })
    }

    // -- CoT: Chain of Thought --

    /// Record a chain-of-thought reasoning step from LLM response text.
    /// Extracts inference content and estimates confidence from text cues.
    pub fn record_cot_step(&mut self, content: &str, confidence: f64) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        if chain.mode != ReasoningMode::ChainOfThought {
            return None;
        }
        let step = ReasoningStep::new(
            self.step_counter,
            ReasoningStepType::Inference,
            content.to_string(),
        )
        .with_confidence(confidence);
        chain.add_step(step);
        let event = Payload::ReasoningStepCompleted {
            step_index: self.step_counter,
            step_type: "Inference".into(),
            confidence,
        };
        self.step_counter += 1;
        Some(event)
    }

    // -- ToT: Tree of Thought --

    /// Record a tree-of-thought branch candidate.
    pub fn record_branch(
        &mut self,
        branch_id: &str,
        content: &str,
        confidence: f64,
    ) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        if chain.mode != ReasoningMode::TreeOfThought {
            return None;
        }
        let step = ReasoningStep::new(
            self.step_counter,
            ReasoningStepType::Branch,
            content.to_string(),
        )
        .with_confidence(confidence)
        .with_branch(branch_id.to_string());
        chain.add_step(step);
        let event = Payload::ReasoningStepCompleted {
            step_index: self.step_counter,
            step_type: "Branch".into(),
            confidence,
        };
        self.step_counter += 1;
        Some(event)
    }

    /// Evaluate and select a branch. Returns both `StepCompleted` and `BranchEvaluated` events.
    pub fn evaluate_branch(&mut self, branch_id: &str, score: f64, selected: bool) -> Vec<Payload> {
        let mut events = Vec::new();
        let Some(chain) = self.chain.as_mut() else {
            return events;
        };
        if chain.mode != ReasoningMode::TreeOfThought {
            return events;
        }
        let step = ReasoningStep::new(
            self.step_counter,
            ReasoningStepType::Evaluation,
            format!("Branch {branch_id} scored {score:.2}"),
        )
        .with_confidence(score)
        .with_branch(branch_id.to_string());
        chain.add_step(step);
        events.push(Payload::ReasoningStepCompleted {
            step_index: self.step_counter,
            step_type: "Evaluation".into(),
            confidence: score,
        });
        events.push(Payload::ReasoningBranchEvaluated {
            branch_id: branch_id.to_string(),
            score,
            selected,
        });
        self.step_counter += 1;
        events
    }

    // -- HypothesisTest --

    /// Record a hypothesis.
    pub fn record_hypothesis(&mut self, content: &str, confidence: f64) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        if chain.mode != ReasoningMode::HypothesisTest {
            return None;
        }
        let step = ReasoningStep::new(
            self.step_counter,
            ReasoningStepType::Hypothesis,
            content.to_string(),
        )
        .with_confidence(confidence);
        chain.add_step(step);
        let event = Payload::ReasoningStepCompleted {
            step_index: self.step_counter,
            step_type: "Hypothesis".into(),
            confidence,
        };
        self.step_counter += 1;
        Some(event)
    }

    /// Record evidence (supporting or contradicting).
    pub fn record_evidence(&mut self, content: &str, confidence: f64) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        if chain.mode != ReasoningMode::HypothesisTest {
            return None;
        }
        let step = ReasoningStep::new(
            self.step_counter,
            ReasoningStepType::Evidence,
            content.to_string(),
        )
        .with_confidence(confidence);
        chain.add_step(step);
        let event = Payload::ReasoningStepCompleted {
            step_index: self.step_counter,
            step_type: "Evidence".into(),
            confidence,
        };
        self.step_counter += 1;
        Some(event)
    }

    // -- Generic completion --

    /// Track a reasoning step from LLM response based on the active mode.
    /// This is the main entry point called from the orchestrator after each LLM response.
    /// It analyzes the response text and records appropriate steps.
    pub fn track_step(&mut self, response_text: &str) -> Vec<Payload> {
        let mut events = Vec::new();
        let Some(chain) = &self.chain else {
            return events;
        };
        let mode = chain.mode;
        let confidence = estimate_confidence(response_text);

        match mode {
            ReasoningMode::ChainOfThought => {
                if let Some(ev) = self.record_cot_step(response_text, confidence) {
                    events.push(ev);
                }
            }
            ReasoningMode::TreeOfThought => {
                // In tree-of-thought, we track each response as a branch unless it's an evaluation
                let branch_id = format!("b{}", self.step_counter);
                if response_text.to_lowercase().contains("best")
                    || response_text.to_lowercase().contains("selected")
                    || response_text.to_lowercase().contains("choose")
                {
                    let eval_events = self.evaluate_branch(&branch_id, confidence, true);
                    events.extend(eval_events);
                } else if let Some(ev) = self.record_branch(&branch_id, response_text, confidence) {
                    events.push(ev);
                }
            }
            ReasoningMode::HypothesisTest => {
                let lower = response_text.to_lowercase();
                if lower.contains("hypothesis") || lower.contains("\u{5047}\u{8bbe}") {
                    if let Some(ev) = self.record_hypothesis(response_text, confidence) {
                        events.push(ev);
                    }
                } else if lower.contains("evidence")
                    || lower.contains("\u{8bc1}\u{636e}")
                    || lower.contains("found")
                    || lower.contains("observed")
                {
                    if let Some(ev) = self.record_evidence(response_text, confidence) {
                        events.push(ev);
                    }
                } else {
                    // Default to evidence in hypothesis mode
                    if let Some(ev) = self.record_evidence(response_text, confidence) {
                        events.push(ev);
                    }
                }
            }
        }
        events
    }

    /// Check if the chain should be abandoned due to low confidence.
    #[must_use]
    pub fn should_abandon(&self) -> bool {
        self.chain
            .as_ref()
            .is_some_and(ReasoningChain::should_abandon)
    }

    /// Refine the current hypothesis based on accumulated evidence.
    ///
    /// Calculates the ratio of supporting (confidence >= 0.5) vs contradicting
    /// evidence and adjusts the hypothesis step's confidence accordingly.
    /// Returns a `ReasoningStepCompleted` event if refinement occurred.
    pub fn refine_hypothesis(&mut self) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        if chain.mode != ReasoningMode::HypothesisTest {
            return None;
        }

        let evidence_steps: Vec<&ReasoningStep> = chain
            .steps
            .iter()
            .filter(|s| s.step_type == ReasoningStepType::Evidence)
            .collect();

        if evidence_steps.is_empty() {
            return None;
        }

        let supporting = evidence_steps
            .iter()
            .filter(|e| e.confidence >= 0.5)
            .count();
        let total = evidence_steps.len();
        let support_u32 = u32::try_from(supporting).unwrap_or(u32::MAX);
        let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);
        let support_ratio = f64::from(support_u32) / f64::from(total_u32);

        // Find the hypothesis step and update its confidence
        let hypothesis = chain
            .steps
            .iter_mut()
            .rev()
            .find(|s| s.step_type == ReasoningStepType::Hypothesis)?;
        let old_confidence = hypothesis.confidence;
        // Blend: 40% original + 60% evidence support ratio
        hypothesis.confidence = old_confidence
            .mul_add(0.4, support_ratio * 0.6)
            .clamp(0.05, 0.95);
        let new_confidence = hypothesis.confidence;

        Some(Payload::ReasoningStepCompleted {
            step_index: hypothesis.index,
            step_type: "HypothesisRefined".into(),
            confidence: new_confidence,
        })
    }

    /// Complete the reasoning chain and return the completion event.
    pub fn complete(&mut self, conclusion: &str) -> Option<Payload> {
        let chain = self.chain.as_mut()?;
        let confidence = if chain.steps.is_empty() {
            0.5
        } else {
            chain.overall_confidence
        };
        chain.finalize(conclusion.to_string(), confidence);
        let event = Payload::ReasoningChainCompleted {
            chain_id: chain.id.clone(),
            mode: chain.mode.to_string(),
            step_count: chain.step_count(),
            overall_confidence: chain.overall_confidence,
            conclusion_summary: if conclusion.len() > 200 {
                format!("{}...", &conclusion[..200])
            } else {
                conclusion.to_string()
            },
        };
        Some(event)
    }
}

impl Default for ReasoningEngine {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify evidence text strength based on linguistic indicators.
///
/// - **Strong**: definitive, confirmatory language ("conclusive", "proves", "confirms")
/// - **Weak**: tentative, uncertain language ("possibly", "unclear", "might")
/// - **Moderate**: everything else
#[must_use]
pub fn classify_evidence(text: &str) -> EvidenceStrength {
    let lower = text.to_lowercase();

    let strong_cues = [
        "conclusive",
        "proves",
        "confirms",
        "verified",
        "definitive",
        "\u{786e}\u{8ba4}",
        "\u{8bc1}\u{5b9e}",
        "\u{786e}\u{5b9a}",
    ];
    let weak_cues = [
        "possibly",
        "unclear",
        "might",
        "perhaps",
        "uncertain",
        "\u{4e0d}\u{786e}\u{5b9a}",
        "\u{53ef}\u{80fd}",
        "\u{4e5f}\u{8bb8}",
    ];

    let strong_hits = strong_cues.iter().filter(|c| lower.contains(*c)).count();
    let weak_hits = weak_cues.iter().filter(|c| lower.contains(*c)).count();

    if strong_hits > weak_hits {
        EvidenceStrength::Strong
    } else if weak_hits > strong_hits {
        EvidenceStrength::Weak
    } else if strong_hits > 0 {
        // Tied but both present -- moderate
        EvidenceStrength::Moderate
    } else {
        EvidenceStrength::Moderate
    }
}

/// Score the quality of a completed reasoning chain.
///
/// Three dimensions (each 0.0..1.0):
/// - **Step diversity**: proportion of distinct step types used (max 7 types)
/// - **Evidence coverage**: evidence steps / total steps (higher = better informed)
/// - **Confidence trajectory**: whether confidence improved over the chain
///
/// Final score = diversity * 0.4 + coverage * 0.3 + trajectory * 0.3
#[must_use]
pub fn score_chain_quality(chain: &ReasoningChain) -> f64 {
    if chain.steps.is_empty() {
        return 0.0;
    }

    // Step diversity: how many distinct step types appear?
    let mut type_set = std::collections::HashSet::new();
    for step in &chain.steps {
        type_set.insert(std::mem::discriminant(&step.step_type));
    }
    let diversity = f64::from(u32::try_from(type_set.len()).unwrap_or(u32::MAX)) / 7.0;
    let diversity = diversity.min(1.0);

    // Evidence coverage
    let evidence_count = chain
        .steps
        .iter()
        .filter(|s| s.step_type == ReasoningStepType::Evidence)
        .count();
    let total = chain.steps.len();
    let ev_u32 = u32::try_from(evidence_count).unwrap_or(u32::MAX);
    let total_u32 = u32::try_from(total).unwrap_or(u32::MAX);
    let coverage = if total > 0 {
        f64::from(ev_u32) / f64::from(total_u32)
    } else {
        0.0
    };

    // Confidence trajectory: compare first-half avg vs second-half avg
    let half = total / 2;
    let trajectory = if half > 0 && total > 1 {
        let half_u32 = u32::try_from(half).unwrap_or(u32::MAX);
        let second_len_u32 = u32::try_from(total - half).unwrap_or(u32::MAX);
        let first_half_avg: f64 = chain.steps[..half]
            .iter()
            .map(|s| s.confidence)
            .sum::<f64>()
            / f64::from(half_u32);
        let second_half_avg: f64 = chain.steps[half..]
            .iter()
            .map(|s| s.confidence)
            .sum::<f64>()
            / f64::from(second_len_u32);
        // Normalize: if second > first, trajectory is positive (up to 1.0)
        ((second_half_avg - first_half_avg) + 0.5).clamp(0.0, 1.0)
    } else {
        0.5
    };

    diversity.mul_add(0.4, coverage.mul_add(0.3, trajectory * 0.3))
}

/// Estimate confidence from LLM response text based on linguistic cues.
fn estimate_confidence(text: &str) -> f64 {
    let lower = text.to_lowercase();

    // High-confidence indicators
    let high_cues = [
        "certainly",
        "definitely",
        "clearly",
        "\u{786e}\u{5b9a}",
        "clearly shows",
        "it is clear",
        "without doubt",
    ];
    // Low-confidence indicators
    let low_cues = [
        "possibly",
        "might",
        "perhaps",
        "uncertain",
        "\u{4e0d}\u{786e}\u{5b9a}",
        "may be",
        "unclear",
        "I'm not sure",
    ];

    let high_count = high_cues.iter().filter(|c| lower.contains(*c)).count();
    let low_count = low_cues.iter().filter(|c| lower.contains(*c)).count();

    let base = 0.5;
    let high = u32::try_from(high_count).unwrap_or(u32::MAX);
    let low = u32::try_from(low_count).unwrap_or(u32::MAX);
    let delta = f64::from(high).mul_add(0.15, -f64::from(low) * 0.15);
    (base + delta).clamp(0.1, 0.95)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_activate_short_input() {
        assert!(!ReasoningEngine::should_activate("hello"));
        assert!(!ReasoningEngine::should_activate("what is rust?"));
    }

    #[test]
    fn should_activate_long_complex_input() {
        let input = format!(
            "Please analyze this complex problem step by step. {}",
            "x ".repeat(100)
        );
        assert!(ReasoningEngine::should_activate(&input));
    }

    #[test]
    fn should_activate_long_but_simple_input() {
        let input = "a ".repeat(200); // long but no complexity keywords
        assert!(!ReasoningEngine::should_activate(&input));
    }

    #[test]
    fn select_mode_hypothesis() {
        assert_eq!(
            ReasoningEngine::select_mode("Why is this failing? Please investigate the root cause"),
            ReasoningMode::HypothesisTest
        );
        assert_eq!(
            ReasoningEngine::select_mode(
                "\u{8bf7}\u{8bca}\u{65ad}\u{8fd9}\u{4e2a}\u{95ee}\u{9898}\u{7684}\u{6839}\u{56e0}"
            ),
            ReasoningMode::HypothesisTest
        );
    }

    #[test]
    fn select_mode_tree() {
        assert_eq!(
            ReasoningEngine::select_mode("Compare these alternatives and list pros and cons"),
            ReasoningMode::TreeOfThought
        );
    }

    #[test]
    fn select_mode_default_cot() {
        assert_eq!(
            ReasoningEngine::select_mode("explain how this algorithm works"),
            ReasoningMode::ChainOfThought
        );
    }

    #[test]
    fn activate_creates_chain() {
        let mut engine = ReasoningEngine::new();
        assert!(!engine.is_active());
        let ev = engine.activate(ReasoningMode::ChainOfThought, "test input");
        assert!(engine.is_active());
        assert!(matches!(ev, Payload::ReasoningStarted { .. }));
    }

    #[test]
    fn cot_step_tracking() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        let ev = engine.record_cot_step("First, we observe X", 0.7);
        assert!(ev.is_some());
        assert!(matches!(
            ev.unwrap(),
            Payload::ReasoningStepCompleted { step_index: 0, .. }
        ));

        let ev2 = engine.record_cot_step("Therefore Y follows", 0.8);
        assert!(matches!(
            ev2.unwrap(),
            Payload::ReasoningStepCompleted { step_index: 1, .. }
        ));
    }

    #[test]
    fn tot_branch_and_evaluate() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::TreeOfThought, "test");
        let ev = engine.record_branch("a", "Path A: use caching", 0.7);
        assert!(ev.is_some());
        let ev2 = engine.record_branch("b", "Path B: use streaming", 0.6);
        assert!(ev2.is_some());
        let eval_events = engine.evaluate_branch("a", 0.9, true);
        assert_eq!(eval_events.len(), 2);
        assert!(matches!(
            eval_events[1],
            Payload::ReasoningBranchEvaluated { selected: true, .. }
        ));
    }

    #[test]
    fn hypothesis_test_flow() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::HypothesisTest, "test");
        let ev = engine.record_hypothesis("The bug is in auth middleware", 0.6);
        assert!(ev.is_some());
        let ev2 = engine.record_evidence("Log shows 401 at /api/turn", 0.8);
        assert!(ev2.is_some());
        let ev3 = engine.record_evidence("Token is valid per JWT decode", 0.3);
        assert!(ev3.is_some());
    }

    #[test]
    fn track_step_cot() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        let events = engine.track_step("The system clearly shows a pattern of failure");
        assert_eq!(events.len(), 1);
    }

    #[test]
    fn track_step_tot_evaluation() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::TreeOfThought, "test");
        let events = engine.track_step("Based on analysis, the best option is A");
        assert_eq!(events.len(), 2); // StepCompleted + BranchEvaluated
    }

    #[test]
    fn track_step_hypothesis() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::HypothesisTest, "test");
        let events = engine.track_step("My hypothesis is that the memory leak comes from...");
        assert_eq!(events.len(), 1);
        assert!(
            matches!(events[0], Payload::ReasoningStepCompleted { step_type: ref t, .. } if t == "Hypothesis")
        );
    }

    #[test]
    fn complete_chain() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        engine.record_cot_step("Step 1", 0.7);
        engine.record_cot_step("Step 2", 0.8);
        let ev = engine.complete("Final conclusion");
        assert!(ev.is_some());
        if let Some(Payload::ReasoningChainCompleted { step_count, .. }) = ev {
            assert_eq!(step_count, 2);
        }
    }

    #[test]
    fn format_context_when_active() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        engine.record_cot_step("First observation", 0.7);
        let ctx = engine.format_context();
        assert!(ctx.is_some());
        assert!(ctx.unwrap().contains("First observation"));
    }

    #[test]
    fn format_context_when_inactive() {
        let engine = ReasoningEngine::new();
        assert!(engine.format_context().is_none());
    }

    #[test]
    fn should_abandon_low_confidence() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        for i in 0..3 {
            engine.record_cot_step(&format!("step {i}"), 0.1);
        }
        assert!(engine.should_abandon());
    }

    #[test]
    fn wrong_mode_returns_none() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        // Branch only works in ToT mode
        assert!(engine.record_branch("a", "test", 0.5).is_none());
        // Hypothesis only works in HypothesisTest mode
        assert!(engine.record_hypothesis("test", 0.5).is_none());
    }

    #[test]
    fn estimate_confidence_high() {
        let c = estimate_confidence("This clearly shows the issue is in module X");
        assert!(c > 0.5);
    }

    #[test]
    fn estimate_confidence_low() {
        let c = estimate_confidence("This might possibly be related, I'm uncertain");
        assert!(c < 0.5);
    }

    #[test]
    fn estimate_confidence_neutral() {
        let c = estimate_confidence("The function returns a value");
        assert!((c - 0.5).abs() < 0.01);
    }

    // -- Advanced reasoning tests --

    #[test]
    fn classify_evidence_strong() {
        assert_eq!(
            classify_evidence("This conclusive test proves the hypothesis"),
            EvidenceStrength::Strong,
        );
        assert_eq!(
            classify_evidence(
                "\u{5b9e}\u{9a8c}\u{786e}\u{8ba4}\u{4e86}\u{8fd9}\u{4e2a}\u{7ed3}\u{8bba}"
            ),
            EvidenceStrength::Strong,
        );
    }

    #[test]
    fn classify_evidence_weak() {
        assert_eq!(
            classify_evidence("This possibly might be related, unclear evidence"),
            EvidenceStrength::Weak,
        );
    }

    #[test]
    fn classify_evidence_moderate_default() {
        assert_eq!(
            classify_evidence("The log shows an error at line 42"),
            EvidenceStrength::Moderate,
        );
    }

    #[test]
    fn refine_hypothesis_supporting_evidence() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::HypothesisTest, "test");
        engine.record_hypothesis("Bug in auth", 0.5);
        // Add supporting evidence (confidence >= 0.5)
        engine.record_evidence("Log shows 401 error", 0.8);
        engine.record_evidence("Auth token expired", 0.7);
        engine.record_evidence("Config is correct", 0.6);

        let ev = engine.refine_hypothesis();
        assert!(ev.is_some());
        // All evidence supports (conf >= 0.5), support_ratio = 1.0
        // New confidence = 0.5 * 0.4 + 1.0 * 0.6 = 0.8
        if let Some(Payload::ReasoningStepCompleted { confidence, .. }) = ev {
            assert!(
                confidence > 0.5,
                "confidence should increase with supporting evidence"
            );
        }
    }

    #[test]
    fn refine_hypothesis_contradicting_evidence() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::HypothesisTest, "test");
        engine.record_hypothesis("Memory leak in module X", 0.5);
        // Add contradicting evidence (confidence < 0.5)
        engine.record_evidence("Memory usage is stable", 0.2);
        engine.record_evidence("No allocations found", 0.3);
        engine.record_evidence("Heap profile clean", 0.1);

        let ev = engine.refine_hypothesis();
        assert!(ev.is_some());
        // All evidence contradicts (conf < 0.5), support_ratio = 0.0
        // New confidence = 0.5 * 0.4 + 0.0 * 0.6 = 0.2
        if let Some(Payload::ReasoningStepCompleted { confidence, .. }) = ev {
            assert!(
                confidence < 0.5,
                "confidence should decrease with contradicting evidence"
            );
        }
    }

    #[test]
    fn refine_hypothesis_no_evidence_returns_none() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::HypothesisTest, "test");
        engine.record_hypothesis("Some hypothesis", 0.5);
        assert!(engine.refine_hypothesis().is_none());
    }

    #[test]
    fn refine_hypothesis_wrong_mode_returns_none() {
        let mut engine = ReasoningEngine::new();
        engine.activate(ReasoningMode::ChainOfThought, "test");
        assert!(engine.refine_hypothesis().is_none());
    }

    #[test]
    fn score_chain_quality_empty() {
        let chain = ReasoningChain::new(ReasoningMode::ChainOfThought);
        assert!(score_chain_quality(&chain).abs() < f64::EPSILON);
    }

    #[test]
    fn score_chain_quality_diverse_chain() {
        let mut chain = ReasoningChain::new(ReasoningMode::HypothesisTest);
        chain.add_step(
            ReasoningStep::new(0, ReasoningStepType::Premise, "Given X").with_confidence(0.5),
        );
        chain.add_step(
            ReasoningStep::new(1, ReasoningStepType::Hypothesis, "Hypothesis H")
                .with_confidence(0.6),
        );
        chain.add_step(
            ReasoningStep::new(2, ReasoningStepType::Evidence, "Evidence E1").with_confidence(0.7),
        );
        chain.add_step(
            ReasoningStep::new(3, ReasoningStepType::Conclusion, "Conclusion C")
                .with_confidence(0.8),
        );
        let score = score_chain_quality(&chain);
        assert!(score > 0.5, "diverse chain should score > 0.5, got {score}");
    }

    #[test]
    fn score_chain_quality_monotone_chain() {
        let mut chain = ReasoningChain::new(ReasoningMode::ChainOfThought);
        for i in 0..4 {
            chain.add_step(
                ReasoningStep::new(i, ReasoningStepType::Inference, format!("step {i}"))
                    .with_confidence(0.5),
            );
        }
        let score = score_chain_quality(&chain);
        // Only 1 type out of 7 = diversity 1/7 ~ 0.14, no evidence, flat trajectory
        assert!(
            score < 0.5,
            "monotone chain should score < 0.5, got {score}"
        );
    }
}
