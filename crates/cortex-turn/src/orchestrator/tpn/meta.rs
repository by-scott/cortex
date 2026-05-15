use std::fmt::Write as _;

use cortex_types::{Message, Payload};

use super::TpnLoopContext;
use crate::context::pressure::{PressureLevel, compute_occupancy, estimate_tokens};
use crate::orchestrator::dmn::{PressureContext, apply_compress_history};
use crate::orchestrator::{TraceCategory, journal_append};

/// Returns `Some(early_exit_text)` when the loop should terminate early.
pub(super) async fn post_tool_iteration(
    ctx: &mut TpnLoopContext<'_>,
    _response: &crate::llm::LlmResponse,
    _tool_iteration: usize,
    meta_hint: &mut Option<String>,
) -> Option<Option<String>> {
    let used: usize = ctx
        .history
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum();
    let pressure = PressureLevel::from_occupancy(
        compute_occupancy(used, ctx.config.max_tokens),
        &ctx.config.pressure_thresholds,
    );
    if pressure >= PressureLevel::Compress {
        apply_compress_history(&mut PressureContext {
            history: ctx.history,
            llm: ctx.llm,
            journal: ctx.journal,
            turn_id: ctx.turn_id,
            corr_id: ctx.corr_id,
            events_log: ctx.events_log,
            working_mem: ctx.working_mem,
            compress_template: ctx.compress_template,
            summary_cache: ctx.summary_cache,
            max_tokens: ctx.config.max_tokens,
            pressure_thresholds: ctx.config.pressure_thresholds,
        })
        .await;
    }

    apply_metacognition_alerts(ctx, meta_hint);
    apply_exploration_hint(ctx, meta_hint);
    apply_conditional_skills(ctx, meta_hint);
    None
}

/// Handle metacognition alerts after tool execution.
///
/// Check metacognition alerts and apply appropriate responses.
fn apply_metacognition_alerts(ctx: &mut TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let alerts = ctx
        .meta_monitor
        .check_with_confidence(ctx.confidence.score());
    for alert in &alerts {
        ctx.tracer.trace_at(
            TraceCategory::Meta,
            cortex_types::TraceLevel::Basic,
            &format!("Alert: {:?}", alert.kind),
        );
        let action: &'static str = match alert.kind {
            crate::meta::AlertKind::DoomLoop => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-doom-loop"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_DOOM_LOOP.to_string()
                        }),
                );
                "doom_loop_strategy_switch"
            }
            crate::meta::AlertKind::Duration => {
                let payload = Payload::MetaControlApplied {
                    action: "duration_warning".into(),
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
                ctx.events_log.push(payload);
                "duration_warning"
            }
            crate::meta::AlertKind::Fatigue => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-fatigue"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_FATIGUE.to_string()
                        }),
                );
                "fatigue_step_break"
            }
            crate::meta::AlertKind::FrameAnchoring => {
                *meta_hint = Some(
                    ctx.prompt_manager
                        .and_then(|pm| pm.get_system_template("hint-frame-anchoring"))
                        .unwrap_or_else(|| {
                            cortex_kernel::prompt_manager::DEFAULT_HINT_FRAME_ANCHORING.to_string()
                        }),
                );
                "frame_anchoring_recheck"
            }
            crate::meta::AlertKind::HealthDegraded => "health_degraded_noted",
        };
        let payload = Payload::MetaControlApplied {
            action: action.into(),
        };
        journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &payload);
        ctx.events_log.push(payload);
    }

    if let Some(registry) = ctx.skill_registry {
        let alert_names: Vec<String> = alerts.iter().map(|a| format!("{:?}", a.kind)).collect();
        if !alert_names.is_empty() {
            let activated = registry.activated_skills("", "normal", &alert_names);
            for summary in activated {
                let already = meta_hint
                    .as_ref()
                    .is_some_and(|h| h.contains(&summary.name));
                if already {
                    continue;
                }
                if let Some(content) = registry.render(&summary.name, "").map(|rendered| {
                    let crate::skills::SkillContent::Markdown(content) = rendered.content;
                    content
                }) {
                    let skill_section = format!("\n[Skill: {}]\n{}", summary.name, content);
                    match meta_hint {
                        Some(existing) => existing.push_str(&skill_section),
                        None => *meta_hint = Some(skill_section),
                    }
                }
                let ev = Payload::SkillInvoked {
                    name: summary.name.clone(),
                    trigger: cortex_types::InvocationTrigger::MetacognitiveAlert(
                        alert_names.join(","),
                    )
                    .to_string(),
                    execution_mode: "inline".to_string(),
                };
                journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
                ctx.events_log.push(ev);
            }
        }
    }
}

/// Check RPE exploration candidates and inject hint when uncertainty is high.
///
/// Emits `ExplorationTriggered` for the top candidate and, if no urgent
/// metacognition hint is active, injects a suggestion into the request frame.
fn apply_exploration_hint(ctx: &mut TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let candidates = ctx.meta_monitor.rpe.exploration_candidates();
    if candidates.is_empty() {
        return;
    }

    let (top_name, top_bonus) = &candidates[0];
    let ev = Payload::ExplorationTriggered {
        tool_name: top_name.clone(),
        bonus: *top_bonus,
    };
    journal_append(ctx.journal, ctx.turn_id, ctx.corr_id, &ev);
    ctx.events_log.push(ev);

    if meta_hint.is_some() {
        return;
    }

    let template = ctx
        .prompt_manager
        .and_then(|pm| pm.get_system_template("hint-exploration"))
        .unwrap_or_else(|| cortex_kernel::prompt_manager::DEFAULT_HINT_EXPLORATION.to_string());

    let display: Vec<String> = candidates
        .iter()
        .take(3)
        .map(|(name, bonus)| format!("'{name}' (uncertainty bonus={bonus:.2})"))
        .collect();
    let hint = template.replace("__CANDIDATES__", &display.join(", "));
    *meta_hint = Some(hint);
}

fn apply_conditional_skills(ctx: &TpnLoopContext<'_>, meta_hint: &mut Option<String>) {
    let Some(registry) = ctx.skill_registry else {
        return;
    };
    let input = ctx
        .history
        .last()
        .map(Message::text_content)
        .unwrap_or_default();
    let used: usize = ctx
        .history
        .iter()
        .map(|m| estimate_tokens(&m.text_content()))
        .sum();
    let occupancy = compute_occupancy(used, ctx.config.max_tokens);
    let pressure = PressureLevel::from_occupancy(occupancy, &ctx.config.pressure_thresholds);
    let pressure_name = pressure.name();
    let alerts = ctx
        .meta_monitor
        .check_with_confidence(ctx.confidence.score());
    let alert_names: Vec<String> = alerts.iter().map(|a| format!("{:?}", a.kind)).collect();

    let activated = registry.activated_skills(&input, pressure_name, &alert_names);
    if activated.is_empty() {
        return;
    }
    let mut skill_text = String::from("[Auto-activated skills]\n");
    for summary in &activated {
        if let Some(content) = registry.render(&summary.name, "").map(|rendered| {
            let crate::skills::SkillContent::Markdown(content) = rendered.content;
            content
        }) {
            let _ = writeln!(skill_text, "\n## {}\n{}", summary.name, content);
        }
    }
    match meta_hint {
        Some(existing) => {
            existing.push('\n');
            existing.push_str(&skill_text);
        }
        None => *meta_hint = Some(skill_text),
    }
}
