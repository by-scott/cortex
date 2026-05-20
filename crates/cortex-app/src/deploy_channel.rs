use std::io::Write as _;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChannelSubcommand {
    Telegram,
    Whatsapp,
    Qq,
    Qclaw,
    Pair,
    Subscribe,
    Unsubscribe,
    Approve,
    Allow,
    Deny,
    Unallow,
    Undeny,
    Revoke,
    Policy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PolicyListKind {
    Whitelist,
    Blacklist,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommandSpec<T> {
    subcommand: T,
    names: &'static [&'static str],
    summary: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DetailUsageSpec {
    usage: &'static str,
    summary: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChannelInvocation<'a> {
    subcommand: Option<&'a str>,
    remaining: &'a [String],
}

struct ChannelPairOptions {
    platform: Option<String>,
}

const CHANNEL_SUBCOMMAND_SPECS: &[CommandSpec<ChannelSubcommand>] = &[
    CommandSpec {
        subcommand: ChannelSubcommand::Telegram,
        names: &["telegram"],
        summary: "Show Telegram configuration info",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Whatsapp,
        names: &["whatsapp"],
        summary: "Show WhatsApp configuration info",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Qq,
        names: &["qq"],
        summary: "Show QQ configuration info",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Qclaw,
        names: &["qclaw"],
        summary: "Show QClaw adapter configuration info",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Pair,
        names: &["pair"],
        summary: "Show pending/paired users",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Subscribe,
        names: &["subscribe"],
        summary: "Enable session subscription for a paired user",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Unsubscribe,
        names: &["unsubscribe"],
        summary: "Disable session subscription for a paired user",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Approve,
        names: &["approve"],
        summary: "Approve a user (platform: telegram|whatsapp|qq|qclaw)",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Allow,
        names: &["allow"],
        summary: "Add user to whitelist",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Deny,
        names: &["deny"],
        summary: "Add user to blacklist",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Unallow,
        names: &["unallow"],
        summary: "Remove user from whitelist",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Undeny,
        names: &["undeny"],
        summary: "Remove user from blacklist",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Revoke,
        names: &["revoke"],
        summary: "Remove a paired user",
    },
    CommandSpec {
        subcommand: ChannelSubcommand::Policy,
        names: &["policy"],
        summary: "Show or set policy (pairing|whitelist|open)",
    },
];

const CHANNEL_DETAIL_SPECS: &[DetailUsageSpec] = &[
    DetailUsageSpec {
        usage: "pair [platform]",
        summary: "Show pair state",
    },
    DetailUsageSpec {
        usage: "subscribe <platform> <user_id>",
        summary: "Enable session broadcasts for a paired user",
    },
    DetailUsageSpec {
        usage: "unsubscribe <platform> <user_id>",
        summary: "Disable session broadcasts for a paired user",
    },
    DetailUsageSpec {
        usage: "approve <platform> <user_id> [--subscribe|--no-subscribe]",
        summary: "Approve a pending user and optionally change subscription",
    },
    DetailUsageSpec {
        usage: "revoke <platform> <user_id>",
        summary: "Revoke a paired user immediately",
    },
    DetailUsageSpec {
        usage: "allow <platform> <user_id>",
        summary: "Add a user to the whitelist",
    },
    DetailUsageSpec {
        usage: "deny <platform> <user_id>",
        summary: "Add a user to the blacklist",
    },
    DetailUsageSpec {
        usage: "unallow <platform> <user_id>",
        summary: "Remove a user from the whitelist",
    },
    DetailUsageSpec {
        usage: "undeny <platform> <user_id>",
        summary: "Remove a user from the blacklist",
    },
    DetailUsageSpec {
        usage: "policy <platform> [mode]",
        summary: "Show or set policy mode",
    },
];

impl PolicyListKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
        }
    }

    const fn store_list(self) -> cortex_runtime::channels::store::PolicyList {
        match self {
            Self::Whitelist => cortex_runtime::channels::store::PolicyList::Whitelist,
            Self::Blacklist => cortex_runtime::channels::store::PolicyList::Blacklist,
        }
    }
}

