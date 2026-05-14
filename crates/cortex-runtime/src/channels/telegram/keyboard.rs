pub(super) fn telegram_builtin_bot_commands() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"command": "help", "description": "Show available commands"}),
        serde_json::json!({"command": "status", "description": "Runtime status"}),
        serde_json::json!({"command": "permission", "description": "Permission mode"}),
        serde_json::json!({"command": "think", "description": "Thinking output"}),
        serde_json::json!({"command": "stop", "description": "Cancel running turn"}),
        serde_json::json!({"command": "session", "description": "Session management"}),
        serde_json::json!({"command": "config", "description": "View configuration"}),
        serde_json::json!({"command": "quit", "description": "End current session"}),
        serde_json::json!({"command": "exit", "description": "End current session"}),
    ]
}

/// Return an inline keyboard for bare commands that benefit from buttons.
pub(super) fn command_keyboard(
    cmd: &str,
    current_mode: cortex_types::RiskLevel,
    show_thinking: bool,
) -> Option<serde_json::Value> {
    match cmd {
        "/help" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": nav_button_label("Status", cmd, "/status"), "callback_data": "/status"},
                {"text": nav_button_label("Permission", cmd, "/permission"), "callback_data": "/permission"},
            ],[
                {"text": nav_button_label("Sessions", cmd, "/session"), "callback_data": "/session"},
                {"text": nav_button_label("Config", cmd, "/config"), "callback_data": "/config"},
            ],[
                {"text": nav_button_label("Thinking", cmd, "/think"), "callback_data": "/think"},
                {"text": "Stop", "callback_data": "/stop"},
            ]]
        })),
        "/status" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "Refresh", "callback_data": "/status"},
                {"text": nav_button_label("Permission", cmd, "/permission"), "callback_data": "/permission"},
            ],[
                {"text": nav_button_label("Sessions", cmd, "/session"), "callback_data": "/session"},
                {"text": nav_button_label("Config", cmd, "/config"), "callback_data": "/config"},
            ],[
                {"text": nav_button_label("Thinking", cmd, "/think"), "callback_data": "/think"},
            ]]
        })),
        "/permission" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": permission_button_label("Strict", current_mode, cortex_types::RiskLevel::Allow), "callback_data": "/permission strict"},
                {"text": permission_button_label("Balanced", current_mode, cortex_types::RiskLevel::Review), "callback_data": "/permission balanced"},
                {"text": permission_button_label("Open", current_mode, cortex_types::RiskLevel::RequireConfirmation), "callback_data": "/permission open"},
            ],[
                {"text": "Refresh", "callback_data": "/permission"},
                {"text": nav_button_label("Status", cmd, "/status"), "callback_data": "/status"},
            ]]
        })),
        "/think" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": thinking_button_label("Show", show_thinking, true), "callback_data": "/think show"},
                {"text": thinking_button_label("Hide", show_thinking, false), "callback_data": "/think hide"},
            ],[
                {"text": "Status", "callback_data": "/think status"},
                {"text": nav_button_label("Config", cmd, "/config"), "callback_data": "/config"},
            ]]
        })),
        "/session" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "List", "callback_data": "/session list"},
                {"text": "New", "callback_data": "/session new"},
            ],[
                {"text": "Switch", "callback_data": "/session switch"},
                {"text": "End", "callback_data": "/quit"},
            ]]
        })),
        "/config" => Some(serde_json::json!({
            "inline_keyboard": [[
                {"text": "API", "callback_data": "/config get api"},
                {"text": "Memory", "callback_data": "/config get memory"},
                {"text": "Tools", "callback_data": "/config get tools"},
            ],[
                {"text": "Web", "callback_data": "/config get web"},
                {"text": "Skills", "callback_data": "/config get skills"},
                {"text": "Summary", "callback_data": "/config list"},
            ]]
        })),
        _ => None,
    }
}

pub(super) fn root_command_keyboard_for_callback(
    data: &str,
    current_mode: cortex_types::RiskLevel,
    show_thinking: bool,
) -> Option<serde_json::Value> {
    if data.starts_with("/help") || data.starts_with("/stop") {
        command_keyboard("/help", current_mode, show_thinking)
    } else if data.starts_with("/status") {
        command_keyboard("/status", current_mode, show_thinking)
    } else if data.starts_with("/permission") {
        command_keyboard("/permission", current_mode, show_thinking)
    } else if data.starts_with("/think") {
        command_keyboard("/think", current_mode, show_thinking)
    } else if data.starts_with("/session") || data == "/quit" {
        command_keyboard("/session", current_mode, show_thinking)
    } else if data.starts_with("/config") {
        command_keyboard("/config", current_mode, show_thinking)
    } else {
        None
    }
}

fn permission_button_label(
    label: &str,
    current_mode: cortex_types::RiskLevel,
    button_mode: cortex_types::RiskLevel,
) -> String {
    if current_mode == button_mode {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn nav_button_label(label: &str, current_cmd: &str, button_cmd: &str) -> String {
    if current_cmd == button_cmd {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn thinking_button_label(label: &str, show_thinking: bool, button_show: bool) -> String {
    if show_thinking == button_show {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

pub(super) fn session_switch_keyboard(
    sessions: &[cortex_types::SessionMetadata],
    current_session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut buttons: Vec<Vec<serde_json::Value>> = sessions
        .iter()
        .filter(|session| {
            current_session_id.is_none_or(|current| session.id.to_string() != current)
        })
        .take(10)
        .map(|session| {
            let id = session.id.to_string();
            let short_id = &id[..id.len().min(8)];
            let label = session.name.as_deref().unwrap_or(short_id);
            vec![serde_json::json!({
                "text": format!("{label}  (turns: {})", session.turn_count),
                "callback_data": format!("/session switch {id}"),
            })]
        })
        .collect();

    if buttons.is_empty() {
        return None;
    }
    buttons.push(vec![serde_json::json!({
        "text": "Back",
        "callback_data": "/session",
    })]);
    Some(serde_json::json!({ "inline_keyboard": buttons }))
}

#[derive(Clone, Copy)]
pub(super) enum PermissionCallbackAction<'a> {
    Approve(&'a str),
    Deny(&'a str),
    Refresh(&'a str),
}

pub(super) fn parse_permission_callback(data: &str) -> Option<PermissionCallbackAction<'_>> {
    let mut parts = data.splitn(3, ':');
    let prefix = parts.next()?;
    let action = parts.next()?;
    let id = parts.next()?;
    if prefix != "perm" || id.is_empty() {
        return None;
    }
    match action {
        "approve" => Some(PermissionCallbackAction::Approve(id)),
        "deny" => Some(PermissionCallbackAction::Deny(id)),
        "refresh" => Some(PermissionCallbackAction::Refresh(id)),
        _ => None,
    }
}

pub(super) fn permission_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[
            {"text": "Approve", "callback_data": format!("perm:approve:{id}")},
            {"text": "Deny", "callback_data": format!("perm:deny:{id}")},
        ],[
            {"text": "Refresh", "callback_data": format!("perm:refresh:{id}")},
        ]]
    })
}

pub(super) fn permission_resolved_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": [[
            {"text": "Refresh", "callback_data": format!("perm:refresh:{id}")},
        ]]
    })
}

pub(super) fn parse_permission_prompt_id(prompt: &str) -> Option<&str> {
    for line in prompt.lines() {
        if let Some(id) = line.strip_prefix("Approve: /approve ") {
            let trimmed = id.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

pub(super) fn permission_resolved_text(id: &str) -> String {
    format!("Permission request {id} is already resolved.")
}
