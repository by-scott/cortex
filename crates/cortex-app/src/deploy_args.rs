/// Parse `--system` flag from argument list.
#[must_use]
pub fn parse_system_flag(args: &[String]) -> bool {
    args.iter().any(|a| a == "--system")
}

/// Parse `--id <ID>` from argument list.
#[must_use]
pub fn parse_instance_id(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--id")
        .and_then(|i| args.get(i + 1))
        .cloned()
}

/// Parse `--home <PATH>` from argument list.
#[must_use]
pub fn parse_home_arg(args: &[String]) -> Option<String> {
    args.iter()
        .position(|a| a == "--home")
        .and_then(|i| args.get(i + 1))
        .cloned()
}
