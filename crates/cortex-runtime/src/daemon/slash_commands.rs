use std::fs;
use std::path::Path;

use cortex_types::{ConfirmationResponse, RiskLevel};

use crate::command_registry::{
    CommandInvocation, CommandRegistry, CommandResult, ControlCommand, DefaultCommandRegistry,
};

use super::{CancelTurnError, DaemonState, SlashCommandAction};

fn first_arg(rest: &str) -> Option<&str> {
    rest.split_whitespace().next().filter(|arg| !arg.is_empty())
}

fn slash_args<'a>(input: &'a str, command: &str) -> Option<&'a str> {
    input
        .strip_prefix(command)
        .filter(|rest| rest.is_empty() || rest.starts_with(char::is_whitespace))
        .map(str::trim)
}

fn parse_permission_mode(mode: &str) -> Option<RiskLevel> {
    match mode.trim().to_ascii_lowercase().as_str() {
        "strict" | "allow" => Some(RiskLevel::Allow),
        "balanced" | "review" => Some(RiskLevel::Review),
        "open" | "relaxed" | "requireconfirmation" | "require-confirmation" => {
            Some(RiskLevel::RequireConfirmation)
        }
        _ => None,
    }
}

fn parse_thinking_visibility(value: &str) -> Option<bool> {
    match value.trim().to_ascii_lowercase().as_str() {
        "show" | "shown" | "on" | "true" | "1" | "yes" | "enable" | "enabled" => Some(true),
        "hide" | "hidden" | "off" | "false" | "0" | "no" | "disable" | "disabled" => Some(false),
        _ => None,
    }
}

const fn permission_mode_label(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Allow => "strict",
        RiskLevel::Review => "balanced",
        RiskLevel::RequireConfirmation => "open",
        RiskLevel::Block => "block",
    }
}

