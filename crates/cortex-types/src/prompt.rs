use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PromptLayer {
    Soul,
    Identity,
    Behavioral,
    User,
}

impl PromptLayer {
    #[must_use]
    pub const fn filename(self) -> &'static str {
        match self {
            Self::Soul => "soul.md",
            Self::Identity => "identity.md",
            Self::Behavioral => "behavioral.md",
            Self::User => "user.md",
        }
    }

    #[must_use]
    pub const fn all() -> [Self; 4] {
        [Self::Soul, Self::Identity, Self::Behavioral, Self::User]
    }
}

impl fmt::Display for PromptLayer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Soul => write!(f, "soul"),
            Self::Identity => write!(f, "identity"),
            Self::Behavioral => write!(f, "behavioral"),
            Self::User => write!(f, "user"),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintContext {
    #[serde(default)]
    pub available_capabilities: BTreeSet<String>,
    #[serde(default)]
    pub approved_self_edit: bool,
}

impl LintContext {
    #[must_use]
    pub fn with_available_capabilities<I, S>(mut self, capabilities: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.available_capabilities = capabilities.into_iter().map(Into::into).collect();
        self
    }

    #[must_use]
    pub const fn with_approved_self_edit(mut self, approved: bool) -> Self {
        self.approved_self_edit = approved;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LintRule {
    ForbiddenClaim,
    MissingCapability,
    RuntimePolicyOverride,
    StaleRuntimeState,
    UnapprovedSelfEditDiff,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintViolation {
    pub layer: PromptLayer,
    pub rule: LintRule,
    pub message: String,
    pub evidence: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LintReport {
    pub violations: Vec<LintViolation>,
}

impl LintReport {
    #[must_use]
    pub const fn is_ok(&self) -> bool {
        self.violations.is_empty()
    }

    #[must_use]
    pub fn render(&self) -> String {
        if self.violations.is_empty() {
            return "prompt lint passed".to_string();
        }
        self.violations
            .iter()
            .map(|violation| {
                format!(
                    "{:?} in {}: {} ({})",
                    violation.rule, violation.layer, violation.message, violation.evidence
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[must_use]
pub fn lint(layer: PromptLayer, prompt_text: &str, lint_context: &LintContext) -> LintReport {
    let lower = prompt_text.to_ascii_lowercase();
    let mut violations = Vec::new();
    push_forbidden_claims(layer, &lower, &mut violations);
    push_runtime_policy_overrides(layer, &lower, &mut violations);
    push_stale_runtime_state(layer, &lower, &mut violations);
    push_missing_capabilities(layer, prompt_text, lint_context, &mut violations);
    push_unapproved_self_edit(layer, &lower, lint_context, &mut violations);
    LintReport { violations }
}

fn push_forbidden_claims(layer: PromptLayer, lower: &str, violations: &mut Vec<LintViolation>) {
    for phrase in [
        "production ready",
        "mature multi-tenant",
        "guaranteed secure",
        "cannot leak",
        "fully sandboxed",
        "kernel isolated",
        "complete biological cognition",
    ] {
        if lower.contains(phrase) {
            violations.push(violation(
                layer,
                LintRule::ForbiddenClaim,
                "prompt makes a release, security, or cognition claim that must come from runtime evidence",
                phrase,
            ));
        }
    }
}

fn push_runtime_policy_overrides(
    layer: PromptLayer,
    lower: &str,
    violations: &mut Vec<LintViolation>,
) {
    for phrase in [
        "current permission mode",
        "auto-approve up to",
        "auto approve up to",
        "runtime policy:",
    ] {
        if lower.contains(phrase) {
            violations.push(violation(
                layer,
                LintRule::RuntimePolicyOverride,
                "live runtime policy belongs in the runtime policy section, not durable prompts",
                phrase,
            ));
        }
    }
}

fn push_stale_runtime_state(layer: PromptLayer, lower: &str, violations: &mut Vec<LintViolation>) {
    for phrase in [
        "active session",
        "pending confirmation",
        "last user message",
        "this turn",
        "currently typing",
    ] {
        if lower.contains(phrase) {
            violations.push(violation(
                layer,
                LintRule::StaleRuntimeState,
                "transient runtime state must not fossilize into durable prompts",
                phrase,
            ));
        }
    }
}

fn push_missing_capabilities(
    layer: PromptLayer,
    prompt_text: &str,
    lint_context: &LintContext,
    violations: &mut Vec<LintViolation>,
) {
    for capability in referenced_capabilities(prompt_text) {
        if !lint_context.available_capabilities.contains(&capability) {
            violations.push(violation(
                layer,
                LintRule::MissingCapability,
                "prompt references a capability absent from runtime schemas",
                &capability,
            ));
        }
    }
}

fn push_unapproved_self_edit(
    layer: PromptLayer,
    lower: &str,
    context: &LintContext,
    violations: &mut Vec<LintViolation>,
) {
    if context.approved_self_edit {
        return;
    }
    for phrase in ["```diff", "diff --git", "apply_patch"] {
        if lower.contains(phrase) {
            violations.push(violation(
                layer,
                LintRule::UnapprovedSelfEditDiff,
                "prompt update contains a self-edit diff without an approved edit boundary",
                phrase,
            ));
        }
    }
}

fn referenced_capabilities(content: &str) -> Vec<String> {
    content
        .split_whitespace()
        .filter_map(|token| {
            let token = token.trim_matches(|ch: char| {
                !ch.is_ascii_alphanumeric() && ch != '_' && ch != '-' && ch != ':'
            });
            token
                .strip_prefix("capability:")
                .or_else(|| token.strip_prefix("tool:"))
                .filter(|value| !value.is_empty())
                .map(str::to_string)
        })
        .collect()
}

fn violation(layer: PromptLayer, rule: LintRule, message: &str, evidence: &str) -> LintViolation {
    LintViolation {
        layer,
        rule,
        message: message.to_string(),
        evidence: evidence.to_string(),
    }
}
