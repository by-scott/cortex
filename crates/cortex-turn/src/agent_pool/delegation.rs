/// Contract that bounds one delegated worker.
///
/// A worker never inherits broad parent authority by default. Tool access,
/// evidence access, budgets, artifact expectations, and merge review are stated
/// explicitly so delegation can be reviewed and replayed as a controlled action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DelegationContract {
    pub scope: String,
    pub allowed_tools: Vec<String>,
    pub forbidden_actions: Vec<String>,
    pub token_budget: usize,
    pub iteration_budget: usize,
    pub evidence_budget: usize,
    pub allowed_evidence: Vec<String>,
    pub expected_artifact: String,
    pub merge_verifier: String,
    pub review_required: bool,
    pub inherit_parent_authority: bool,
}

impl DelegationContract {
    pub const DEFAULT_TOKEN_BUDGET: usize = 2048;
    pub const DEFAULT_ITERATION_BUDGET: usize = 1;

    #[must_use]
    pub fn new(scope: impl Into<String>, expected_artifact: impl Into<String>) -> Self {
        Self {
            scope: scope.into(),
            allowed_tools: Vec::new(),
            forbidden_actions: Vec::new(),
            token_budget: Self::DEFAULT_TOKEN_BUDGET,
            iteration_budget: Self::DEFAULT_ITERATION_BUDGET,
            evidence_budget: 0,
            allowed_evidence: Vec::new(),
            expected_artifact: expected_artifact.into(),
            merge_verifier: "parent_review".to_string(),
            review_required: true,
            inherit_parent_authority: false,
        }
    }

    #[must_use]
    pub fn readonly(scope: impl Into<String>, expected_artifact: impl Into<String>) -> Self {
        Self::new(scope, expected_artifact)
    }

    #[must_use]
    pub fn with_allowed_tool(mut self, tool: impl Into<String>) -> Self {
        self.allowed_tools.push(tool.into());
        self
    }

    #[must_use]
    pub fn with_forbidden_action(mut self, action: impl Into<String>) -> Self {
        self.forbidden_actions.push(action.into());
        self
    }

    #[must_use]
    pub const fn with_token_budget(mut self, budget: usize) -> Self {
        self.token_budget = budget;
        self
    }

    #[must_use]
    pub const fn with_iteration_budget(mut self, budget: usize) -> Self {
        self.iteration_budget = budget;
        self
    }

    #[must_use]
    pub const fn with_evidence_budget(mut self, budget: usize) -> Self {
        self.evidence_budget = budget;
        self
    }

    #[must_use]
    pub fn with_allowed_evidence(mut self, evidence: impl Into<String>) -> Self {
        self.allowed_evidence.push(evidence.into());
        self
    }

    #[must_use]
    pub fn with_merge_verifier(mut self, verifier: impl Into<String>) -> Self {
        self.merge_verifier = verifier.into();
        self
    }

    #[must_use]
    pub const fn with_review_required(mut self, required: bool) -> Self {
        self.review_required = required;
        self
    }

    #[must_use]
    pub const fn with_parent_authority_inheritance(mut self, inherit: bool) -> Self {
        self.inherit_parent_authority = inherit;
        self
    }

    #[must_use]
    pub fn permits_tool(&self, tool: &str) -> bool {
        self.allowed_tools.iter().any(|allowed| allowed == tool)
            && !self
                .forbidden_actions
                .iter()
                .any(|forbidden| forbidden == tool)
    }

    /// Validate this contract before a worker is started.
    ///
    /// # Errors
    /// Returns `DelegationContractError` when a required contract field is
    /// missing, a budget is zero, or authority inheritance is too broad.
    pub fn validate(&self) -> Result<(), DelegationContractError> {
        if self.scope.trim().is_empty() {
            return Err(DelegationContractError::MissingScope);
        }
        if self.expected_artifact.trim().is_empty() {
            return Err(DelegationContractError::MissingExpectedArtifact);
        }
        if self.merge_verifier.trim().is_empty() {
            return Err(DelegationContractError::MissingMergeVerifier);
        }
        if self.token_budget == 0 {
            return Err(DelegationContractError::ZeroTokenBudget);
        }
        if self.iteration_budget == 0 {
            return Err(DelegationContractError::ZeroIterationBudget);
        }
        if self.inherit_parent_authority && self.allowed_tools.is_empty() {
            return Err(DelegationContractError::BroadAuthorityInheritance);
        }
        Ok(())
    }

