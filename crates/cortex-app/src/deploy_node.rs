#[derive(Debug, Clone, PartialEq, Eq)]
struct NestedInvocation<'a> {
    subcommand: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CommandSpec<T> {
    subcommand: T,
    names: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum NodeSubcommand {
    Setup,
    Status,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserSubcommand {
    Enable,
    Disable,
    Status,
}

const NODE_SUBCOMMAND_SPECS: &[CommandSpec<NodeSubcommand>] = &[
    CommandSpec {
        subcommand: NodeSubcommand::Setup,
        names: &["setup"],
    },
    CommandSpec {
        subcommand: NodeSubcommand::Status,
        names: &["status"],
    },
];

const BROWSER_SUBCOMMAND_SPECS: &[CommandSpec<BrowserSubcommand>] = &[
    CommandSpec {
        subcommand: BrowserSubcommand::Enable,
        names: &["enable"],
    },
    CommandSpec {
        subcommand: BrowserSubcommand::Disable,
        names: &["disable"],
    },
    CommandSpec {
        subcommand: BrowserSubcommand::Status,
        names: &["status"],
    },
];

/// `cortex node setup|status`
///
/// # Errors
/// Returns an error string if setup cannot install or update the local Node.js
/// toolchain.
pub fn cmd_node(args: &[String]) -> Result<(), String> {
    let paths = crate::deploy::resolve_paths_from_args(args);
    let data_dir = paths.data_dir();

    match parse_node_subcommand(parse_nested_invocation(args, "node").subcommand)? {
        NodeSubcommand::Setup => crate::node_manager::cmd_node_setup(&data_dir),
        NodeSubcommand::Status => {
            crate::node_manager::cmd_node_status(&data_dir);
            Ok(())
        }
    }
}

/// `cortex browser enable|disable|status`
///
/// # Errors
/// Returns an error string if browser integration files cannot be written or
/// removed.
pub fn cmd_browser(args: &[String]) -> Result<(), String> {
    let paths = crate::deploy::resolve_paths_from_args(args);
    let home = paths.instance_home();
    let data_dir = paths.data_dir();

    match parse_browser_subcommand(parse_nested_invocation(args, "browser").subcommand)? {
        BrowserSubcommand::Enable => {
            crate::node_manager::cmd_browser_enable(args, &home, &data_dir)
        }
        BrowserSubcommand::Disable => crate::node_manager::cmd_browser_disable(args, &home),
        BrowserSubcommand::Status => {
            crate::node_manager::cmd_browser_status(&home, &data_dir);
            Ok(())
        }
    }
}

fn parse_nested_invocation<'a>(args: &'a [String], root: &str) -> NestedInvocation<'a> {
    let root_pos = args.iter().position(|arg| arg == root);
    let after_root = root_pos.map_or(args, |pos| &args[pos + 1..]);

    let mut index = 0usize;
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
        return NestedInvocation {
            subcommand: Some(arg),
        };
    }

    NestedInvocation { subcommand: None }
}

fn parse_node_subcommand(subcommand: Option<&str>) -> Result<NodeSubcommand, String> {
    let Some(subcommand) = subcommand else {
        return Ok(NodeSubcommand::Status);
    };
    NODE_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
        .ok_or_else(|| unknown_subcommand_error("node", subcommand, NODE_SUBCOMMAND_SPECS))
}

fn parse_browser_subcommand(subcommand: Option<&str>) -> Result<BrowserSubcommand, String> {
    let Some(subcommand) = subcommand else {
        return Ok(BrowserSubcommand::Status);
    };
    BROWSER_SUBCOMMAND_SPECS
        .iter()
        .find(|spec| spec.names.contains(&subcommand))
        .map(|spec| spec.subcommand)
        .ok_or_else(|| unknown_subcommand_error("browser", subcommand, BROWSER_SUBCOMMAND_SPECS))
}

fn unknown_subcommand_error<T>(root: &str, subcommand: &str, specs: &[CommandSpec<T>]) -> String
where
    T: Copy,
{
    let choices = specs
        .iter()
        .map(|spec| spec.names[0])
        .collect::<Vec<_>>()
        .join(", ");
    format!("unknown {root} command: {subcommand}. Use: {choices}")
}