/// `cortex channel <telegram|whatsapp|qq|qclaw|pair> [options]`
///
/// Channels run inside the daemon. This command provides configuration info
/// and file-backed pairing management.
pub fn cmd_channel(args: &[String]) {
    let paths = crate::deploy::resolve_paths_from_args(args);
    let instance_home = paths.instance_home();
    let invocation = parse_channel_invocation(args);
    let remaining = invocation.remaining;

    match parse_channel_subcommand(invocation.subcommand) {
        Some(ChannelSubcommand::Telegram) => cmd_channel_telegram(&instance_home),
        Some(ChannelSubcommand::Whatsapp) => cmd_channel_whatsapp(&instance_home),
        Some(ChannelSubcommand::Qq) => cmd_channel_qq(&instance_home),
        Some(ChannelSubcommand::Qclaw) => cmd_channel_qclaw(args, remaining, &instance_home),
        Some(ChannelSubcommand::Pair) => cmd_channel_pair(remaining, &instance_home),
        Some(ChannelSubcommand::Subscribe) => {
            cmd_channel_subscription(args, remaining, &instance_home, true);
        }
        Some(ChannelSubcommand::Unsubscribe) => {
            cmd_channel_subscription(args, remaining, &instance_home, false);
        }
        Some(ChannelSubcommand::Approve) => cmd_channel_approve(args, remaining, &instance_home),
        Some(ChannelSubcommand::Allow) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Whitelist, true);
        }
        Some(ChannelSubcommand::Deny) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Blacklist, true);
        }
        Some(ChannelSubcommand::Unallow) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Whitelist, false);
        }
        Some(ChannelSubcommand::Undeny) => {
            cmd_channel_list_op(remaining, &instance_home, PolicyListKind::Blacklist, false);
        }
        Some(ChannelSubcommand::Revoke) => cmd_channel_revoke(remaining, &instance_home),
        Some(ChannelSubcommand::Policy) => cmd_channel_policy(remaining, &instance_home),
        None => print_channel_usage(),
    }
}

fn parse_channel_invocation(args: &[String]) -> ChannelInvocation<'_> {
    let root_pos = args.iter().position(|arg| arg == "channel");
    let after_root = root_pos.map_or(args, |pos| &args[pos + 1..]);

    let mut index = 0;
    while index < after_root.len() {
        let arg = after_root[index].as_str();
        if matches!(arg, "--id" | "--home") {
            index += 2;
            continue;
        }
        if arg.starts_with('-') {
            index += 1;
            continue;
        }
        return ChannelInvocation {
            subcommand: Some(arg),
            remaining: &after_root[index + 1..],
        };
    }

    ChannelInvocation {
        subcommand: None,
        remaining: &[],
    }
}

fn parse_channel_subcommand(subcommand: Option<&str>) -> Option<ChannelSubcommand> {
    let subcommand = subcommand?;
    CHANNEL_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
}

fn print_channel_usage() {
    eprintln!("Usage: cortex channel <subcommand>");
    eprintln!();
    eprintln!("Channels run inside the daemon automatically.");
    for spec in CHANNEL_SUBCOMMAND_SPECS {
        eprintln!("  {:<28} {}", spec.names[0], spec.summary);
    }
    for spec in CHANNEL_DETAIL_SPECS {
        eprintln!("  {:<28} {}", spec.usage, spec.summary);
    }
}

fn print_usage_line(usage: &str) {
    eprintln!("Usage: {usage}");
}

fn channel_action_usage(scope: &str, required_args: &[&str]) -> String {
    let suffix = if required_args.is_empty() {
        String::new()
    } else {
        format!(" {}", required_args.join(" "))
    };
    format!("cortex channel {scope}{suffix}")
}

fn cmd_channel_telegram(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "telegram").auth;
    let has_token = auth_path.exists();

    eprintln!("Telegram channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (token present)");
        eprintln!("  The daemon will start Telegram polling/webhook automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_TELEGRAM_TOKEN=<token> and reinstall:");
        eprintln!("       CORTEX_TELEGRAM_TOKEN=123:ABC cortex install");
        eprintln!("    2. Or create channels/telegram/auth.json with {{\"bot_token\": \"...\"}}");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_whatsapp(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "whatsapp").auth;
    let has_token = auth_path.exists();

    eprintln!("WhatsApp channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (token present)");
        eprintln!("  The daemon will start WhatsApp webhook automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_WHATSAPP_TOKEN=<token> and reinstall:");
        eprintln!("       CORTEX_WHATSAPP_TOKEN=EAA... cortex install");
        eprintln!("    2. Or create channels/whatsapp/auth.json with credentials");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_qq(home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "qq").auth;
    let has_token = auth_path.exists();

    eprintln!("QQ channel (runs inside daemon)");
    eprintln!();
    if has_token {
        eprintln!("  Status: configured (AppID/AppSecret present)");
        eprintln!("  The daemon will start QQ Bot WebSocket automatically.");
    } else {
        eprintln!("  Status: not configured");
        eprintln!();
        eprintln!("  To enable:");
        eprintln!("    1. Set CORTEX_QQ_APP_ID / CORTEX_QQ_APP_SECRET and reinstall:");
        eprintln!("       CORTEX_QQ_APP_ID=123 CORTEX_QQ_APP_SECRET=xyz cortex install");
        eprintln!("    2. Or create channels/qq/auth.json with QQ credentials");
        eprintln!("    3. Restart the daemon: cortex restart");
    }
}

