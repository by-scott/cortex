#![warn(clippy::pedantic, clippy::nursery)]

pub mod auth;
pub mod cli;
pub mod deploy;
pub mod node_manager;
pub mod permission;
pub mod plugin_loader;
pub mod plugin_manager;
pub mod scaffold;

#[cfg(test)]
mod tests;

use cortex_runtime::{CortexRuntime, DaemonClient, PluginRegistry, StreamEvent};
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use std::io::{self, BufRead, Write};

use cli::{
    RunMode, check_exclusive_modes, detect_pipe_prompt, handle_early_args, parse_arg_value,
    validate_instance_id,
};

// ── Logging ──────────────────────────────────────────────────

fn init_logging() {
    let json = std::env::var("CORTEX_LOG_FORMAT").is_ok_and(|v| v == "json");
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    if json {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(filter)
            .init();
    } else {
        tracing_subscriber::fmt()
            .compact()
            .with_env_filter(filter)
            .init();
    }
}

// ── Capability drift + plugins ───────────────────────────────

fn load_plugins(rt: &mut CortexRuntime) {
    let cfg = rt.config().plugins.clone();
    let home = rt.home().to_path_buf();
    let mut pr = PluginRegistry::new();
    let (mut loaded, warnings) = plugin_loader::load_plugins(&home, &cfg, &mut pr, rt.tools_mut());
    for w in &warnings {
        tracing::warn!(plugin_warning = %w, "plugin loading warning");
    }

    if !loaded.manifests.is_empty() {
        tracing::info!(
            count = loaded.manifests.len(),
            libraries = loaded.library_count(),
            skill_dirs = loaded.skill_dirs.len(),
            prompt_dirs = loaded.prompt_dirs.len(),
            "plugins loaded"
        );
    }

    // Store plugin skill directories for the daemon to load later.
    rt.plugin_skill_dirs = std::mem::take(&mut loaded.skill_dirs);
    // Keep plugin shared libraries alive to prevent vtable invalidation.
    rt.plugin_libraries = loaded.libraries;
}

fn init_and_prepare(rt: &mut CortexRuntime) {
    check_api_key_configured(rt.config(), rt.home());
    load_plugins(rt);
}

// ── stdio bridges ───────────────────────────────────────────

fn mcp_method_to_rpc(method: &str) -> String {
    match method {
        "initialize" => "mcp/initialize".into(),
        "tools/list" => "mcp/tools-list".into(),
        "tools/call" => "mcp/tools-call".into(),
        "prompts/list" => "mcp/prompts-list".into(),
        "prompts/get" => "mcp/prompts-get".into(),
        "notifications/initialized" | "initialized" => "mcp/initialized".into(),
        other => format!("mcp/{other}"),
    }
}

