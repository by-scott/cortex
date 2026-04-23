use cortex_runtime::PermissionGate;
use cortex_types::{PermissionDecision, RiskLevel};
use std::io::{self, Write};

/// Interactive permission gate that prompts the user via terminal.
pub struct InteractivePermissionGate;

impl PermissionGate for InteractivePermissionGate {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        match risk_level {
            RiskLevel::Allow => PermissionDecision::Approved,
            RiskLevel::Block => {
                eprintln!("[BLOCKED] Tool '{tool_name}' is blocked (risk too high)");
                PermissionDecision::Denied
            }
            RiskLevel::Review | RiskLevel::RequireConfirmation => {
                eprint!("[CONFIRM] Allow tool '{tool_name}' (risk: {risk_level:?})? (y/n): ");
                io::stderr().flush().ok();

                let mut input = String::new();
                match io::stdin().read_line(&mut input) {
                    Ok(_) => {
                        if input.trim().eq_ignore_ascii_case("y") {
                            PermissionDecision::Approved
                        } else {
                            PermissionDecision::Denied
                        }
                    }
                    Err(_) => PermissionDecision::Denied,
                }
            }
        }
    }
}
