#![forbid(unsafe_code)]

use std::path::PathBuf;

use cortex_runtime::{DaemonConfig, DaemonRequest, DaemonResponse, DaemonServer, send_request};
use cortex_types::{
    ActorId, AuthContext, ClientId, DeploymentPlan, DeploymentStep, TenantId, TransportCapabilities,
};

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(2);
    }
}

fn run() -> Result<(), String> {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map_or("version", String::as_str);
    match command {
        "daemon" => run_daemon(&args[1..]),
        "status" => print_status(&args[1..]),
        "send" => submit_message(&args[1..]),
        "register-tenant" => register_tenant(&args[1..]),
        "bind-client" => bind_client(&args[1..]),
        "stop" => stop_daemon(&args[1..]),
        "release-plan" => {
            print_release_plan();
            Ok(())
        }
        "version" | "--version" | "-V" => {
            print_version();
            Ok(())
        }
        "help" | "--help" | "-h" => {
            print_help();
            Ok(())
        }
        _ => {
            print_help();
            Err(format!("unknown command: {command}"))
        }
    }
}

fn run_daemon(args: &[String]) -> Result<(), String> {
    let config = daemon_config(args);
    DaemonServer::open(config)
        .and_then(DaemonServer::serve)
        .map_err(|error| format!("daemon failed: {error:?}"))
}

fn print_version() {
    println!("cortex {}", env!("CARGO_PKG_VERSION"));
}

fn print_status(args: &[String]) -> Result<(), String> {
    let socket_path = socket_path(args);
    if socket_path.exists() {
        match request(&socket_path, &DaemonRequest::Status)? {
            DaemonResponse::Status { status } => {
                println!("Cortex {}", status.version);
                println!("line: daemon-first 1.5 full rewrite runtime");
                println!("socket: {}", status.socket_path);
                println!("tenants: {}", status.tenants);
                println!("clients: {}", status.clients);
                println!("sessions: {}", status.sessions);
                println!("persistent: {}", status.persistent);
                if let Some(journal_mode) = status.journal_mode {
                    println!("journal_mode: {journal_mode}");
                }
                if let Some(pages) = status.wal_autocheckpoint_pages {
                    println!("wal_autocheckpoint_pages: {pages}");
                }
                return Ok(());
            }
            other => return Err(format!("unexpected daemon response: {other:?}")),
        }
    }
    print_offline_status();
    Ok(())
}

fn print_offline_status() {
    println!("Cortex {}", env!("CARGO_PKG_VERSION"));
    println!("line: daemon-first 1.5 full rewrite runtime");
    println!("gate: docker strict rust:latest, fmt, clippy pedantic/nursery, tests");
    println!("daemon: unix socket RPC, SQLite state, journal recovery");
    println!("multi-user: tenant/actor/client/session ownership");
    println!("runtime: first-turn session reuse, active-session delivery");
    println!("rag: query-scope auth, corpus ACL, BM25 lexical scoring, taint blocking");
    println!("plugins: capability authorization, host-path denial, output limits");
}

fn submit_message(args: &[String]) -> Result<(), String> {
    let input = positional_text(args).ok_or_else(|| "send requires text".to_string())?;
    let context = context_from_args(args);
    match request(
        socket_path(args),
        &DaemonRequest::SubmitUserMessage { context, input },
    )? {
        DaemonResponse::SubmittedTurn { turn, usage } => {
            println!("session: {}", turn.session_id.as_str());
            println!("turn: {}", turn.turn_id.as_str());
            println!("tokens: {}", usage.total());
            Ok(())
        }
        DaemonResponse::Error { message } => Err(message),
        other => Err(format!("unexpected daemon response: {other:?}")),
    }
}

fn register_tenant(args: &[String]) -> Result<(), String> {
    let tenant_id =
        TenantId::from_raw(option_value(args, "--tenant").unwrap_or_else(|| "default".into()));
    let name = option_value(args, "--name").unwrap_or_else(|| tenant_id.as_str().to_string());
    match request(
        socket_path(args),
        &DaemonRequest::RegisterTenant { tenant_id, name },
    )? {
        DaemonResponse::Ack => {
            println!("ok");
            Ok(())
        }
        DaemonResponse::Error { message } => Err(message),
        other => Err(format!("unexpected daemon response: {other:?}")),
    }
}