fn cmd_channel_qclaw(command_args: &[String], args: &[String], home: &Path) {
    let auth_path = cortex_kernel::ChannelFileSet::from_instance_home(home, "qclaw").auth;
    let has_token = auth_path.exists();
    let args = qclaw_args_without_global_flags(args);

    match args.first().map(String::as_str) {
        Some("login") => {
            let options = parse_qclaw_login_options(&args[1..]);
            match run_qclaw_login(home, &options) {
                Ok(credentials) => {
                    crate::deploy::reload_running_daemon_config(command_args);
                    eprintln!("QClaw adapter configured.");
                    eprintln!("  account_id: {}", credentials.account_id);
                    if let Some(user_id) = credentials.user_id.as_deref() {
                        eprintln!("  user_id: {user_id}");
                    }
                    eprintln!("Restart the daemon if it is not already running.");
                }
                Err(error) => {
                    eprintln!("QClaw login failed: {error}");
                    std::process::exit(1);
                }
            }
        }
        Some("--help" | "-h" | "help") => print_qclaw_usage(),
        Some(other) => {
            eprintln!("Unknown QClaw command: {other}");
            print_qclaw_usage();
        }
        None => {
            eprintln!("QClaw adapter channel (runs inside daemon)");
            eprintln!();
            if has_token {
                eprintln!("  Status: configured (iLink token present)");
                eprintln!("  The daemon will start QClaw long-polling automatically.");
            } else {
                eprintln!("  Status: not configured");
                eprintln!();
                eprintln!("  To enable:");
                eprintln!("    cortex channel qclaw login");
                eprintln!("    cortex restart");
            }
        }
    }
}

fn qclaw_args_without_global_flags(args: &[String]) -> Vec<String> {
    let mut filtered = Vec::new();
    let mut index = 0;
    while index < args.len() {
        if matches!(args[index].as_str(), "--id" | "--home") {
            index += 2;
            continue;
        }
        filtered.push(args[index].clone());
        index += 1;
    }
    filtered
}

fn print_qclaw_usage() {
    eprintln!("Usage: cortex channel qclaw [login]");
    eprintln!();
    eprintln!("Commands:");
    eprintln!("  login                      Start QClaw QR login and save credentials");
    eprintln!();
    eprintln!("Options for login:");
    eprintln!("  --base-url <url>            iLink API base URL");
    eprintln!("  --route-tag <tag>           Optional QClaw route tag");
    eprintln!("  --bot-agent <agent>         Bot agent identifier sent in base_info");
}

fn parse_qclaw_login_options(
    args: &[String],
) -> cortex_runtime::channels::qclaw::QclawLoginOptions {
    let mut options = cortex_runtime::channels::qclaw::QclawLoginOptions::default();
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--base-url" => options.base_url = iter.next().cloned(),
            "--route-tag" => options.route_tag = iter.next().cloned(),
            "--bot-agent" => options.bot_agent = iter.next().cloned(),
            _ => {}
        }
    }
    options
}

fn run_qclaw_login(
    home: &Path,
    options: &cortex_runtime::channels::qclaw::QclawLoginOptions,
) -> Result<cortex_runtime::channels::qclaw::QclawLoginCredentials, String> {
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        tokio::task::block_in_place(|| {
            handle.block_on(cortex_runtime::channels::qclaw::login_with_qr(
                home,
                options,
                print_qclaw_qr,
                read_qclaw_verify_code,
            ))
        })
    } else {
        tokio::runtime::Runtime::new()
            .map_err(|error| format!("failed to start runtime: {error}"))?
            .block_on(cortex_runtime::channels::qclaw::login_with_qr(
                home,
                options,
                print_qclaw_qr,
                read_qclaw_verify_code,
            ))
    }
}

fn print_qclaw_qr(url: &str) {
    eprintln!("Scan this QClaw QR code with WeChat:");
    match qrcode::QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let rendered = code
                .render::<qrcode::render::unicode::Dense1x2>()
                .quiet_zone(false)
                .build();
            eprintln!("{rendered}");
        }
        Err(error) => eprintln!("Could not render QR code: {error}"),
    }
    eprintln!("Fallback URL: {url}");
}

