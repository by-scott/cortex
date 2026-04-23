use std::fmt::Write as _;

use cortex_kernel::{format_config_section, format_config_summary};
use cortex_types::config::{CortexConfig, ProviderRegistry};

use crate::session_manager::SessionManager;

/// Result of dispatching a command.
#[derive(Debug)]
pub enum CommandResult {
    /// Command produced output text to display.
    Output(String),
    /// Command signals the caller to exit.
    Exit,
    /// The command was not recognized.
    NotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandScope {
    Control,
    Session,
    Config,
    Lifecycle,
    Help,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlCommand {
    Stop,
    Status,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionCommand<'a> {
    List,
    New,
    Switch { target: &'a str },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigCommand<'a> {
    List,
    Get { section: &'a str },
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandKind<'a> {
    Control(ControlCommand),
    Session(SessionCommand<'a>),
    Config(ConfigCommand<'a>),
    Lifecycle,
    Help,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCommand<'a> {
    pub kind: CommandKind<'a>,
    pub scope: CommandScope,
    pub raw: &'a str,
    pub args: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandInvocation<'a> {
    Control(ControlCommand),
    Builtin(ParsedCommand<'a>),
    Unknown(ParsedCommand<'a>),
}

/// Trait for dispatching slash commands.
pub trait CommandRegistry {
    /// Dispatch a command string (e.g. "/session list") and return the result.
    fn dispatch(&self, input: &str, ctx: &mut CommandContext<'_>) -> CommandResult;

    /// Parse a command string into a higher-level scope classification.
    fn parse<'a>(&self, input: &'a str) -> ParsedCommand<'a>;

    /// List all registered command names.
    fn list_commands(&self) -> Vec<&'static str>;
}

/// Mutable context passed into command handlers.
///
/// Contains session state that commands may need to read or modify.
pub struct CommandContext<'a> {
    pub session_manager: &'a SessionManager<'a>,
    pub session_meta: &'a mut cortex_types::SessionMetadata,
    pub session_id: &'a mut cortex_types::SessionId,
    pub history: &'a mut Vec<cortex_types::Message>,
    pub turn_count: &'a mut usize,
    pub config: &'a CortexConfig,
    pub providers: &'a ProviderRegistry,
}

/// Default command registry with built-in /session, /config, /quit, /exit.
pub struct DefaultCommandRegistry;

impl DefaultCommandRegistry {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl Default for DefaultCommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultCommandRegistry {
    #[must_use]
    pub fn classify<'a>(&self, input: &'a str) -> CommandInvocation<'a> {
        let parsed = self.parse(input);
        match parsed.kind {
            CommandKind::Control(command) => CommandInvocation::Control(command),
            CommandKind::Session(_)
            | CommandKind::Config(_)
            | CommandKind::Lifecycle
            | CommandKind::Help => CommandInvocation::Builtin(parsed),
            CommandKind::Unknown => CommandInvocation::Unknown(parsed),
        }
    }
}

impl CommandRegistry for DefaultCommandRegistry {
    fn dispatch(&self, input: &str, ctx: &mut CommandContext<'_>) -> CommandResult {
        let parsed = self.parse(input);
        match parsed.kind {
            CommandKind::Lifecycle => CommandResult::Exit,
            CommandKind::Help => CommandResult::Output(help_text()),
            CommandKind::Session(ref command) => dispatch_session(command, ctx),
            CommandKind::Config(ref command) => dispatch_config(command, ctx.config, ctx.providers),
            CommandKind::Control(_) | CommandKind::Unknown => CommandResult::NotFound(format!(
                "Unknown command: {}\nType /help to see available commands",
                parsed.raw
            )),
        }
    }

    fn parse<'a>(&self, input: &'a str) -> ParsedCommand<'a> {
        let trimmed = input.trim();
        let mut parts = trimmed.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("");
        let args = parts.next().unwrap_or("").trim();
        let kind = match name {
            "/stop" => CommandKind::Control(ControlCommand::Stop),
            "/status" => CommandKind::Control(ControlCommand::Status),
            "/session" => CommandKind::Session(parse_session_command(args)),
            "/config" => CommandKind::Config(parse_config_command(args)),
            "/quit" | "/exit" => CommandKind::Lifecycle,
            "/help" => CommandKind::Help,
            _ => CommandKind::Unknown,
        };
        let scope = match kind {
            CommandKind::Control(_) => CommandScope::Control,
            CommandKind::Session(_) => CommandScope::Session,
            CommandKind::Config(_) => CommandScope::Config,
            CommandKind::Lifecycle => CommandScope::Lifecycle,
            CommandKind::Help => CommandScope::Help,
            CommandKind::Unknown => CommandScope::Unknown,
        };
        ParsedCommand {
            kind,
            scope,
            raw: trimmed,
            args,
        }
    }

    fn list_commands(&self) -> Vec<&'static str> {
        vec![
            "/help", "/status", "/stop", "/session", "/config", "/quit", "/exit",
        ]
    }
}

fn help_text() -> String {
    "Commands:\n  \
     /help                    Show this help\n  \
     /status                  Show runtime status and token usage\n  \
     /stop                    Cancel the running turn\n  \
     /session list            List all sessions\n  \
     /session new             Create a new session\n  \
     /session switch <id>     Switch to a previous session\n  \
     /config [section]        View configuration\n  \
     /quit                    Quit"
        .into()
}

fn parse_session_command(args: &str) -> SessionCommand<'_> {
    let trimmed = args.trim();
    match trimmed.split_whitespace().next() {
        Some("list") => SessionCommand::List,
        Some("new") => SessionCommand::New,
        Some("switch") => trimmed
            .strip_prefix("switch")
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map_or(SessionCommand::Invalid, |target| SessionCommand::Switch {
                target,
            }),
        Some("resume") => trimmed
            .strip_prefix("resume")
            .map(str::trim)
            .filter(|target| !target.is_empty())
            .map_or(SessionCommand::Invalid, |target| SessionCommand::Switch {
                target,
            }),
        _ => SessionCommand::Invalid,
    }
}

fn dispatch_session(command: &SessionCommand<'_>, ctx: &mut CommandContext<'_>) -> CommandResult {
    match command {
        SessionCommand::List => {
            let sessions = ctx.session_manager.list_sessions();
            if sessions.is_empty() {
                return CommandResult::Output("No saved sessions.".into());
            }
            let mut out = format!("{:<14} {:<20} {:<24} Turns\n", "ID", "Name", "Created");
            for s in &sessions {
                let id_str = s.id.to_string();
                let id_short = &id_str[..id_str.len().min(12)];
                let name = s.name.as_deref().unwrap_or("-");
                let created = s.created_at.format("%Y-%m-%d %H:%M:%S");
                let _ = writeln!(
                    out,
                    "{id_short:<14} {name:<20} {created:<24} {}",
                    s.turn_count
                );
            }
            CommandResult::Output(out)
        }
        SessionCommand::New => {
            ctx.session_manager
                .end_session(ctx.session_meta, *ctx.turn_count);
            ctx.history.clear();
            let (new_id, new_meta) = ctx.session_manager.create_session();
            *ctx.session_id = new_id;
            *ctx.session_meta = new_meta;
            *ctx.turn_count = 0;
            CommandResult::Output(format!("New session: {}", &new_id.to_string()[..8]))
        }
        SessionCommand::Switch { target } => {
            match ctx
                .session_manager
                .resume_session(target, ctx.session_meta, *ctx.turn_count)
            {
                Ok(resumed) => {
                    *ctx.history = resumed.history;
                    *ctx.session_id = resumed.new_session_id;
                    *ctx.session_meta = resumed.new_meta;
                    *ctx.turn_count = 0;
                    CommandResult::Output(format!(
                        "Switched to session {}. Restored {} messages.\nNew session: {}",
                        resumed.restored_from,
                        resumed.message_count,
                        &resumed.new_session_id.to_string()[..8],
                    ))
                }
                Err(e) => CommandResult::Output(e.to_string()),
            }
        }
        SessionCommand::Invalid => CommandResult::Output(
            "Usage: /session list | /session switch <id-prefix> | /session new".into(),
        ),
    }
}

fn parse_config_command(args: &str) -> ConfigCommand<'_> {
    let mut parts = args.split_whitespace();
    match parts.next() {
        None | Some("list") => ConfigCommand::List,
        Some("get") => parts
            .next()
            .map_or(ConfigCommand::Invalid, |section| ConfigCommand::Get {
                section,
            }),
        Some(_) => ConfigCommand::Invalid,
    }
}

fn dispatch_config(
    command: &ConfigCommand<'_>,
    config: &CortexConfig,
    providers: &ProviderRegistry,
) -> CommandResult {
    match command {
        ConfigCommand::List => {
            let summary = format_config_summary(config, providers);
            CommandResult::Output(summary)
        }
        ConfigCommand::Get { section } => match format_config_section(config, providers, section) {
            Ok(text) => CommandResult::Output(text),
            Err(e) => CommandResult::Output(e),
        },
        ConfigCommand::Invalid => {
            CommandResult::Output("Usage: /config list | /config get <section>".into())
        }
    }
}
