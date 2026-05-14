use cortex_types::{
    ControlActionCandidate, ControlDecision, ControlSignal, EffectReversibility, Payload,
    PermissionDecision, RiskLevel, ToolEffect,
};

use super::journal_append;
use super::protected_runtime::protected_runtime_access;
use super::tool_effects;
use super::tpn::TpnLoopContext;

pub(super) struct PermissionEvaluation {
    pub(super) decision: PermissionDecision,
    pub(super) denial_reason: Option<String>,
}

pub(super) fn evaluate_tool_permission(
    ctx: &mut TpnLoopContext<'_>,
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
) -> PermissionEvaluation {
    let risk_level = assess_tool_risk(ctx, tool_name, input, effects);
    let plugin_origin = ctx.tools.plugin_origin(tool_name);
    let protected_access = protected_runtime_access(
        tool_name,
        input,
        effects,
        &ctx.config.protected_runtime_roots,
        plugin_origin.as_deref(),
    );
    let control_decision = permission_control_decision(
        tool_name,
        input,
        effects,
        risk_level,
        ctx.config.risk.auto_approve_up_to,
        ctx.config.execution_scope,
        protected_access.as_deref(),
    );
    let permission_explanation = control_decision.permission_explanation();
    let control_payload = Payload::ControlDecisionRecorded {
        decision: control_decision,
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &control_payload);
    ctx.events_log.push(control_payload);

    let perm_payload = Payload::PermissionRequested {
        tool_name: tool_name.to_string(),
        risk_level: format!("{risk_level:?}"),
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &perm_payload);
    ctx.events_log.push(perm_payload);

    let decision = ctx
        .gate
        .check_with_explanation(tool_name, risk_level, &permission_explanation);
    let denial_reason = match decision {
        PermissionDecision::Approved => None,
        PermissionDecision::Denied => Some(
            protected_access
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| permission_explanation.clone()),
        ),
        PermissionDecision::Pending | PermissionDecision::TimedOut => {
            Some(format!("confirmation required: {permission_explanation}"))
        }
    };
    PermissionEvaluation {
        decision,
        denial_reason,
    }
}

fn assess_tool_risk(
    ctx: &TpnLoopContext<'_>,
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
) -> RiskLevel {
    let plugin_origin = ctx.tools.plugin_origin(tool_name);
    let protected_runtime_blocked = protected_runtime_access(
        tool_name,
        input,
        effects,
        &ctx.config.protected_runtime_roots,
        plugin_origin.as_deref(),
    )
    .is_some();
    let background_blocked = ctx.config.execution_scope == cortex_sdk::ExecutionScope::Background
        && !background_tool_allowed(ctx, tool_name);

    if protected_runtime_blocked || background_blocked {
        RiskLevel::Block
    } else {
        ctx.risk_assessor.assess_level_with_depth_and_effects(
            tool_name,
            input,
            ctx.config.agent_depth,
            effects,
        )
    }
}

fn background_tool_allowed(ctx: &TpnLoopContext<'_>, tool_name: &str) -> bool {
    ctx.risk_assessor.policy_allows_background(tool_name)
        || ctx
            .tools
            .capabilities(tool_name)
            .is_some_and(|capabilities| capabilities.background_safe)
}

