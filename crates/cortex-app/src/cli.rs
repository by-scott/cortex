use crate::deploy;
use crate::scaffold;

pub const KNOWN_FLAGS: &[&str] = &[
    "--acp",
    "--mcp-server",
    "--daemon",
    "--stdio",
    "--session",
    "--home",
    "--new-plugin",
    "--new-process-plugin",
    "--help",
    "--version",
    "--system",
    "--user",
    "--purge",
    "--force",
    "--factory",
    "--subscribe",
    "--no-subscribe",
    "--id",
    "-h",
    "-V",
    "-f",
];
pub const VALUE_FLAGS: &[&str] = &[
    "--home",
    "--new-plugin",
    "--new-process-plugin",
    "--id",
    "--session",
];

pub enum RunMode {
    Repl,
    Acp,
    McpServer,
    Daemon { stdio: bool },
}

fn is_known_subcommand(name: &str) -> bool {
    deploy::deploy_command_specs()
        .iter()
        .any(|spec| spec.names().contains(&name))
}

// ── Early CLI handling ───────────────────────────────────────

pub fn handle_early_args(args: &[String]) {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        std::process::exit(0);
    }
    // `cortex help [subcommand]` (subcommand style, no dashes)
    if args.get(1).is_some_and(|a| a == "help") {
        if let Some(sub) = args.get(2) {
            print_subcommand_help(sub);
        } else {
            print_help();
        }
        std::process::exit(0);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        eprintln!("cortex {}", env!("CARGO_PKG_VERSION"));
        std::process::exit(0);
    }
    reject_unknown_flags(args);
    handle_new_plugin(args);
    handle_deploy_subcommand(args);

    // Unknown subcommands fall through to REPL/pipe mode, which will report
    // "daemon is not running" if the daemon is unavailable, or send the input
    // as a prompt if the daemon is running.
}

fn print_help() {
    eprintln!(
        "\
Cortex v{ver} -- Cognitive Runtime

Usage: cortex [OPTIONS] [COMMAND]

Modes:
  (default)         REPL interactive mode
  --daemon          Daemon mode (HTTP + Socket + stdio)
  --acp             Agent Client Protocol mode
  --mcp-server      MCP Server (stdio JSON-RPC)

Options:
  --home <PATH>     Data directory (default: ~/.cortex)
  --id <ID>         Instance ID (default: default)
  --new-plugin <N>  Generate trusted in-process plugin scaffold
  --new-process-plugin <N>
                   Generate process-isolated plugin scaffold
  --help, -h        Show this help
  --version, -V     Show version

Commands:
",
        ver = env!("CARGO_PKG_VERSION")
    );

    for spec in deploy::deploy_command_specs() {
        eprintln!("  {:<18} {}", spec.primary_name(), spec.summary());
    }

    eprintln!("\nClient: cortex \"question\" for single-prompt pipe mode");
}

fn print_subcommand_help(sub: &str) {
    if let Some(spec) = deploy::deploy_command_specs()
        .iter()
        .find(|spec| spec.names().contains(&sub))
        && let Some(help) = spec.help()
    {
        eprintln!("{help}");
        return;
    }

    eprintln!("Unknown command: {sub}");
    eprintln!("Run 'cortex help' for available commands.");
}

fn reject_unknown_flags(args: &[String]) {
    for (i, arg) in args.iter().enumerate().skip(1) {
        if arg.starts_with("--")
            && !KNOWN_FLAGS.contains(&arg.as_str())
            && !(i > 0 && VALUE_FLAGS.contains(&args[i - 1].as_str()))
        {
            eprintln!("Error: unknown option '{arg}'\nRun 'cortex --help' for usage.");
            std::process::exit(1);
        }
    }
}