    #[must_use]
    pub fn worker_prompt(&self, task_prompt: &str, extra_messages: &[String]) -> String {
        let mut prompt = format!(
            "Delegation contract:\n\
             - scope: {}\n\
             - allowed_tools: {}\n\
             - forbidden_actions: {}\n\
             - token_budget: {}\n\
             - iteration_budget: {}\n\
             - evidence_budget: {}\n\
             - allowed_evidence: {}\n\
             - expected_artifact: {}\n\
             - merge_verifier: {}\n\
             - review_required: {}\n\
             - inherit_parent_authority: {}\n\n\
             Task:\n{}",
            self.scope,
            list_or_none(&self.allowed_tools),
            list_or_none(&self.forbidden_actions),
            self.token_budget,
            self.iteration_budget,
            self.evidence_budget,
            list_or_none(&self.allowed_evidence),
            self.expected_artifact,
            self.merge_verifier,
            self.review_required,
            self.inherit_parent_authority,
            task_prompt
        );
        if !extra_messages.is_empty() {
            prompt.push_str("\n\nAdditional routed context:\n");
            prompt.push_str(&extra_messages.join("\n"));
        }
        prompt
    }
}

impl Default for DelegationContract {
    fn default() -> Self {
        Self::readonly("bounded investigation", "written answer")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DelegationContractError {
    MissingScope,
    MissingExpectedArtifact,
    MissingMergeVerifier,
    ZeroTokenBudget,
    ZeroIterationBudget,
    BroadAuthorityInheritance,
}

impl std::fmt::Display for DelegationContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingScope => write!(f, "delegation contract missing scope"),
            Self::MissingExpectedArtifact => {
                write!(f, "delegation contract missing expected artifact")
            }
            Self::MissingMergeVerifier => write!(f, "delegation contract missing merge verifier"),
            Self::ZeroTokenBudget => write!(f, "delegation contract token budget is zero"),
            Self::ZeroIterationBudget => write!(f, "delegation contract iteration budget is zero"),
            Self::BroadAuthorityInheritance => {
                write!(
                    f,
                    "delegation contract inherits parent authority too broadly"
                )
            }
        }
    }
}

impl std::error::Error for DelegationContractError {}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::{DelegationContract, DelegationContractError};

    #[test]
    fn delegation_contract_filters_tools_and_validates_budgets() {
        let contract = DelegationContract::readonly("read source files", "summary")
            .with_allowed_tool("read")
            .with_forbidden_action("bash")
            .with_token_budget(512)
            .with_iteration_budget(2)
            .with_allowed_evidence("src/**/*.rs");

        assert!(contract.validate().is_ok());
        assert!(contract.permits_tool("read"));
        assert!(!contract.permits_tool("bash"));
        assert!(!contract.permits_tool("write"));
    }

    #[test]
    fn delegation_contract_rejects_broad_authority_inheritance() {
        let contract = DelegationContract::readonly("implement isolated change", "patch")
            .with_parent_authority_inheritance(true);

        assert_eq!(
            contract.validate(),
            Err(DelegationContractError::BroadAuthorityInheritance)
        );
    }

    #[test]
    fn worker_prompt_renders_the_review_contract() {
        let contract = DelegationContract::readonly("inspect config", "risk list")
            .with_allowed_tool("read")
            .with_merge_verifier("operator_review");
        let rendered = contract.worker_prompt("Find risky config.", &["config.toml".to_string()]);

        assert!(rendered.contains("scope: inspect config"));
        assert!(rendered.contains("allowed_tools: read"));
        assert!(rendered.contains("merge_verifier: operator_review"));
        assert!(rendered.contains("Find risky config."));
        assert!(rendered.contains("Additional routed context"));
    }
}
