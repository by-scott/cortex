#![forbid(unsafe_code)]

use cortex_types::{ActorId, AuthContext, ClientId, DeploymentPlan, DeploymentStep, TenantId};

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let command = args.first().map_or("version", String::as_str);
    match command {
        "status" => print_status(),
        "release-plan" => print_release_plan(),
        "version" | "--version" | "-V" => print_version(),
        "help" | "--help" | "-h" => print_help(),
        _ => {
            eprintln!("unknown command: {command}");
            print_help();
            std::process::exit(2);
        }
    }
}

fn print_version() {
    println!("cortex {}", env!("CARGO_PKG_VERSION"));
}

fn print_status() {
    println!("Cortex {}", env!("CARGO_PKG_VERSION"));
    println!("line: 1.5 full rewrite");
    println!("gate: docker strict rust:latest, fmt, clippy pedantic/nursery, tests");
    println!("multi-user: tenant/actor/client/session ownership");
    println!("runtime: journal recovery, first-turn session reuse, active-session delivery");
    println!("rag: query-scope auth, corpus ACL, BM25 lexical scoring, taint blocking");
    println!("plugins: capability authorization, host-path denial, output limits");
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
    println!("usage: cortex [version|status|release-plan|help]");
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
