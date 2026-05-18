#[derive(Clone, Copy)]
pub(super) enum QqPermissionCallbackAction<'a> {
    Approve(&'a str),
    Deny(&'a str),
    Refresh(&'a str),
}

pub(super) fn parse_qq_permission_callback(data: &str) -> Option<QqPermissionCallbackAction<'_>> {
    let mut parts = data.splitn(3, ':');
    let prefix = parts.next()?;
    let action = parts.next()?;
    let id = parts.next()?;
    if prefix != "perm" || id.is_empty() {
        return None;
    }
    match action {
        "approve" => Some(QqPermissionCallbackAction::Approve(id)),
        "deny" => Some(QqPermissionCallbackAction::Deny(id)),
        "refresh" => Some(QqPermissionCallbackAction::Refresh(id)),
        _ => None,
    }
}

fn qq_button(label: &str, visited_label: &str, data: &str, style: i64) -> serde_json::Value {
    serde_json::json!({
        "id": data,
        "render_data": {
            "label": label,
            "visited_label": visited_label,
            "style": style,
        },
        "action": {
            "type": 1,
            "data": data,
            "permission": { "type": 2 },
            "click_limit": 1,
        },
    })
}

pub(super) fn qq_command_keyboard(
    cmd: &str,
    current_mode: cortex_types::RiskLevel,
    show_thinking: bool,
) -> Option<serde_json::Value> {
    match cmd {
        "/help" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Status", cmd, "/status"), "Status", "/status", 1),
                        qq_button(&qq_nav_button_label("Permission", cmd, "/permission"), "Permission", "/permission", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Sessions", cmd, "/session"), "Sessions", "/session", 1),
                        qq_button(&qq_nav_button_label("Config", cmd, "/config"), "Config", "/config", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Thinking", cmd, "/think"), "Thinking", "/think", 1),
                        qq_button("Stop", "Stopping", "/stop", 0),
                    ]},
                ]
            }
        })),
        "/status" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("Refresh", "Refreshed", "/status", 1),
                        qq_button(&qq_nav_button_label("Permission", cmd, "/permission"), "Permission", "/permission", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Sessions", cmd, "/session"), "Sessions", "/session", 1),
                        qq_button(&qq_nav_button_label("Config", cmd, "/config"), "Config", "/config", 1),
                    ]},
                    {"buttons": [
                        qq_button(&qq_nav_button_label("Thinking", cmd, "/think"), "Thinking", "/think", 1),
                    ]},
                ]
            }
        })),
        "/permission" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button(&qq_permission_button_label("Strict", current_mode, cortex_types::RiskLevel::Allow), "Strict", "/permission strict", qq_permission_button_style(current_mode, cortex_types::RiskLevel::Allow)),
                        qq_button(&qq_permission_button_label("Balanced", current_mode, cortex_types::RiskLevel::Review), "Balanced", "/permission balanced", qq_permission_button_style(current_mode, cortex_types::RiskLevel::Review)),
                        qq_button(&qq_permission_button_label("Open", current_mode, cortex_types::RiskLevel::RequireConfirmation), "Open", "/permission open", qq_permission_button_style(current_mode, cortex_types::RiskLevel::RequireConfirmation)),
                    ]},
                    {"buttons": [
                        qq_button("Refresh", "Refreshed", "/permission", 1),
                        qq_button(&qq_nav_button_label("Status", cmd, "/status"), "Status", "/status", 1),
                    ]},
                ]
            }
        })),
        "/think" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button(&qq_thinking_button_label("Show", show_thinking, true), "Show", "/think show", i64::from(show_thinking)),
                        qq_button(&qq_thinking_button_label("Hide", show_thinking, false), "Hide", "/think hide", i64::from(!show_thinking)),
                    ]},
                    {"buttons": [
                        qq_button("Status", "Status", "/think status", 1),
                        qq_button(&qq_nav_button_label("Config", cmd, "/config"), "Config", "/config", 1),
                    ]},
                ]
            }
        })),
        "/session" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("List", "Listed", "/session list", 1),
                        qq_button("New", "Created", "/session new", 1),
                    ]},
                    {"buttons": [
                        qq_button("Switch", "Switch", "/session switch", 1),
                        qq_button("End", "Ended", "/quit", 0),
                    ]},
                ]
            }
        })),
        "/config" => Some(serde_json::json!({
            "content": {
                "rows": [
                    {"buttons": [
                        qq_button("API", "API", "/config get api", 1),
                        qq_button("Memory", "Memory", "/config get memory", 1),
                        qq_button("Tools", "Tools", "/config get tools", 1),
                    ]},
                    {"buttons": [
                        qq_button("Web", "Web", "/config get web", 1),
                        qq_button("Skills", "Skills", "/config get skills", 1),
                        qq_button("Summary", "Summary", "/config list", 1),
                    ]},
                ]
            }
        })),
        _ => None,
    }
}