fn read_qclaw_verify_code(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    std::io::stderr()
        .flush()
        .map_err(|error| format!("failed to flush prompt: {error}"))?;
    let mut code = String::new();
    std::io::stdin()
        .read_line(&mut code)
        .map_err(|error| format!("failed to read verification code: {error}"))?;
    Ok(code.trim().to_string())
}

fn cmd_channel_pair(args: &[String], home: &Path) {
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let options = parse_channel_pair_options(args);
    let platforms: Vec<&str> = options.platform.as_deref().map_or_else(
        || vec!["telegram", "whatsapp", "qq", "qclaw"],
        |platform| vec![platform],
    );

    for platform in platforms {
        let store =
            cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));
        eprintln!("=== {platform} ===");
        let paired = store.paired_users();
        let pending = store.pending_pairs();

        if pending.is_empty() {
            eprintln!("  No pending pair requests.");
        } else {
            eprintln!("  Pending ({}):", pending.len());
            for pending_pair in &pending {
                eprintln!(
                    "    User: {} ({}) -- Code: {} -- {}",
                    pending_pair.user_id,
                    pending_pair.user_name,
                    pending_pair.code,
                    pending_pair.created_at
                );
            }
        }
        eprintln!("  Paired ({}):", paired.len());
        for paired_user in &paired {
            eprintln!(
                "    {} ({}) -- since {} -- subscription: {}",
                paired_user.user_id,
                paired_user.name,
                format_paired_at(&paired_user.paired_at),
                if paired_user.subscribe {
                    "enabled"
                } else {
                    "disabled"
                }
            );
        }
    }
}

fn parse_channel_pair_options(args: &[String]) -> ChannelPairOptions {
    let mut platform = None;
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "--id" => {
                let _ = iter.next();
            }
            other if other.starts_with("--") => {}
            other => {
                if platform.is_none() {
                    platform = Some(other.to_string());
                }
            }
        }
    }
    ChannelPairOptions { platform }
}

fn cmd_channel_subscription(
    command_args: &[String],
    args: &[String],
    home: &Path,
    subscribe: bool,
) {
    if args.len() < 2 {
        let scope = if subscribe {
            "subscribe"
        } else {
            "unsubscribe"
        };
        print_usage_line(&channel_action_usage(scope, &["<platform>", "<user_id>"]));
        eprintln!("  platform: telegram|whatsapp|qq|qclaw");
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));
    match store.set_pair_subscription(user_id, subscribe) {
        Ok(user) => {
            crate::deploy::reload_running_daemon_config(command_args);
            eprintln!(
                "Channel subscription {} for {platform} user {} ({}). If the daemon is running, this applies shortly.",
                if subscribe { "enabled" } else { "disabled" },
                user.user_id,
                user.name
            );
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PairedUserNotFound(_)) => {
            eprintln!("Paired user {user_id} not found on {platform}.");
        }
        Err(err) => eprintln!("Failed to update subscription for {user_id} on {platform}: {err}"),
    }
}

fn format_paired_at(raw: &str) -> String {
    let secs_str = raw.trim_end_matches('s');
    let Ok(secs) = secs_str.parse::<u64>() else {
        return raw.to_string();
    };
    let ts = std::time::UNIX_EPOCH + std::time::Duration::from_secs(secs);
    let dt: chrono::DateTime<chrono::Local> = ts.into();
    dt.format("%Y-%m-%d %H:%M").to_string()
}

