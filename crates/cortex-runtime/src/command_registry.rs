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

/// Trait for dispatching slash commands.
pub trait CommandRegistry {
    /// Dispatch a command string (e.g. "/session list") and return the result.
    fn dispatch(&self, input: &str, ctx: &mut CommandContext<'_>) -> CommandResult;

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

impl CommandRegistry for DefaultCommandRegistry {
    fn dispatch(&self, input: &str, ctx: &mut CommandContext<'_>) -> CommandResult {
        let trimmed = input.trim();
        if trimmed == "/quit" || trimmed == "/exit" {
            return CommandResult::Exit;
        }
        if trimmed == "/help" {
            return CommandResult::Output(
                "Commands:\n  \
                 /help                    Show this help\n  \
                 /status                  Show runtime status and token usage\n  \
                 /stop                    Cancel the running turn\n  \
                 /session list            List all sessions\n  \
                 /session new             Create a new session\n  \
                 /session switch <id>     Switch to a previous session\n  \
                 /config [section]        View configuration\n  \
                 /quit                    Quit"
                    .into(),
            );
        }
        if trimmed.starts_with("/session") {
            return dispatch_session(trimmed, ctx);
        }
        if trimmed.starts_with("/config") {
            return dispatch_config(trimmed, ctx.config, ctx.providers);
        }
        CommandResult::NotFound(format!(
            "Unknown command: {trimmed}\nType /help to see available commands"
        ))
    }

    fn list_commands(&self) -> Vec<&'static str> {
        vec![
            "/help", "/status", "/stop", "/session", "/config", "/quit", "/exit",
        ]
    }
}

fn dispatch_session(input: &str, ctx: &mut CommandContext<'_>) -> CommandResult {
    let parts: Vec<&str> = input.splitn(3, ' ').collect();
    match parts.get(1).copied() {
        Some("list") => {
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
        Some("new") => {
            ctx.session_manager
                .end_session(ctx.session_meta, *ctx.turn_count);
            ctx.history.clear();
            let (new_id, new_meta) = ctx.session_manager.create_session();
            *ctx.session_id = new_id;
            *ctx.session_meta = new_meta;
            *ctx.turn_count = 0;
            CommandResult::Output(format!("New session: {}", &new_id.to_string()[..8]))
        }
        Some("switch" | "resume") => {
            let prefix = parts.get(2).copied().unwrap_or("");
            if prefix.is_empty() {
                return CommandResult::Output("Usage: /session switch <id-prefix>".into());
            }
            match ctx
                .session_manager
                .resume_session(prefix, ctx.session_meta, *ctx.turn_count)
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
        _ => CommandResult::Output(
            "Usage: /session list | /session switch <id-prefix> | /session new".into(),
        ),
    }
}

fn dispatch_config(
    input: &str,
    config: &CortexConfig,
    providers: &ProviderRegistry,
) -> CommandResult {
    let parts: Vec<&str> = input.splitn(3, ' ').collect();
    match parts.get(1).copied() {
        Some("list") | None => {
            let summary = format_config_summary(config, providers);
            CommandResult::Output(summary)
        }
        Some("get") => parts.get(2).map_or_else(
            || {
                CommandResult::Output(
                    "Usage: /config get <section>\nSections: api, context, memory, embedding, metacognition, turn, autonomous, tools, providers, daemon, web, skills, auth, rate_limit, health, evolution".into(),
                )
            },
            |section| match format_config_section(config, providers, section) {
                Ok(text) => CommandResult::Output(text),
                Err(e) => CommandResult::Output(e),
            },
        ),
        _ => CommandResult::Output("Usage: /config list | /config get <section>".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_kernel::SessionStore;

    fn make_test_ctx(
        tmp: &tempfile::TempDir,
    ) -> (
        cortex_kernel::Journal,
        SessionStore,
        CortexConfig,
        ProviderRegistry,
    ) {
        let db_path = tmp.path().join("test.db");
        let journal = cortex_kernel::Journal::open(&db_path).unwrap();
        let session_store = SessionStore::open(&tmp.path().join("sessions")).unwrap();
        let config = CortexConfig::default();
        let providers = ProviderRegistry::new();
        (journal, session_store, config, providers)
    }

    #[test]
    fn quit_returns_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let (journal, session_store, config, providers) = make_test_ctx(&tmp);
        let sm = SessionManager::new(&journal, &session_store);
        let (mut sid, mut meta) = sm.create_session();
        let mut history = Vec::new();
        let mut turn_count = 0;

        let registry = DefaultCommandRegistry::new();
        let mut ctx = CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &config,
            providers: &providers,
        };
        let result = registry.dispatch("/quit", &mut ctx);
        assert!(matches!(result, CommandResult::Exit));
    }

    #[test]
    fn exit_returns_exit() {
        let tmp = tempfile::tempdir().unwrap();
        let (journal, session_store, config, providers) = make_test_ctx(&tmp);
        let sm = SessionManager::new(&journal, &session_store);
        let (mut sid, mut meta) = sm.create_session();
        let mut history = Vec::new();
        let mut turn_count = 0;

        let registry = DefaultCommandRegistry::new();
        let mut ctx = CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &config,
            providers: &providers,
        };
        let result = registry.dispatch("/exit", &mut ctx);
        assert!(matches!(result, CommandResult::Exit));
    }

    #[test]
    fn unknown_command_returns_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let (journal, session_store, config, providers) = make_test_ctx(&tmp);
        let sm = SessionManager::new(&journal, &session_store);
        let (mut sid, mut meta) = sm.create_session();
        let mut history = Vec::new();
        let mut turn_count = 0;

        let registry = DefaultCommandRegistry::new();
        let mut ctx = CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &config,
            providers: &providers,
        };
        let result = registry.dispatch("/foobar", &mut ctx);
        assert!(matches!(result, CommandResult::NotFound(_)));
    }

    #[test]
    fn session_list_shows_header() {
        let tmp = tempfile::tempdir().unwrap();
        let (journal, session_store, config, providers) = make_test_ctx(&tmp);
        let sm = SessionManager::new(&journal, &session_store);
        let (mut sid, mut meta) = sm.create_session();
        let mut history = Vec::new();
        let mut turn_count = 0;

        let registry = DefaultCommandRegistry::new();
        let mut ctx = CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &config,
            providers: &providers,
        };
        let result = registry.dispatch("/session list", &mut ctx);
        match result {
            CommandResult::Output(text) => assert!(text.contains("ID")),
            other => panic!("expected Output, got: {other:?}"),
        }
    }

    #[test]
    fn list_commands_contains_expected() {
        let registry = DefaultCommandRegistry::new();
        let cmds = registry.list_commands();
        assert!(cmds.contains(&"/session"));
        assert!(cmds.contains(&"/config"));
        assert!(cmds.contains(&"/quit"));
    }
}