pub(super) fn qq_root_keyboard_for_callback(
    data: &str,
    current_mode: cortex_types::RiskLevel,
    show_thinking: bool,
) -> Option<serde_json::Value> {
    if data.starts_with("/help") || data.starts_with("/stop") {
        qq_command_keyboard("/help", current_mode, show_thinking)
    } else if data.starts_with("/status") {
        qq_command_keyboard("/status", current_mode, show_thinking)
    } else if data.starts_with("/permission") {
        qq_command_keyboard("/permission", current_mode, show_thinking)
    } else if data.starts_with("/think") {
        qq_command_keyboard("/think", current_mode, show_thinking)
    } else if data.starts_with("/session") || data == "/quit" {
        qq_command_keyboard("/session", current_mode, show_thinking)
    } else if data.starts_with("/config") {
        qq_command_keyboard("/config", current_mode, show_thinking)
    } else {
        None
    }
}

fn qq_permission_button_label(
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

fn qq_permission_button_style(
    current_mode: cortex_types::RiskLevel,
    button_mode: cortex_types::RiskLevel,
) -> i64 {
    i64::from(current_mode == button_mode)
}

fn qq_nav_button_label(label: &str, current_cmd: &str, button_cmd: &str) -> String {
    if current_cmd == button_cmd {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

fn qq_thinking_button_label(label: &str, show_thinking: bool, button_show: bool) -> String {
    if show_thinking == button_show {
        format!("• {label}")
    } else {
        label.to_string()
    }
}

pub(super) fn qq_session_switch_keyboard(
    sessions: &[cortex_types::SessionMetadata],
    current_session_id: Option<&str>,
) -> Option<serde_json::Value> {
    let mut rows = Vec::new();
    for session in sessions
        .iter()
        .filter(|session| {
            current_session_id.is_none_or(|current| session.id.to_string() != current)
        })
        .take(10)
    {
        let id = session.id.to_string();
        let short_id = &id[..id.len().min(8)];
        let label = session.name.as_deref().unwrap_or(short_id);
        rows.push(serde_json::json!({
            "buttons": [
                qq_button(
                    &format!("{label} ({})", session.turn_count),
                    label,
                    &format!("/session switch {id}"),
                    1,
                )
            ]
        }));
    }
    if rows.is_empty() {
        return None;
    }
    rows.push(serde_json::json!({
        "buttons": [qq_button("Back", "Back", "/session", 1)]
    }));
    Some(serde_json::json!({ "content": { "rows": rows } }))
}

pub(super) fn qq_permission_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "content": {
            "rows": [
                {"buttons": [
                    qq_button("Approve", "Approved", &format!("perm:approve:{id}"), 1),
                    qq_button("Deny", "Denied", &format!("perm:deny:{id}"), 0),
                    qq_button("Refresh", "Refreshed", &format!("perm:refresh:{id}"), 1),
                ]}
            ]
        }
    })
}

pub(super) fn qq_permission_resolved_keyboard(id: &str) -> serde_json::Value {
    serde_json::json!({
        "content": {
            "rows": [
                {"buttons": [
                    qq_button("Refresh", "Refreshed", &format!("perm:refresh:{id}"), 1),
                ]}
            ]
        }
    })
}

pub(super) fn qq_permission_resolved_text(id: &str) -> String {
    format!("✅ Permission request {id} has already been resolved.")
}

pub(super) fn qq_permission_delivery(
    event: &crate::daemon::BroadcastEvent,
) -> Option<(String, serde_json::Value)> {
    match event {
        crate::daemon::BroadcastEvent::PermissionRequested(info) => {
            Some((info.prompt_text(), qq_permission_keyboard(&info.id)))
        }
        crate::daemon::BroadcastEvent::Observer { source, content } if source == "permission" => {
            parse_permission_prompt_id(content)
                .map(|id| (content.clone(), qq_permission_keyboard(id)))
        }
        _ => None,
    }
}

fn parse_permission_prompt_id(prompt: &str) -> Option<&str> {
    prompt
        .lines()
        .find_map(|line| line.strip_prefix("Approve: /approve "))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}
