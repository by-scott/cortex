use crate::agent_pool::delegation::{DelegationContract, DelegationContractError};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum AgentSubTurnMode {
    Readonly,
    Fork,
    Teammate,
    Full,
}

impl AgentSubTurnMode {
    pub(super) fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("readonly") {
            "fork" => Self::Fork,
            "teammate" => Self::Teammate,
            "full" => Self::Full,
            _ => Self::Readonly,
        }
    }
}

impl std::fmt::Display for AgentSubTurnMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Readonly => write!(f, "readonly"),
            Self::Fork => write!(f, "fork"),
            Self::Teammate => write!(f, "teammate"),
            Self::Full => write!(f, "full"),
        }
    }
}

pub(super) fn agent_delegation_contract(
    input: &serde_json::Value,
    description: &str,
) -> Result<DelegationContract, DelegationContractError> {
    let scope = input
        .get("scope")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or(description);
    let expected_artifact = input
        .get("expected_artifact")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("worker answer");
    let merge_verifier = input
        .get("merge_verifier")
        .and_then(serde_json::Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("parent_review");

    let mut contract = DelegationContract::new(scope, expected_artifact)
        .with_token_budget(usize_field(
            input,
            "token_budget",
            DelegationContract::DEFAULT_TOKEN_BUDGET,
        ))
        .with_iteration_budget(usize_field(
            input,
            "iteration_budget",
            DelegationContract::DEFAULT_ITERATION_BUDGET,
        ))
        .with_evidence_budget(usize_field(input, "evidence_budget", 0))
        .with_merge_verifier(merge_verifier)
        .with_review_required(bool_field(input, "review_required", true))
        .with_parent_authority_inheritance(bool_field(input, "inherit_parent_authority", false));

    for tool in string_array_field(input, "allowed_tools") {
        contract = contract.with_allowed_tool(tool);
    }
    for action in string_array_field(input, "forbidden_actions") {
        contract = contract.with_forbidden_action(action);
    }
    for evidence in string_array_field(input, "allowed_evidence") {
        contract = contract.with_allowed_evidence(evidence);
    }

    contract.validate()?;
    Ok(contract)
}

fn string_array_field(input: &serde_json::Value, field: &str) -> Vec<String> {
    input
        .get(field)
        .and_then(serde_json::Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn usize_field(input: &serde_json::Value, field: &str, default: usize) -> usize {
    input
        .get(field)
        .and_then(serde_json::Value::as_u64)
        .and_then(|value| usize::try_from(value).ok())
        .unwrap_or(default)
}

fn bool_field(input: &serde_json::Value, field: &str, default: bool) -> bool {
    input
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(default)
}