fn run_stdio_bridge(client: &DaemonClient, is_mcp: bool) {
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let Ok(req) = serde_json::from_str::<serde_json::Value>(trimmed) else {
            let err = serde_json::json!({"jsonrpc":"2.0","id":null,
                "error":{"code":-32700,"message":"Parse error"}});
            if let Ok(j) = serde_json::to_string(&err) {
                let _ = writeln!(stdout, "{j}");
                let _ = stdout.flush();
            }
            continue;
        };

        let method = req
            .get("method")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("");
        let params = req
            .get("params")
            .cloned()
            .unwrap_or_else(|| serde_json::json!({}));
        let id = req.get("id").cloned().unwrap_or(serde_json::Value::Null);

        // JSON-RPC 2.0: notifications (no id) are fire-and-forget.
        let is_notification = id.is_null();

        let rpc = if is_mcp {
            mcp_method_to_rpc(method)
        } else {
            method.to_owned()
        };

        let result = if !is_mcp && method == "session/prompt" {
            let session_id = params
                .get("session_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");
            let prompt = params
                .get("prompt")
                .or_else(|| params.get("input"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or("");

            if prompt.trim().is_empty() {
                Err(cortex_runtime::ClientError::RpcError {
                    code: -32602,
                    message: "missing prompt parameter".into(),
                })
            } else {
                client.prompt(session_id, prompt, None).map(|response| {
                    serde_json::json!({
                        "session_id": session_id,
                        "response": response,
                    })
                })
            }
        } else {
            client.send_rpc(&rpc, &params)
        };

        // JSON-RPC 2.0: notifications (no id) are fire-and-forget — no response.
        if is_notification {
            continue;
        }

        let response = match result {
            Ok(r) => serde_json::json!({"jsonrpc":"2.0","id":id,"result":r}),
            Err(e) => serde_json::json!({"jsonrpc":"2.0","id":id,
                "error":{"code":-32603,"message":e.to_string()}}),
        };
        if let Ok(j) = serde_json::to_string(&response) {
            let _ = writeln!(stdout, "{j}");
            let _ = stdout.flush();
        }
    }
}

// ── Client REPL (daemon-connected) ───────────────────────────

fn run_client_repl(client: &std::sync::Arc<DaemonClient>) {
    let session_id = match client.new_session() {
        Ok(id) => id,
        Err(e) => {
            eprintln!("Failed to create session: {e}");
            return;
        }
    };
    eprintln!("Cortex v{} -- client mode", env!("CARGO_PKG_VERSION"));
    eprintln!(
        "Session: {}\nType /help for commands, /quit to exit\n",
        &session_id[..session_id.len().min(12)]
    );
    let mut rl = new_editor();

    // Channel for background turn completion notifications.
    let (done_tx, done_rx) = std::sync::mpsc::channel::<Result<String, String>>();
    let turn_active = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));

    loop {
        // Drain any completed background turn results before prompting.
        for result in done_rx.try_iter() {
            turn_active.store(false, std::sync::atomic::Ordering::Relaxed);
            match result {
                Ok(r) if r.is_empty() => {}
                Ok(_) => {} // already streamed
                Err(e) => eprintln!("[ERROR] {e}"),
            }
        }

        let prompt_str = if turn_active.load(std::sync::atomic::Ordering::Relaxed) {
            "cortex(busy)> "
        } else {
            "cortex> "
        };

        match rl.readline(prompt_str) {
            Ok(line) => {
                let input = line.trim();
                if input.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(input);
                if input == "/quit" || input == "/exit" {
                    let _ = client.end_session(&session_id);
                    eprintln!("Goodbye.");
                    break;
                }
                if input.starts_with('/') {
                    match client.dispatch_command(input) {
                        Ok(o) => eprintln!("{o}"),
                        Err(e) => eprintln!("[ERROR] {e}"),
                    }
                    continue;
                }

                // Reject new prompts while a turn is running.
                if turn_active.load(std::sync::atomic::Ordering::Relaxed) {
                    eprintln!("A turn is in progress. Use /stop to cancel or wait.");
                    continue;
                }

                // Spawn the turn on a background thread so the REPL stays
                // responsive for /stop, /status, and other commands.
                turn_active.store(true, std::sync::atomic::Ordering::Relaxed);
                let tx = done_tx.clone();
                let bg_client = std::sync::Arc::clone(client);
                let sid = session_id.clone();
                let prompt_text = input.to_string();
                let bg_active = std::sync::Arc::clone(&turn_active);
                std::thread::spawn(move || {
                    let result = bg_client.prompt(
                        &sid,
                        &prompt_text,
                        Some(&mut |event| match event {
                            StreamEvent::Text { content } => {
                                print!("{content}");
                                let _ = io::stdout().flush();
                            }
                            StreamEvent::Done { .. } => println!(),
                            _ => {}
                        }),
                    );
                    bg_active.store(false, std::sync::atomic::Ordering::Relaxed);
                    let _ = tx.send(result.map_err(|e| e.to_string()));
                });
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C: if a turn is active, send /stop.
                if turn_active.load(std::sync::atomic::Ordering::Relaxed) {
                    match client.dispatch_command("/stop") {
                        Ok(o) => eprintln!("{o}"),
                        Err(e) => eprintln!("[ERROR] {e}"),
                    }
                } else {
                    eprintln!("Interrupted. /quit to exit.");
                }
            }
            Err(ReadlineError::Eof) => {
                let _ = client.end_session(&session_id);
                eprintln!("Goodbye.");
                break;
            }
            Err(e) => {
                eprintln!("Readline error: {e}");
                break;
            }
        }
    }
}

fn run_pipe_mode(client: &DaemonClient, prompt: &str, session_id: Option<String>) {
    // Dispatch slash commands to the command registry, not the LLM.
    if prompt.starts_with('/') {
        match client.dispatch_command(prompt) {
            Ok(o) => {
                print!("{o}");
                io::stdout().flush().ok();
            }
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let sid = session_id.unwrap_or_else(|| {
        client.new_session().unwrap_or_else(|e| {
            eprintln!("Failed to create session: {e}");
            std::process::exit(1);
        })
    });
    match client.prompt(
        &sid,
        prompt,
        Some(&mut |event| {
            if let StreamEvent::Text { content } = event {
                print!("{content}");
                let _ = io::stdout().flush();
            }
        }),
    ) {
        Ok(_) => {
            io::stdout().flush().ok();
        }
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    }
}

// ── Path resolution ──────────────────────────────────────────

fn resolve_base(home_arg: Option<&str>) -> std::path::PathBuf {
    home_arg
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var("CORTEX_HOME")
                .ok()
                .map(std::path::PathBuf::from)
        })
        .unwrap_or_else(|| {
            let h = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_owned());
            std::path::PathBuf::from(h).join(".cortex")
        })
}

