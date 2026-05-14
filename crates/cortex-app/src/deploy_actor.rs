#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActorSubcommand {
    Alias,
    Transport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BindingAction {
    List,
    Set,
    Unset,
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
struct ActorInvocation<'a> {
    subcommand: Option<&'a str>,
    remaining: &'a [String],
}

const ACTOR_SUBCOMMAND_SPECS: &[CommandSpec<ActorSubcommand>] = &[
    CommandSpec {
        subcommand: ActorSubcommand::Alias,
        names: &["alias"],
        summary: "List or change actor aliases",
    },
    CommandSpec {
        subcommand: ActorSubcommand::Transport,
        names: &["transport"],
        summary: "List or change transport actor bindings",
    },
];

const ACTOR_DETAIL_SPECS: &[DetailUsageSpec] = &[
    DetailUsageSpec {
        usage: "alias list",
        summary: "List actor aliases",
    },
    DetailUsageSpec {
        usage: "alias set <from> <to>",
        summary: "Map one actor to a canonical actor",
    },
    DetailUsageSpec {
        usage: "alias unset <from>",
        summary: "Remove an actor alias",
    },
    DetailUsageSpec {
        usage: "transport list",
        summary: "List transport actor bindings",
    },
    DetailUsageSpec {
        usage: "transport set <name|all> <actor>",
        summary: "Bind transport(s) to actor",
    },
    DetailUsageSpec {
        usage: "transport unset <name>",
        summary: "Remove transport binding",
    },
];

const BINDING_ACTION_SPECS: &[CommandSpec<BindingAction>] = &[
    CommandSpec {
        subcommand: BindingAction::List,
        names: &["list"],
        summary: "List current bindings",
    },
    CommandSpec {
        subcommand: BindingAction::Set,
        names: &["set"],
        summary: "Create or update a binding",
    },
    CommandSpec {
        subcommand: BindingAction::Unset,
        names: &["unset"],
        summary: "Remove a binding",
    },
];

pub fn cmd_actor(args: &[String]) {
    let paths = crate::deploy::resolve_paths_from_args(args);
    let store = cortex_kernel::ActorBindingsStore::from_paths(&paths);
    let invocation = parse_actor_invocation(args);
    let remaining = invocation.remaining;

    let changed = match parse_actor_subcommand(invocation.subcommand) {
        Some(ActorSubcommand::Alias) => cmd_actor_alias(remaining, &store),
        Some(ActorSubcommand::Transport) => cmd_actor_transport(remaining, &store),
        None => {
            print_actor_usage();
            false
        }
    };
    if changed {
        crate::deploy::reload_running_daemon_config(args);
    }
}

fn parse_actor_invocation(args: &[String]) -> ActorInvocation<'_> {
    let root_pos = args.iter().position(|arg| arg == "actor");
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
        return ActorInvocation {
            subcommand: Some(arg),
            remaining: &after_root[index + 1..],
        };
    }

    ActorInvocation {
        subcommand: None,
        remaining: &[],
    }
}

fn parse_actor_subcommand(subcommand: Option<&str>) -> Option<ActorSubcommand> {
    let subcommand = subcommand?;
    ACTOR_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
}

fn parse_binding_action(action: Option<&str>) -> Option<BindingAction> {
    let action = action?;
    BINDING_ACTION_SPECS
        .iter()
        .find(|spec| spec.names.contains(&action))
        .map(|spec| spec.subcommand)
}

fn print_actor_usage() {
    eprintln!("Usage: cortex actor <subcommand>");
    eprintln!();
    eprintln!("Identity mapping for unified session ownership.");
    for spec in ACTOR_SUBCOMMAND_SPECS {
        eprintln!("  {:<28} {}", spec.names[0], spec.summary);
    }
    for spec in ACTOR_DETAIL_SPECS {
        eprintln!("  {:<28} {}", spec.usage, spec.summary);
    }
}

fn print_usage_line(usage: &str) {
    eprintln!("Usage: {usage}");
}

fn actor_action_usage(scope: &str, required_args: &[&str]) -> String {
    let suffix = if required_args.is_empty() {
        String::new()
    } else {
        format!(" {}", required_args.join(" "))
    };
    format!("cortex actor {scope}{suffix}")
}

fn cmd_actor_alias(args: &[String], store: &cortex_kernel::ActorBindingsStore) -> bool {
    let Some(action) = parse_binding_action(args.first().map(String::as_str)) else {
        print_actor_usage();
        return false;
    };
    match action {
        BindingAction::List => {
            list_bindings(store.actor_aliases(), "Actor aliases");
            false
        }
        BindingAction::Set => {
            if args.len() < 3 {
                print_usage_line(&actor_action_usage("alias set", &["<from>", "<to>"]));
                return false;
            }
            store.set_actor_alias(&args[1], &args[2]);
            eprintln!("Actor alias set: {} -> {}", args[1], args[2]);
            true
        }
        BindingAction::Unset => {
            if args.len() < 2 {
                print_usage_line(&actor_action_usage("alias unset", &["<from>"]));
                return false;
            }
            if store.remove_actor_alias(&args[1]) {
                eprintln!("Actor alias removed: {}", args[1]);
                true
            } else {
                eprintln!("Actor alias not found: {}", args[1]);
                false
            }
        }
    }
}

fn cmd_actor_transport(args: &[String], store: &cortex_kernel::ActorBindingsStore) -> bool {
    let Some(action) = parse_binding_action(args.first().map(String::as_str)) else {
        print_actor_usage();
        return false;
    };
    match action {
        BindingAction::List => {
            list_bindings(store.transport_actors(), "Transport actor bindings");
            false
        }
        BindingAction::Set => {
            if args.len() < 3 {
                print_usage_line(&actor_action_usage(
                    "transport set",
                    &["<name|all>", "<actor>"],
                ));
                return false;
            }
            let name = &args[1];
            let actor = &args[2];
            if name == "all" || name == "*" {
                for transport in &["http", "rpc", "ws", "sock", "stdio"] {
                    store.set_transport_actor(transport, actor);
                }
                eprintln!("All transports bound to {actor}");
            } else {
                store.set_transport_actor(name, actor);
                eprintln!("Transport binding set: {name} -> {actor}");
            }
            true
        }
        BindingAction::Unset => {
            if args.len() < 2 {
                print_usage_line(&actor_action_usage("transport unset", &["<name>"]));
                return false;
            }
            if store.remove_transport_actor(&args[1]) {
                eprintln!("Transport binding removed: {}", args[1]);
                true
            } else {
                eprintln!("Transport binding not found: {}", args[1]);
                false
            }
        }
    }
}

fn list_bindings(map: std::collections::BTreeMap<String, String>, label: &str) {
    eprintln!("{label}:");
    if map.is_empty() {
        eprintln!("  (empty)");
        return;
    }
    for (key, value) in map {
        eprintln!("  {key} -> {value}");
    }
}