fn bind_client(args: &[String]) -> Result<(), String> {
    let context = context_from_args(args);
    let max_chars = option_value(args, "--max-chars")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(4_096);
    match request(
        socket_path(args),
        &DaemonRequest::BindClient {
            context,
            capabilities: TransportCapabilities::plain(max_chars),
        },
    )? {
        DaemonResponse::Ack => {
            println!("ok");
            Ok(())
        }
        DaemonResponse::Error { message } => Err(message),
        other => Err(format!("unexpected daemon response: {other:?}")),
    }
}

fn stop_daemon(args: &[String]) -> Result<(), String> {
    match request(socket_path(args), &DaemonRequest::Shutdown)? {
        DaemonResponse::Ack => {
            println!("ok");
            Ok(())
        }
        DaemonResponse::Error { message } => Err(message),
        other => Err(format!("unexpected daemon response: {other:?}")),
    }
}

fn print_release_plan() {
    let context = AuthContext::new(
        TenantId::from_static("release-tenant"),
        ActorId::from_static("operator"),
        ClientId::from_static("cli"),
    );
    let plan = DeploymentPlan::production_release(cortex_types::OwnedScope::private_for(&context));
    println!("Cortex {} release plan", env!("CARGO_PKG_VERSION"));
    for record in plan.records {
        println!("- {}", step_label(record.step));
    }
}

fn print_help() {
    println!(
        "usage: cortex [daemon|status|send|register-tenant|bind-client|stop|version|release-plan|help]"
    );
    println!("common options: --socket PATH --data-dir PATH --tenant ID --actor ID --client ID");
}

fn daemon_config(args: &[String]) -> DaemonConfig {
    DaemonConfig::new(data_dir(args), socket_path(args))
}

fn data_dir(args: &[String]) -> PathBuf {
    option_value(args, "--data-dir").map_or_else(default_data_dir, PathBuf::from)
}

fn socket_path(args: &[String]) -> PathBuf {
    option_value(args, "--socket").map_or_else(default_socket_path, PathBuf::from)
}

fn default_data_dir() -> PathBuf {
    std::env::var_os("CORTEX_DATA_DIR").map_or_else(
        || {
            home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".local/share/cortex")
        },
        PathBuf::from,
    )
}

fn default_socket_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CORTEX_SOCKET") {
        return PathBuf::from(path);
    }
    std::env::var_os("XDG_RUNTIME_DIR").map_or_else(
        || default_data_dir().join("cortex.sock"),
        |runtime_dir| PathBuf::from(runtime_dir).join("cortex.sock"),
    )
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn context_from_args(args: &[String]) -> AuthContext {
    AuthContext::new(
        TenantId::from_raw(option_value(args, "--tenant").unwrap_or_else(|| "default".into())),
        ActorId::from_raw(option_value(args, "--actor").unwrap_or_else(|| "local".into())),
        ClientId::from_raw(option_value(args, "--client").unwrap_or_else(|| "cli".into())),
    )
}

fn positional_text(args: &[String]) -> Option<String> {
    let mut index = 0_usize;
    let mut values = Vec::new();
    while index < args.len() {
        if args[index].starts_with("--") {
            index += 2;
        } else {
            values.push(args[index].clone());
            index += 1;
        }
    }
    if values.is_empty() {
        None
    } else {
        Some(values.join(" "))
    }
}

fn option_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

fn request(
    socket_path: impl Into<PathBuf>,
    request: &DaemonRequest,
) -> Result<DaemonResponse, String> {
    let socket_path = socket_path.into();
    send_request(&socket_path, request).map_err(|error| {
        format!(
            "daemon request failed at {}: {error:?}",
            socket_path.display()
        )
    })
}

const fn step_label(step: DeploymentStep) -> &'static str {
    match step {
        DeploymentStep::Backup => "backup",
        DeploymentStep::Migrate => "migrate",
        DeploymentStep::Install => "install",
        DeploymentStep::SmokeTest => "smoke-test",
        DeploymentStep::Package => "package",
        DeploymentStep::Publish => "publish",
    }
}