fn try_connect_daemon(home: &std::path::Path) -> Result<DaemonClient, cortex_runtime::ClientError> {
    DaemonClient::connect_socket(&home.join("data/cortex.sock"))
}

fn suggest_start_or_install() {
    let h = std::env::var("HOME").unwrap_or_default();
    if std::path::PathBuf::from(&h)
        .join(".config/systemd/user/cortex.service")
        .exists()
    {
        eprintln!("Hint: run `cortex start` to start the daemon.");
    } else {
        eprintln!("Hint: run `cortex install` or `cortex --daemon`.");
    }
}

fn check_api_key_configured(config: &cortex_types::config::CortexConfig, home: &std::path::Path) {
    if !config.api.api_key.is_empty() {
        return;
    }
    let config_path = cortex_kernel::ConfigFileSet::from_paths(
        &cortex_kernel::CortexPaths::from_instance_home(home),
    )
    .config;
    eprintln!(
        "Warning: no API Key configured.\n\
         Edit {} [api].api_key field.\n",
        config_path.display()
    );
}

// No interactive wizard. First run: load_config() auto-generates a full
// default config.toml with all fields. Users configure by editing the file
// or setting CORTEX_API_KEY environment variable.

fn new_editor() -> DefaultEditor {
    DefaultEditor::new().unwrap_or_else(|e| {
        eprintln!("Error: terminal init failed: {e}");
        std::process::exit(1);
    })
}

// ── Entrypoint ───────────────────────────────────────────────

pub async fn run() {
    let args: Vec<String> = std::env::args().collect();
    let home_arg = parse_arg_value(&args, "--home");

    handle_early_args(&args);
    init_logging();

    let mode = check_exclusive_modes(&args);
    let instance_id = parse_arg_value(&args, "--id");
    // Validate instance ID before any operations
    if let Some(ref raw_id) = instance_id
        && let Err(e) = validate_instance_id(raw_id)
    {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
    let base = resolve_base(home_arg.as_deref());
    let id = instance_id.as_deref().unwrap_or("default");
    let home = base.join(id);

    if matches!(&mode, RunMode::Repl) {
        // CLI argument prompt: cortex "hello"
        // Stdin pipe: echo "hello" | cortex
        // Combined: echo "data" | cortex "instruction" → "data\n\ninstruction"
        let arg_prompt = detect_pipe_prompt(&args);
        let stdin_content = (|| {
            use std::io::IsTerminal as _;
            if std::io::stdin().is_terminal() {
                return None;
            }
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf).ok()?;
            let trimmed = buf.trim().to_string();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        })();
        let prompt = match (arg_prompt, stdin_content) {
            (Some(arg), Some(pipe)) => Some(format!("{pipe}\n\n{arg}")),
            (Some(arg), None) => Some(arg),
            (None, Some(pipe)) => Some(pipe),
            (None, None) => None,
        };
        if let Some(prompt) = prompt.filter(|p| !p.trim().is_empty()) {
            if let Ok(c) = try_connect_daemon(&home) {
                run_pipe_mode(&c, &prompt, parse_arg_value(&args, "--session"));
                return;
            }
            eprintln!("Error: daemon is not running.");
            suggest_start_or_install();
            std::process::exit(1);
        }
    }

    if matches!(&mode, RunMode::Acp | RunMode::McpServer) {
        if let Ok(c) = try_connect_daemon(&home) {
            let is_mcp = matches!(&mode, RunMode::McpServer);
            // ACP: methods are native RPC (session/initialize, session/prompt) — pass through.
            // MCP: methods need translation (initialize → mcp/initialize, tools/list → mcp/tools-list).
            run_stdio_bridge(&c, is_mcp);
            return;
        }
        let p = if matches!(&mode, RunMode::Acp) {
            "ACP"
        } else {
            "MCP"
        };
        eprintln!("Error: daemon not running. {p} bridge requires a daemon.");
        suggest_start_or_install();
        std::process::exit(1);
    }

    if let RunMode::Daemon { stdio } = &mode {
        let mut rt = init_runtime(&base, &home).await;
        init_and_prepare(&mut rt);
        let mut cfg = cortex_runtime::DaemonConfig::from_config(rt.config(), rt.home());
        cfg.enable_stdio = *stdio;
        match cortex_runtime::DaemonServer::new(&mut rt, cfg) {
            Ok(server) => server.run().await,
            Err(e) => {
                eprintln!("Error: daemon initialization failed: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    // REPL: connect to daemon (daemon must be running)
    if let Ok(c) = try_connect_daemon(&home) {
        run_client_repl(&std::sync::Arc::new(c));
    } else {
        suggest_start_or_install();
    }
}

async fn init_runtime(base: &std::path::Path, home: &std::path::Path) -> CortexRuntime {
    CortexRuntime::new(base, home).await.unwrap_or_else(|e| {
        eprintln!("Fatal: runtime init failed: {e}");
        std::process::exit(1);
    })
}