fn cmd_channel_approve(command_args: &[String], args: &[String], home: &Path) {
    if args.len() < 2 {
        print_usage_line(&channel_action_usage(
            "approve",
            &["<platform>", "<user_id>", "[--subscribe|--no-subscribe]"],
        ));
        eprintln!("  platform: telegram|whatsapp|qq|qclaw");
        eprintln!("  user_id:  the user's platform ID (shown in 'cortex channel pair')");
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let subscribe = parse_subscription_flag(&args[2..]);
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let dir = paths.channel_dir(platform);
    let store = cortex_runtime::channels::store::ChannelStore::open_dir(dir.clone());

    if !dir.exists() {
        eprintln!("No channel directory for '{platform}'. Is the channel configured?");
        return;
    }
    match store.approve_pending_pair(user_id) {
        Ok(user) => {
            eprintln!("Approved: {} ({}) on {platform}.", user.user_id, user.name);
            eprintln!("The user can now chat. (Takes effect immediately, no restart needed.)");
            if let Some(enabled) = subscribe {
                match store.set_pair_subscription(user_id, enabled) {
                    Ok(updated) => {
                        crate::deploy::reload_running_daemon_config(command_args);
                        eprintln!(
                            "Channel subscription {} for {platform} user {} ({}). If the daemon is running, this applies shortly.",
                            if enabled { "enabled" } else { "disabled" },
                            updated.user_id,
                            updated.name
                        );
                    }
                    Err(err) => eprintln!(
                        "Approved user, but failed to update subscription for {user_id} on {platform}: {err}"
                    ),
                }
            }
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::AlreadyPaired(_)) => {
            eprintln!("User {user_id} is already paired on {platform}.");
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PendingUserNotFound(_)) => {
            eprintln!("Pending pair request not found for {user_id} on {platform}.");
        }
        Err(err) => eprintln!("Failed to approve {user_id} on {platform}: {err}"),
    }
}

fn parse_subscription_flag(args: &[String]) -> Option<bool> {
    args.iter().find_map(|arg| match arg.as_str() {
        "--subscribe" => Some(true),
        "--no-subscribe" => Some(false),
        _ => None,
    })
}

fn cmd_channel_revoke(args: &[String], home: &Path) {
    if args.len() < 2 {
        print_usage_line(&channel_action_usage(
            "revoke",
            &["<platform>", "<user_id>"],
        ));
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));
    if !store.revoke_pair(user_id) {
        eprintln!("User {user_id} not found in paired users on {platform}.");
        return;
    }
    eprintln!("Revoked: {user_id} on {platform}. Takes effect immediately.");
}

fn cmd_channel_list_op(args: &[String], home: &Path, list: PolicyListKind, add: bool) {
    if args.len() < 2 {
        let command = if add {
            format!("allow-{}", list.as_str())
        } else {
            format!("deny-{}", list.as_str())
        };
        print_usage_line(&channel_action_usage(
            &command,
            &["<platform>", "<user_id>"],
        ));
        return;
    }
    let platform = args[0].as_str();
    let user_id = &args[1];
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));

    match store.mutate_policy_list(list.store_list(), user_id, add) {
        Ok(_) => {
            let action = if add { "Added" } else { "Removed" };
            eprintln!("{action} {user_id} {} on {platform}.", list.as_str());
            eprintln!("Takes effect immediately, no restart needed.");
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PolicyEntryExists { .. }) => {
            eprintln!("{user_id} already in {} on {platform}.", list.as_str());
        }
        Err(cortex_runtime::channels::store::ChannelStoreError::PolicyEntryMissing { .. }) => {
            eprintln!("{user_id} not found in {} on {platform}.", list.as_str());
        }
        Err(err) => eprintln!("Failed to update {} on {platform}: {err}", list.as_str()),
    }
}

fn cmd_channel_policy(args: &[String], home: &Path) {
    if args.is_empty() {
        print_usage_line(&channel_action_usage("policy", &["<platform>", "[mode]"]));
        eprintln!("  Modes: pairing (default), whitelist, open");
        return;
    }
    let platform = args[0].as_str();
    let paths = cortex_kernel::CortexPaths::from_instance_home(home);
    let store =
        cortex_runtime::channels::store::ChannelStore::open_dir(paths.channel_dir(platform));

    if let Some(new_mode) = args.get(1) {
        match store.update_policy_mode(new_mode) {
            Ok(_) => {
                eprintln!("Policy for {platform} set to '{new_mode}'. Takes effect immediately.");
            }
            Err(cortex_runtime::channels::store::ChannelStoreError::InvalidPolicyMode(_)) => {
                eprintln!("Invalid mode '{new_mode}'. Use: pairing, whitelist, open");
            }
            Err(err) => eprintln!("Failed to update policy for {platform}: {err}"),
        }
    } else {
        let policy = store.policy();
        let whitelist_count = policy.whitelist.len();
        let blacklist_count = policy.blacklist.len();
        eprintln!("{platform} policy:");
        eprintln!("  mode: {}", policy.mode);
        eprintln!("  whitelist: {whitelist_count} user(s)");
        eprintln!("  blacklist: {blacklist_count} user(s)");
        if whitelist_count > 0 {
            for user in &policy.whitelist {
                eprintln!("    + {user}");
            }
        }
        if blacklist_count > 0 {
            for user in &policy.blacklist {
                eprintln!("    - {user}");
            }
        }
    }
}
