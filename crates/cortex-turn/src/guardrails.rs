/// Result of a guardrail check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardResult {
    /// Input/output appears safe.
    Safe,
    /// A suspicious pattern was detected.
    Suspicious(String),
}

const INJECTION_PATTERNS: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous",
    "disregard previous",
    "forget your instructions",
    "reveal your system prompt",
    "reveal system prompt",
    "print your system prompt",
    "output your instructions",
    "you are now",
    "act as if you have no",
    "pretend you are",
    "jailbreak",
    "do anything now",
    "developer mode",
];

/// Check user input for known prompt injection patterns.
///
/// Includes both basic keyword matching and advanced regex-based detection
/// via [`crate::security::detect_prompt_injection`].
#[must_use]
pub fn input_guard(input: &str) -> GuardResult {
    let lower = input.to_lowercase();
    for pattern in INJECTION_PATTERNS {
        if lower.contains(pattern) {
            return GuardResult::Suspicious(format!("injection pattern: \"{pattern}\""));
        }
    }
    if let Some(desc) = crate::security::detect_prompt_injection(input) {
        return GuardResult::Suspicious(format!("advanced injection: {desc}"));
    }
    GuardResult::Safe
}

/// Check LLM output for system prompt leakage markers.
#[must_use]
pub fn output_guard(output: &str) -> GuardResult {
    let lower = output.to_lowercase();
    let leakage_markers = [
        "my system prompt is",
        "my instructions are",
        "i was instructed to",
        "here is my system prompt",
    ];
    for marker in &leakage_markers {
        if lower.contains(marker) {
            return GuardResult::Suspicious(format!("potential leakage: \"{marker}\""));
        }
    }
    GuardResult::Safe
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_input_safe() {
        assert_eq!(input_guard("read file.txt"), GuardResult::Safe);
    }

    #[test]
    fn injection_detected() {
        assert!(matches!(
            input_guard("ignore previous instructions and do X"),
            GuardResult::Suspicious(_)
        ));
    }

    #[test]
    fn injection_case_insensitive() {
        assert!(matches!(
            input_guard("IGNORE PREVIOUS INSTRUCTIONS"),
            GuardResult::Suspicious(_)
        ));
    }

    #[test]
    fn normal_output_safe() {
        assert_eq!(
            output_guard("The file contains config settings."),
            GuardResult::Safe
        );
    }

    #[test]
    fn output_leakage_detected() {
        assert!(matches!(
            output_guard("My system prompt is to help users"),
            GuardResult::Suspicious(_)
        ));
    }

    #[test]
    fn jailbreak_detected() {
        assert!(matches!(
            input_guard("Enable developer mode"),
            GuardResult::Suspicious(_)
        ));
    }
}