fn permission_control_decision(
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
    risk_level: RiskLevel,
    auto_approve_up_to: RiskLevel,
    execution_scope: cortex_sdk::ExecutionScope,
    protected_access: Option<&str>,
) -> ControlDecision {
    let selected = selected_permission_signal(risk_level, auto_approve_up_to);
    let risk_score = risk_level_score(risk_level);
    let reversibility = aggregate_reversibility(effects);
    let preview = tool_effects::preview(tool_name, input, effects);
    let mut decision = ControlDecision::new(
        selected,
        format!("tool '{tool_name}' assessed as {risk_level:?} before execution"),
    )
    .with_scores(0.86, 0.78, risk_score * 0.45, risk_score)
    .with_reversibility(reversibility)
    .with_candidate(
        ControlActionCandidate::new(
            ControlSignal::CallTool,
            format!("execute the tool using the captured invocation; {preview}"),
        )
        .with_scores(0.74, 0.82, risk_score * 0.50, risk_score)
        .with_reversibility(reversibility)
        .with_required_evidence("tool input and effect preview"),
    )
    .with_candidate(
        ControlActionCandidate::new(
            ControlSignal::RequestPermission,
            format!(
                "ask the operator because assessed risk is {risk_level:?} and auto approval stops at {auto_approve_up_to:?}"
            ),
        )
        .with_scores(0.88, 0.68, 0.20, (risk_score * 0.35).max(0.10))
        .with_reversibility(EffectReversibility::Reversible)
        .with_required_evidence("operator approval"),
    )
    .with_candidate(
        ControlActionCandidate::new(
            ControlSignal::Deny,
            "deny the tool and surface a controlled tool error",
        )
        .with_scores(0.80, 0.40, 0.24, 0.05)
        .with_reversibility(EffectReversibility::Reversible),
    )
    .with_required_evidence("tool declaration")
    .with_required_evidence("risk policy evaluation")
    .with_risk_boundary(format!(
        "auto_approve_up_to={:?}; assessed_risk={risk_level:?}; execution_scope={:?}; effects={}",
        auto_approve_up_to,
        execution_scope,
        tool_effects::summary(effects)
    ))
    .with_fallback_plan("deny the tool result if confirmation is denied, cancelled, or unavailable");

    if selected == ControlSignal::RequestPermission {
        decision = decision
            .with_required_evidence("operator approval")
            .with_blocking_uncertainty("operator has not confirmed the side effect yet")
            .with_rejected_alternative(
                ControlSignal::CallTool,
                "assessed risk exceeds the current auto-approval boundary",
            )
            .with_rejected_alternative(ControlSignal::Deny, "risk is not blocked by policy");
    } else if selected == ControlSignal::CallTool {
        decision = decision
            .with_rejected_alternative(
                ControlSignal::RequestPermission,
                "current policy allows this risk level without waiting",
            )
            .with_rejected_alternative(ControlSignal::Deny, "policy did not block the tool");
    } else {
        decision = decision
            .with_blocking_uncertainty(
                protected_access.unwrap_or("policy classified this invocation as blocked"),
            )
            .with_rejected_alternative(
                ControlSignal::CallTool,
                "blocked tools cannot execute in the current policy boundary",
            )
            .with_rejected_alternative(
                ControlSignal::RequestPermission,
                "blocked tools cannot be escalated through normal confirmation",
            );
    }

    decision
}

fn selected_permission_signal(
    risk_level: RiskLevel,
    auto_approve_up_to: RiskLevel,
) -> ControlSignal {
    if matches!(risk_level, RiskLevel::Block) {
        ControlSignal::Deny
    } else if risk_level <= auto_approve_up_to {
        ControlSignal::CallTool
    } else {
        ControlSignal::RequestPermission
    }
}

const fn risk_level_score(risk_level: RiskLevel) -> f32 {
    match risk_level {
        RiskLevel::Allow => 0.10,
        RiskLevel::Review => 0.38,
        RiskLevel::RequireConfirmation => 0.72,
        RiskLevel::Block => 0.96,
    }
}

fn aggregate_reversibility(effects: &[ToolEffect]) -> EffectReversibility {
    if effects
        .iter()
        .any(|effect| effect.reversibility == EffectReversibility::Irreversible)
    {
        EffectReversibility::Irreversible
    } else if effects
        .iter()
        .all(|effect| effect.reversibility == EffectReversibility::Reversible)
    {
        EffectReversibility::Reversible
    } else {
        EffectReversibility::PartiallyReversible
    }
}
