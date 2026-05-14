use cortex_kernel::Journal;
use cortex_types::{CorrelationId, Payload, ToolEffect, TurnId};

use crate::tools::ToolResult;

use super::journal_append;

pub(super) fn record_preview(
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
) {
    if effects.is_empty() {
        return;
    }
    let payload = Payload::ToolEffectPreviewed {
        tool_name: tool_name.to_string(),
        effects: labels(effects),
        preview: preview(tool_name, input, effects),
        rollback: rollback_hint(effects),
    };
    journal_append(journal, turn_id, corr_id, &payload);
    events_log.push(payload);
}

pub(super) fn record_verification(
    journal: &Journal,
    turn_id: TurnId,
    corr_id: CorrelationId,
    events_log: &mut Vec<Payload>,
    tool_name: &str,
    effects: &[ToolEffect],
    result: &ToolResult,
) {
    if effects.is_empty() {
        return;
    }
    let verified = Payload::ToolEffectVerified {
        tool_name: tool_name.to_string(),
        success: !result.is_error,
        verification: verification(result),
    };
    journal_append(journal, turn_id, corr_id, &verified);
    events_log.push(verified);

    if !result.is_error && effects.iter().any(ToolEffect::is_mutating) {
        let committed = Payload::ToolEffectCommitted {
            tool_name: tool_name.to_string(),
            receipt: commit_receipt(tool_name, effects),
        };
        journal_append(journal, turn_id, corr_id, &committed);
        events_log.push(committed);
    }
}

pub(super) fn summary(effects: &[ToolEffect]) -> String {
    if effects.is_empty() {
        "no declared effects".to_string()
    } else {
        labels(effects).join(", ")
    }
}

pub(super) fn preview(
    tool_name: &str,
    input: &serde_json::Value,
    effects: &[ToolEffect],
) -> String {
    let mut preview = format!("tool={tool_name}; effects={}", labels(effects).join(", "));
    if let Some(paths) = target_values(input, effects) {
        preview.push_str("; targets=");
        preview.push_str(&paths.join(", "));
    }
    preview
}

fn labels(effects: &[ToolEffect]) -> Vec<String> {
    effects.iter().map(ToolEffect::label).collect()
}

fn target_values(input: &serde_json::Value, effects: &[ToolEffect]) -> Option<Vec<String>> {
    let values: Vec<String> = effects
        .iter()
        .filter_map(|effect| {
            if effect.target.is_empty() {
                return None;
            }
            input
                .get(&effect.target)
                .and_then(serde_json::Value::as_str)
                .map(|value| format!("{}={}", effect.target, truncate_json_str(value, 160)))
        })
        .collect();
    if values.is_empty() {
        None
    } else {
        Some(values)
    }
}

fn rollback_hint(effects: &[ToolEffect]) -> Option<String> {
    if !effects.iter().any(ToolEffect::is_mutating) {
        return None;
    }
    let irreversible = effects.iter().any(|effect| {
        matches!(
            effect.reversibility,
            cortex_types::EffectReversibility::Irreversible
        )
    });
    if irreversible {
        Some("no automatic rollback is available for at least one declared effect".to_string())
    } else {
        Some("rollback requires the tool-specific prior state or a compensating action".to_string())
    }
}

fn verification(result: &ToolResult) -> String {
    if result.is_error {
        format!(
            "tool returned error; effect is not committed: {}",
            truncate_json_str(&result.output, 240)
        )
    } else {
        format!(
            "tool completed; output captured for audit: {}",
            truncate_json_str(&result.output, 240)
        )
    }
}

fn commit_receipt(tool_name: &str, effects: &[ToolEffect]) -> String {
    format!(
        "tool={tool_name}; committed_effects={}",
        labels(effects).join(", ")
    )
}

fn truncate_json_str(value: &str, max_len: usize) -> String {
    if value.len() <= max_len {
        value.to_string()
    } else {
        let mut end = max_len.saturating_sub(1);
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}...", &value[..end])
    }
}
