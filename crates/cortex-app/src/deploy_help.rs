#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploySubcommand {
    Install,
    Uninstall,
    Start,
    Stop,
    Restart,
    Status,
    Demo,
    Doctor,
    Ps,
    Reset,
    Plugin,
    Channel,
    Actor,
    Node,
    Browser,
    Permission,
    Config,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeployCommandSpec {
    pub subcommand: DeploySubcommand,
    names: &'static [&'static str],
    summary: &'static str,
    help: Option<&'static str>,
}

impl DeployCommandSpec {
    #[must_use]
    pub const fn primary_name(self) -> &'static str {
        self.names[0]
    }

    #[must_use]
    pub const fn names(self) -> &'static [&'static str] {
        self.names
    }

    #[must_use]
    pub const fn summary(self) -> &'static str {
        self.summary
    }

    #[must_use]
    pub const fn help(self) -> Option<&'static str> {
        self.help
    }
}

const DEPLOY_COMMAND_SPECS: &[DeployCommandSpec] = &[
    DeployCommandSpec {
        subcommand: DeploySubcommand::Install,
        names: &["install"],
        summary: "Install as systemd service",
        help: Some(
            "cortex install — Install as a systemd user service and start the daemon.\n\n\
Usage: cortex install [OPTIONS]\n\n\
Options:\n\
  --id <ID>       Instance ID (default: default)\n\
  --system        Install as system-level service (requires root)\n\
  --permission-level <strict|balanced|open>\n\
                  Tool confirmation policy: strict=Allow only, balanced=Review,\n\
                  open=all non-blocking tools without confirmation.\n\
                  Defaults to balanced when omitted.\n\n\
Environment variables (first install only):\n\
  CORTEX_API_KEY              LLM API key\n\
  CORTEX_PROVIDER             LLM provider (e.g. zai, anthropic, openai)\n\
  CORTEX_MODEL                LLM model name\n\
  CORTEX_BASE_URL             Custom provider base URL\n\
  CORTEX_LLM_PRESET           Preset (minimal, standard, cognitive, full)\n\
  CORTEX_EMBEDDING_PROVIDER   Embedding provider (e.g. ollama)\n\
  CORTEX_EMBEDDING_MODEL      Embedding model name\n\
  CORTEX_EMBEDDING_BASE_URL   Embedding provider base URL\n\
  CORTEX_EMBEDDING_API_KEY    Embedding provider API key\n\
  CORTEX_SHOW_THINKING        Enable provider thinking request/output (default false)\n\
  CORTEX_BRAVE_KEY            Brave Search API key\n\n\
  CORTEX_PERMISSION_LEVEL     Same values as --permission-level\n\n\
If a service already exists it will be stopped and reinstalled.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Uninstall,
        names: &["uninstall"],
        summary: "Remove service",
        help: Some(
            "cortex uninstall — Remove the systemd service.\n\n\
Usage: cortex uninstall [OPTIONS]\n\n\
Options:\n\
  --id <ID>     Instance ID (default: default)\n\
  --purge       Also delete all instance data (config, memory, sessions)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Start,
        names: &["start"],
        summary: "Start daemon",
        help: Some(
            "cortex start — Start the daemon via systemd.\n\nUsage: cortex start [--id <ID>]",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Stop,
        names: &["stop"],
        summary: "Stop daemon",
        help: Some("cortex stop — Stop the daemon via systemd.\n\nUsage: cortex stop [--id <ID>]"),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Restart,
        names: &["restart"],
        summary: "Restart daemon",
        help: Some(
            "cortex restart — Restart the daemon via systemd.\n\nUsage: cortex restart [--id <ID>]",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Status,
        names: &["status"],
        summary: "Show daemon status",
        help: Some(
            "cortex status — Show daemon status.\n\n\
Usage: cortex status [--id <ID>]\n\n\
Displays: active state, PID, socket path, data directory, HTTP address,\n\
          current LLM provider/model/preset, permission mode, context and token usage.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Demo,
        names: &["demo"],
        summary: "Create a local first-run demo fixture",
        help: Some(
            "cortex demo — Create a local first-run demo fixture.\n\n\
Usage: cortex demo [--id <ID>] [--home <PATH>] [--force]\n\n\
Creates a user-local instance (default id: demo), an external demo workspace,\n\
an Ollama-oriented config, empty MCP config, and a local-coding demo skill.\n\
The command does not start services, enable plugins, broaden permissions, or\n\
modify protected runtime state outside the selected demo instance. Use --force\n\
to refresh demo-owned files when the target instance already exists.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Doctor,
        names: &["doctor"],
        summary: "Run local readiness checks",
        help: Some(
            "cortex doctor — Run local readiness checks without changing runtime state.\n\n\
Usage: cortex doctor [--id <ID>] [--system] [--json]\n\n\
Checks OS/systemd availability, instance paths, service/socket state, config,\n\
provider key posture, permission mode, enabled plugins, channel auth,\n\
policy lint findings, protected runtime root paths, and local model endpoint hints.\n\
Use --json for a machine-readable report with remediation hints.\n\
Findings are operator guidance; policy/risk gates are not sandbox containment.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Ps,
        names: &["ps"],
        summary: "List all instances",
        help: Some(
            "cortex ps — List all instances with their status.\n\n\
Usage: cortex ps\n\n\
Shows instance name, status (running/stopped/uninstalled), and socket path.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Reset,
        names: &["reset"],
        summary: "Clear data (keep config); --factory for full wipe",
        help: Some(
            "cortex reset — Clear instance data while preserving configuration.\n\n\
Usage: cortex reset [OPTIONS]\n\n\
Options:\n\
  --id <ID>     Instance ID (default: default)\n\
  --force, -f   Skip confirmation and auto-stop the daemon if running\n\
  --factory     Factory reset: delete everything including config and\n\
                recreate the instance from scratch\n\n\
By default, reset preserves config.toml and clears data, memory,\n\
sessions, prompts, and skills. With --factory, the entire instance\n\
directory is deleted and recreated as if freshly installed.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Plugin,
        names: &["plugin"],
        summary: "Manage plugins",
        help: Some(
            "cortex plugin — Manage plugins.\n\n\
Subcommands:\n\
  install <source>    Install from .cpx file, URL, directory, or name[@version]\n\
                      Names resolve to GitHub: github.com/by-scott/cortex-plugin-<name>\n\
                      Packaged installs require a valid Ed25519 package signature;\n\
                      add --yes after reviewing a new verified publisher key\n\
  enable <name>       Enable an installed plugin for one instance\n\
  disable <name>      Disable an installed plugin for one instance\n\
  uninstall <name>    Disable for one instance; add --purge to remove files\n\
  list                List installed plugins with status\n\
  review <dir>        Show capability, signature, sandbox, and risk summary\n\
  test <dir>          Run the local plugin conformance kit\n\
  keygen <path>       Create a local Ed25519 plugin signing key\n\
  sign <dir> --key <path> [--publisher <id>]\n\
                      Write signed package.toml metadata for publishing\n\
  pack <dir> [out]    Create .cpx archive; default is <repo>-v<version>-<platform>.cpx",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Actor,
        names: &["actor"],
        summary: "Manage actor aliases and transport bindings",
        help: Some(
            "cortex actor — Identity mapping for unified session ownership.\n\n\
Subcommands:\n\
  alias list                    List actor aliases\n\
  alias set <from> <to>         Map one actor to a canonical actor\n\
  alias unset <from>            Remove an actor alias\n\
  transport list                List transport actor bindings\n\
  transport set <name|all> <actor>  Bind transport to actor (all = http,rpc,ws,sock,stdio)\n\
  transport unset <name>            Remove transport binding\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Channel,
        names: &["channel"],
        summary: "Manage channel pairing and policy",
        help: Some(
            "cortex channel — Messaging channel management.\n\n\
Channels run inside the daemon automatically when auth.json exists.\n\n\
Subcommands:\n\
  telegram              Show Telegram configuration info\n\
  whatsapp              Show WhatsApp configuration info\n\
  qq                    Show QQ configuration info\n\
  pair [platform]       Show pending/paired users\n\
  subscribe <plat> <id> Enable session subscription for a paired user\n\
  unsubscribe <plat> <id>\n\
                        Disable session subscription for a paired user\n\
  approve <plat> <id> [--subscribe|--no-subscribe]\n\
                        Approve a user and optionally configure subscription\n\
  revoke <plat> <id>    Remove a paired user\n\
  allow <plat> <id>     Add user to whitelist\n\
  deny <plat> <id>      Add user to blacklist\n\
  unallow <plat> <id>   Remove from whitelist\n\
  undeny <plat> <id>    Remove from blacklist\n\
  policy <plat> [mode]  Show/set policy (pairing|whitelist|open)\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\n\
Environment variables:\n\
  CORTEX_TELEGRAM_TOKEN  Telegram bot token\n\
  CORTEX_WHATSAPP_TOKEN  WhatsApp access token\n\
  CORTEX_QQ_APP_ID       QQ Bot AppID\n\
  CORTEX_QQ_APP_SECRET   QQ Bot AppSecret\n\
  CORTEX_QQ_MARKDOWN     QQ markdown output (default: true)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Node,
        names: &["node"],
        summary: "Manage Node.js tools for MCP servers",
        help: Some(
            "cortex node — Node.js environment management.\n\n\
Subcommands:\n\
  setup                 Install Node.js and pnpm for MCP servers\n\
  status                Show Node.js environment status\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Browser,
        names: &["browser"],
        summary: "Manage browser integration",
        help: Some(
            "cortex browser — Browser integration management.\n\n\
Subcommands:\n\
  enable                Configure Chrome DevTools MCP server\n\
  disable               Remove Chrome DevTools MCP server configuration\n\
  status                Show browser integration status\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Permission,
        names: &["permission"],
        summary: "Show or change the permission mode",
        help: Some(
            "cortex permission — Show or change the tool confirmation mode.\n\n\
Usage: cortex permission [strict|balanced|open] [OPTIONS]\n\n\
Modes:\n\
  strict     Auto-approve only Allow\n\
  balanced   Auto-approve through Review (default)\n\
  open       Auto-approve all non-blocking tools\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\
  --system   Update the system instance config (restart required to apply)\n\n\
Without a mode, prints the current setting.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Config,
        names: &["config"],
        summary: "View or update selected config keys",
        help: Some(
            "cortex config — View or update selected instance config keys.\n\n\
Usage:\n\
  cortex config list [--id <ID>]\n\
  cortex config get <section> [--id <ID>]\n\
  cortex config set <key> <value> [--id <ID>]\n\n\
Supported writable keys:\n\
  turn.show_thinking        true enables provider thinking request/output\n\
  turn.strip_think_tags     true disables provider thinking output (default)\n\
  embedding.api_key         embedding provider API key\n\n\
Changes are written to config.toml and hot-reloaded when the user daemon is running.",
        ),
    },
    DeployCommandSpec {
        subcommand: DeploySubcommand::Policy,
        names: &["policy"],
        summary: "Lint and simulate runtime policy",
        help: Some(
            "cortex policy — Policy-as-code checks for the current instance.\n\n\
Subcommands:\n\
  lint                         Check config and enabled plugins\n\
  simulate <tool> [OPTIONS]    Explain one tool/effect decision\n\n\
Simulation options:\n\
  --tool <NAME>                Tool name; alternative to positional <tool>\n\
  --actor <ACTOR>              Actor label for the report\n\
  --effect <KIND[:TARGET]>     Declared effect; repeatable\n\
  --background                 Simulate background execution\n\n\
Effect kinds include read_file, read_secret, write_file, delete_file,\n\
run_process, network_request, send_message, spend_money, deploy,\n\
modify_credential, persist_memory, publish_content, schedule_task,\n\
generate_media, introspect_runtime, delegate_work.\n\n\
Options:\n\
  --id <ID>  Instance ID (default: default)\n\
  --system   Read the system instance config",
        ),
    },
];

pub const fn deploy_command_specs() -> &'static [DeployCommandSpec] {
    DEPLOY_COMMAND_SPECS
}

pub fn parse_deploy_subcommand(cmd: &str) -> Option<DeploySubcommand> {
    deploy_command_specs()
        .iter()
        .find(|spec| spec.names().contains(&cmd))
        .map(|spec| spec.subcommand)
}