fn update_permission_mode_in_config(config_path: &Path, level: RiskLevel) -> Result<(), String> {
    let content = fs::read_to_string(config_path)
        .map_err(|err| format!("cannot read {}: {err}", config_path.display()))?;
    let level_line = format!("auto_approve_up_to = \"{level:?}\"");
    let mut lines = Vec::new();
    let mut in_risk = false;
    let mut replaced = false;
    let mut inserted_inside_risk = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed == "[risk]" {
            in_risk = true;
            lines.push(line.to_string());
            continue;
        }
        if in_risk && trimmed.starts_with('[') {
            if !replaced {
                lines.push(level_line.clone());
                replaced = true;
                inserted_inside_risk = true;
            }
            in_risk = false;
        }
        if in_risk && trimmed.starts_with("auto_approve_up_to") {
            lines.push(level_line.clone());
            replaced = true;
            continue;
        }
        lines.push(line.to_string());
    }

    if in_risk && !replaced {
        lines.push(level_line.clone());
        replaced = true;
        inserted_inside_risk = true;
    }

    if !replaced && !inserted_inside_risk {
        lines.push(String::new());
        lines.push("[risk]".to_string());
        lines.push(level_line);
    }

    cortex_kernel::atomic_write_text(config_path, lines.join("\n"))
        .map_err(|err| format!("cannot write {}: {err}", config_path.display()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SlashInvocation<'a> {
    Control(ControlCommand),
    Skill { name: &'a str, args: &'a str },
    Builtin(crate::command_registry::ParsedCommand<'a>),
    Unknown(crate::command_registry::ParsedCommand<'a>),
}

impl DaemonState {
    pub fn dispatch_command(&self, command: &str) -> String {
        self.dispatch_command_for_session(None, command)
    }

    pub fn dispatch_command_for_session(&self, session_id: Option<&str>, command: &str) -> String {
        match self.resolve_slash_command_for_session(session_id, command) {
            SlashCommandAction::Output(text)
            | SlashCommandAction::Prompt(text)
            | SlashCommandAction::NotFound(text) => text,
        }
    }

    pub(crate) fn resolve_slash_command_for_session(
        &self,
        session_id: Option<&str>,
        command: &str,
    ) -> SlashCommandAction {
        let trimmed = command.trim();
        if let Some(mode) = slash_args(trimmed, "/permission") {
            return SlashCommandAction::Output(self.resolve_permission_mode(mode));
        }
        if let Some(args) = slash_args(trimmed, "/think") {
            return SlashCommandAction::Output(self.resolve_thinking_output(args));
        }
        if let Some(args) = slash_args(trimmed, "/config")
            && let Some(output) = self.resolve_config_mutation(args)
        {
            return SlashCommandAction::Output(output);
        }
        if let Some(id) = trimmed.strip_prefix("/approve").and_then(first_arg) {
            return SlashCommandAction::Output(self.resolve_pending_permission(
                None,
                id,
                ConfirmationResponse::Approved,
            ));
        }
        if let Some(id) = trimmed.strip_prefix("/deny").and_then(first_arg) {
            return SlashCommandAction::Output(self.resolve_pending_permission(
                None,
                id,
                ConfirmationResponse::Denied,
            ));
        }
        self.resolve_registered_slash_command(session_id, trimmed)
    }

    fn resolve_registered_slash_command(
        &self,
        session_id: Option<&str>,
        trimmed: &str,
    ) -> SlashCommandAction {
        let registry = DefaultCommandRegistry::new();
        match self.parse_slash_invocation(&registry, trimmed) {
            SlashInvocation::Control(ControlCommand::Stop) => {
                let actor = session_id
                    .and_then(|sid| self.session_lookup(sid).map(|session| session.owner_actor))
                    .unwrap_or_else(|| "local:default".to_string());
                match self.cancel_turn_for_actor(&actor, session_id) {
                    Ok(target_session) => {
                        tracing::info!(
                            session_id = target_session.as_str(),
                            "Turn cancellation requested via /stop"
                        );
                        return SlashCommandAction::Output("Turn cancellation requested.".into());
                    }
                    Err(CancelTurnError::NoActiveTurn) => {
                        return SlashCommandAction::Output("No active turn to stop.".into());
                    }
                    Err(CancelTurnError::SessionNotFound) => {
                        return SlashCommandAction::Output(
                            "That session is not visible from this actor.".into(),
                        );
                    }
                }
            }
            SlashInvocation::Control(ControlCommand::Status) => {
                return SlashCommandAction::Output(self.format_status_for_session(session_id));
            }
            SlashInvocation::Skill { name, args } => {
                if let Some(content) = self
                    .skill_registry
                    .with_skill(name, |skill| {
                        if !skill.metadata().user_invocable {
                            return None;
                        }
                        let cortex_turn::skills::SkillContent::Markdown(content) =
                            skill.content(args);
                        Some(content)
                    })
                    .flatten()
                {
                    return SlashCommandAction::Prompt(content);
                }
            }
            SlashInvocation::Unknown(parsed) => {
                return SlashCommandAction::NotFound(format!(
                    "Unknown command: {}\nType /help to see available commands",
                    parsed.raw
                ));
            }
            SlashInvocation::Builtin(_) => {}
        }

        let sm = self.session_manager();
        let mut sid = cortex_types::SessionId::new();
        let mut meta = cortex_types::SessionMetadata::new(sid, 0);
        let mut history = Vec::new();
        let mut turn_count = 0;

        let cfg = self.config().clone();
        let providers = self
            .providers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut ctx = crate::command_registry::CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &cfg,
            providers: &providers,
        };

        match registry.dispatch(trimmed, &mut ctx) {
            CommandResult::Output(text) => SlashCommandAction::Output(text),
            CommandResult::Exit => SlashCommandAction::Output("exit".into()),
            CommandResult::NotFound(msg) => SlashCommandAction::NotFound(msg),
        }
    }

    fn resolve_thinking_output(&self, args: &str) -> String {
        let value = args.split_whitespace().next().unwrap_or("");
        if value.is_empty() || value.eq_ignore_ascii_case("status") {
            return self.format_thinking_output_status();
        }
        let Some(show) = parse_thinking_visibility(value) else {
            return "Usage: /think [show|hide|on|off|status]".to_string();
        };
        self.set_thinking_output(show)
    }

    fn resolve_config_mutation(&self, args: &str) -> Option<String> {
        let mut parts = args.split_whitespace();
        if parts.next()? != "set" {
            return None;
        }
        let Some(key) = parts.next() else {
            return Some("Usage: /config set <key> <value>".to_string());
        };
        let Some(value) = parts.next() else {
            return Some("Usage: /config set <key> <value>".to_string());
        };
        Some(self.set_supported_config_key(key, value))
    }

    fn format_thinking_output_status(&self) -> String {
        let show = !self.config().turn.strip_think_tags;
        format!(
            "🧠 Thinking: request={}, output={}",
            if show { "enabled" } else { "disabled" },
            if show { "shown" } else { "hidden" }
        )
    }

    fn set_thinking_output(&self, show: bool) -> String {
        let config_path = cortex_kernel::CortexPaths::from_instance_home(self.home()).config_path();
        let value = if show { "false" } else { "true" };
        if let Err(err) =
            cortex_kernel::update_config_toml_value(&config_path, "turn", "strip_think_tags", value)
        {
            return format!("Failed to update thinking output: {err}");
        }
        crate::hot_reload::ReloadTarget::reload_config(self);
        format!(
            "🧠 Thinking request {}; output {}.",
            if show { "enabled" } else { "disabled" },
            if show { "shown" } else { "hidden" }
        )
    }

    fn set_supported_config_key(&self, key: &str, value: &str) -> String {
        match key {
            "turn.show_thinking" | "show_thinking" => {
                let Some(show) = cortex_kernel::parse_bool_like(value) else {
                    return "Invalid boolean. Use true/false, on/off, show/hide.".to_string();
                };
                self.set_thinking_output(show)
            }
            "turn.strip_think_tags" | "strip_think_tags" => {
                let Some(strip) = cortex_kernel::parse_bool_like(value) else {
                    return "Invalid boolean. Use true/false, on/off, show/hide.".to_string();
                };
                self.set_thinking_output(!strip)
            }
            "embedding.api_key" => {
                let config_path =
                    cortex_kernel::CortexPaths::from_instance_home(self.home()).config_path();
                let literal = match serde_json::to_string(value) {
                    Ok(literal) => literal,
                    Err(err) => return format!("Failed to encode embedding API key: {err}"),
                };
                if let Err(err) = cortex_kernel::update_config_toml_value(
                    &config_path,
                    "embedding",
                    "api_key",
                    &literal,
                ) {
                    return format!("Failed to update embedding API key: {err}");
                }
                crate::hot_reload::ReloadTarget::reload_config(self);
                "Embedding API key updated.".to_string()
            }
            _ => format!(
                "Unsupported config key: {key}\nSupported: turn.show_thinking, turn.strip_think_tags, embedding.api_key"
            ),
        }
    }

    fn resolve_permission_mode(&self, mode: &str) -> String {
        let Some(level) = parse_permission_mode(mode) else {
            if mode.is_empty() {
                let current = self.config().risk.auto_approve_up_to;
                return format!(
                    "🛡️ Permission mode: {} (auto-approve up to {current:?})",
                    permission_mode_label(current)
                );
            }
            return "Usage: /permission [strict|balanced|open]".to_string();
        };

        let current = self.config().risk.auto_approve_up_to;
        if level == current {
            return format!(
                "🛡️ Permission mode remains {} (auto-approve up to {current:?}).",
                permission_mode_label(current)
            );
        }

        let config_path = cortex_kernel::CortexPaths::from_instance_home(self.home()).config_path();
        if let Err(err) = update_permission_mode_in_config(&config_path, level) {
            return format!("Failed to update permission mode: {err}");
        }

        crate::hot_reload::ReloadTarget::reload_config(self);
        format!(
            "🛡️ Permission mode set to {} (auto-approve up to {level:?}).",
            permission_mode_label(level)
        )
    }

    fn parse_slash_invocation<'a>(
        &self,
        registry: &DefaultCommandRegistry,
        command: &'a str,
    ) -> SlashInvocation<'a> {
        let invocation = registry.classify(command);
        match invocation {
            CommandInvocation::Control(command) => SlashInvocation::Control(command),
            CommandInvocation::Builtin(parsed) => SlashInvocation::Builtin(parsed),
            CommandInvocation::Unknown(parsed) => {
                let name = parsed
                    .raw
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .trim_start_matches('/');
                if name.is_empty() {
                    return SlashInvocation::Unknown(parsed);
                }
                if self.skill_registry.with_skill(name, |_| ()).is_some() {
                    SlashInvocation::Skill {
                        name,
                        args: parsed.args,
                    }
                } else {
                    SlashInvocation::Unknown(parsed)
                }
            }
        }
    }
}