fn handle_new_plugin(args: &[String]) {
    if let Some(idx) = args.iter().position(|a| a == "--new-process-plugin") {
        if let Some(name) = args.get(idx + 1) {
            match scaffold::generate_process_plugin(name) {
                Ok(dir) => {
                    eprintln!("Created process plugin project: {dir}/");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            }
        } else {
            eprintln!("Usage: cortex --new-process-plugin <name>");
            std::process::exit(1);
        }
    }

    let Some(idx) = args.iter().position(|a| a == "--new-plugin") else {
        return;
    };
    if let Some(name) = args.get(idx + 1) {
        match scaffold::generate_plugin(name) {
            Ok(dir) => {
                eprintln!("Created plugin project: {dir}/");
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("Usage: cortex --new-plugin <name>");
        std::process::exit(1);
    }
}

fn handle_deploy_subcommand(args: &[String]) {
    let Some(subcmd) = args.iter().skip(1).find(|a| {
        !a.starts_with("--") && {
            let pi = args.iter().position(|x| x == *a).unwrap_or(0);
            pi == 0 || !VALUE_FLAGS.contains(&args[pi - 1].as_str())
        }
    }) else {
        return;
    };
    let sc = subcmd.clone();
    // Pass all args (except binary name) so flags like --id before the
    // subcommand are visible to the handler.
    if let Some(result) = deploy::dispatch(&sc, &args[1..]) {
        match result {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
    }
}

// ── Mode detection ───────────────────────────────────────────

#[must_use]
pub fn check_exclusive_modes(args: &[String]) -> RunMode {
    let a = args.iter().any(|a| a == "--acp");
    let m = args.iter().any(|a| a == "--mcp-server");
    let d = args.iter().any(|a| a == "--daemon");
    if u8::from(a) + u8::from(m) + u8::from(d) > 1 {
        eprintln!("Error: --acp, --mcp-server, and --daemon are mutually exclusive.");
        std::process::exit(1);
    }
    if a {
        RunMode::Acp
    } else if m {
        RunMode::McpServer
    } else if d {
        RunMode::Daemon {
            stdio: args.iter().any(|a| a == "--stdio"),
        }
    } else {
        RunMode::Repl
    }
}

#[must_use]
pub fn parse_arg_value(args: &[String], flag: &str) -> Option<String> {
    args.iter()
        .position(|a| a == flag)
        .and_then(|i| args.get(i + 1))
        .cloned()
        .filter(|v| !v.is_empty())
}

/// Validate an instance ID: non-empty, ≤64 chars, alphanumeric/hyphen/underscore only.
///
/// # Errors
/// Returns an error message if the ID is empty, too long, or contains invalid characters.
pub fn validate_instance_id(id: &str) -> Result<(), String> {
    if id.is_empty() {
        return Err("instance ID must not be empty".into());
    }
    if id.len() > 64 {
        return Err("instance ID must not exceed 64 characters".into());
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(
            "instance ID must contain only alphanumeric characters, hyphens, or underscores".into(),
        );
    }
    Ok(())
}

#[must_use]
pub fn detect_pipe_prompt(args: &[String]) -> Option<String> {
    for (i, arg) in args.iter().enumerate().skip(1) {
        if i > 0 && VALUE_FLAGS.contains(&args[i - 1].as_str()) {
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if is_known_subcommand(arg) || arg == "help" {
            return None;
        }
        return Some(arg.clone());
    }
    None
}

#[cfg(test)]
mod tests {
    use super::detect_pipe_prompt;

    #[test]
    fn detect_pipe_prompt_ignores_known_nested_subcommands() {
        let args = vec![
            "cortex".to_string(),
            "actor".to_string(),
            "alias".to_string(),
            "list".to_string(),
        ];
        assert_eq!(detect_pipe_prompt(&args), None);

        let browser_args = vec![
            "cortex".to_string(),
            "browser".to_string(),
            "status".to_string(),
        ];
        assert_eq!(detect_pipe_prompt(&browser_args), None);
    }

    #[test]
    fn detect_pipe_prompt_still_accepts_freeform_prompt() {
        let args = vec!["cortex".to_string(), "explain runtime".to_string()];
        assert_eq!(
            detect_pipe_prompt(&args),
            Some("explain runtime".to_string())
        );
    }

    #[test]
    fn channel_subscription_flags_are_known() {
        assert!(super::KNOWN_FLAGS.contains(&"--subscribe"));
        assert!(super::KNOWN_FLAGS.contains(&"--no-subscribe"));
    }

    #[test]
    fn process_plugin_scaffold_flag_is_known_and_value_taking() {
        assert!(super::KNOWN_FLAGS.contains(&"--new-process-plugin"));
        assert!(super::VALUE_FLAGS.contains(&"--new-process-plugin"));
    }
}
