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
    "--help",
    "--version",
    "--system",
    "--user",
    "--purge",
    "--force",
    "--factory",
    "--id",
    "-h",
    "-V",
    "-f",
];
pub const VALUE_FLAGS: &[&str] = &["--home", "--new-plugin", "--id", "--session"];

pub enum RunMode {
    Repl,
    Acp,
    McpServer,
    Daemon { stdio: bool },
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
  --new-plugin <N>  Generate plugin scaffold
  --help, -h        Show this help
  --version, -V     Show version

Commands:
  install [--system] [--id ID]  Install as systemd service
  uninstall [--purge]           Remove service
  start / stop / restart        Manage daemon
  status            Show daemon status
  ps                List all instances
  reset [--factory]   Clear data (keep config); --factory for full wipe
  plugin install <source>   Install plugin (.cpx file, URL, local dir, or owner/repo)
  plugin uninstall <name>   Remove an installed plugin
  plugin list               List installed plugins
  plugin pack <dir>         Pack a directory into a .cpx file
  channel telegram        Show Telegram configuration info
  channel whatsapp        Show WhatsApp configuration info
  channel pair [platform]       Show pending/paired users
  channel approve <plat> <id>   Approve a user (skip pairing code)
  channel revoke <plat> <id>    Remove a paired user
  channel allow <plat> <id>     Add user to whitelist
  channel deny <plat> <id>      Add user to blacklist
  channel policy <plat> [mode]  Show/set policy (pairing|whitelist|open)
  node setup              Install Node.js + pnpm (for MCP servers)
  node status             Show Node.js environment status
  browser enable          Configure Chrome DevTools MCP server
  browser status          Show browser integration status

Client: cortex \"question\" for single-prompt pipe mode",
        ver = env!("CARGO_PKG_VERSION")
    );
}

fn print_subcommand_help(sub: &str) {
    match sub {
        "install" | "deploy" => eprintln!(
            "\
cortex install — Install as a systemd user service and start the daemon.

Usage: cortex install [OPTIONS]

Options:
  --id <ID>       Instance ID (default: default)
  --system        Install as system-level service (requires root)

Environment variables (first install only):
  CORTEX_API_KEY              LLM API key
  CORTEX_PROVIDER             LLM provider (e.g. zai, anthropic, openai)
  CORTEX_MODEL                LLM model name
  CORTEX_BASE_URL             Custom provider base URL
  CORTEX_LLM_PRESET           Preset (minimal, standard, cognitive, full)
  CORTEX_EMBEDDING_PROVIDER   Embedding provider (e.g. ollama)
  CORTEX_EMBEDDING_MODEL      Embedding model name
  CORTEX_EMBEDDING_BASE_URL   Embedding provider base URL
  CORTEX_BRAVE_KEY            Brave Search API key

If a service already exists it will be stopped and redeployed."
        ),
        "uninstall" | "undeploy" => eprintln!(
            "\
cortex uninstall — Remove the systemd service.

Usage: cortex uninstall [OPTIONS]

Options:
  --id <ID>     Instance ID (default: default)
  --purge       Also delete all instance data (config, memory, sessions)"
        ),
        "start" => eprintln!(
            "\
cortex start — Start the daemon via systemd.

Usage: cortex start [--id <ID>]"
        ),
        "stop" => eprintln!(
            "\
cortex stop — Stop the daemon via systemd.

Usage: cortex stop [--id <ID>]"
        ),
        "restart" => eprintln!(
            "\
cortex restart — Restart the daemon via systemd.

Usage: cortex restart [--id <ID>]"
        ),
        "status" => eprintln!(
            "\
cortex status — Show daemon status.

Usage: cortex status [--id <ID>]

Displays: active state, PID, socket path, data directory, HTTP address,
          current LLM provider/model/preset."
        ),
        "ps" => eprintln!(
            "\
cortex ps — List all instances with their status.

Usage: cortex ps

Shows instance name, status (running/stopped/uninstalled), and socket path."
        ),
        "reset" => eprintln!(
            "\
cortex reset — Clear instance data while preserving configuration.

Usage: cortex reset [OPTIONS]

Options:
  --id <ID>     Instance ID (default: default)
  --force, -f   Skip confirmation and auto-stop the daemon if running
  --factory     Factory reset: delete everything including config and
                recreate the instance from scratch

By default, reset preserves config.toml and clears data, memory,
sessions, prompts, and skills. With --factory, the entire instance
directory is deleted and recreated as if freshly installed."
        ),
        "plugin" => eprintln!(
            "\
cortex plugin -- Manage plugins.

Subcommands:
  install <source>    Install from .cpx file, URL, directory, or name
                      Names resolve to GitHub: github.com/by-scott/cortex-plugin-<name>
  uninstall <name>    Remove an installed plugin
  list                List installed plugins with status
  pack <dir> [out]    Create .cpx archive from plugin directory"
        ),
        "channel" => print_channel_help(),
        _ => {
            eprintln!("Unknown command: {sub}");
            eprintln!("Run 'cortex help' for available commands.");
        }
    }
}

fn print_channel_help() {
    eprintln!(
        "\
cortex channel -- Messaging channel management.

Channels run inside the daemon automatically when auth.json exists.

Subcommands:
  telegram              Show Telegram configuration info
  whatsapp              Show WhatsApp configuration info
  pair [platform]       Show pending/paired users
  approve <plat> <id>   Approve a user (skip pairing code)
  revoke <plat> <id>    Remove a paired user
  allow <plat> <id>     Add user to whitelist
  deny <plat> <id>      Add user to blacklist
  unallow <plat> <id>   Remove from whitelist
  undeny <plat> <id>    Remove from blacklist
  policy <plat> [mode]  Show/set policy (pairing|whitelist|open)

Options:
  --id <ID>  Instance ID (default: default)

Environment variables:
  CORTEX_TELEGRAM_TOKEN  Telegram bot token
  CORTEX_WHATSAPP_TOKEN  WhatsApp access token"
    );
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
    const SUBS: &[&str] = &[
        "install",
        "uninstall",
        "deploy",
        "undeploy",
        "start",
        "stop",
        "restart",
        "status",
        "ps",
        "reset",
        "help",
        "plugin",
        "channel",
    ];
    for (i, arg) in args.iter().enumerate().skip(1) {
        if i > 0 && VALUE_FLAGS.contains(&args[i - 1].as_str()) {
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        if SUBS.contains(&arg.as_str()) {
            return None;
        }
        return Some(arg.clone());
    }
    None
}
