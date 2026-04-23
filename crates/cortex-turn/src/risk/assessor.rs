use cortex_types::{RiskLevel, RiskScore};

/// Stateless risk assessor. Scores tool invocations on 4 axes.
pub struct RiskAssessor;

impl RiskAssessor {
    #[must_use]
    pub fn assess(&self, tool_name: &str, input: &serde_json::Value) -> RiskScore {
        let tool_risk = base_tool_risk(tool_name);
        let file_sensitivity = file_sensitivity_score(input);
        let blast_radius = blast_radius_score(tool_name, input);
        let irreversibility = irreversibility_score(tool_name);
        RiskScore::new(tool_risk, file_sensitivity, blast_radius, irreversibility)
    }

    #[must_use]
    pub fn assess_level(&self, tool_name: &str, input: &serde_json::Value) -> RiskLevel {
        let score = self.assess(tool_name, input);
        RiskLevel::from_score(score.composite_score())
    }

    #[must_use]
    pub fn assess_with_depth(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        depth: usize,
    ) -> RiskScore {
        self.assess(tool_name, input).with_depth_decay(depth)
    }

    #[must_use]
    pub fn assess_level_with_depth(
        &self,
        tool_name: &str,
        input: &serde_json::Value,
        depth: usize,
    ) -> RiskLevel {
        let score = self.assess_with_depth(tool_name, input, depth);
        RiskLevel::from_score(score.composite_score())
    }
}

fn base_tool_risk(tool_name: &str) -> f32 {
    match tool_name {
        "read" => 0.1,
        "write" | "edit" | "agent" => 0.5,
        "bash" => 0.8,
        // Plugin and MCP tools are opaque unless they receive an explicit
        // profile, so require confirmation by default.
        _ => 0.9,
    }
}

fn is_builtin_tool(tool_name: &str) -> bool {
    matches!(tool_name, "read" | "write" | "edit" | "agent" | "bash")
}

fn file_sensitivity_score(input: &serde_json::Value) -> f32 {
    let path = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    let sensitive_patterns = [
        ".env",
        "credentials",
        "secret",
        "password",
        "token",
        "private_key",
        "id_rsa",
    ];
    let config_patterns = [
        "config.toml",
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
    ];

    let lower = path.to_lowercase();
    if sensitive_patterns.iter().any(|p| lower.contains(p)) {
        0.9
    } else if config_patterns.iter().any(|p| lower.contains(p)) {
        0.4
    } else {
        0.1
    }
}

fn blast_radius_score(tool_name: &str, input: &serde_json::Value) -> f32 {
    if tool_name != "bash" {
        return match tool_name {
            "write" | "edit" => 0.3,
            _ if !is_builtin_tool(tool_name) => 0.6,
            _ => 0.0,
        };
    }
    let cmd = input
        .get("command")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let lower = cmd.to_lowercase();
    let high_risk = ["rm -rf", "push", "docker", "sudo", "chmod", "chown"];
    let medium_risk = ["git", "cargo", "npm", "pip"];

    if high_risk.iter().any(|p| lower.contains(p)) {
        0.9
    } else if medium_risk.iter().any(|p| lower.contains(p)) {
        0.5
    } else {
        0.2
    }
}

fn irreversibility_score(tool_name: &str) -> f32 {
    match tool_name {
        "bash" => 0.7,
        "write" => 0.3,
        "edit" => 0.2,
        _ if !is_builtin_tool(tool_name) => 0.5,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_is_allow() {
        let a = RiskAssessor;
        let level = a.assess_level("read", &serde_json::json!({"file_path": "src/main.rs"}));
        assert_eq!(level, RiskLevel::Allow);
    }

    #[test]
    fn bash_at_least_review() {
        let a = RiskAssessor;
        let level = a.assess_level("bash", &serde_json::json!({"command": "ls"}));
        assert!(level >= RiskLevel::Review);
    }

    #[test]
    fn bash_rm_rf_high_risk() {
        let a = RiskAssessor;
        let level = a.assess_level("bash", &serde_json::json!({"command": "rm -rf /tmp/test"}));
        assert!(level >= RiskLevel::RequireConfirmation);
    }

    #[test]
    fn sensitive_file_raises_score() {
        let a = RiskAssessor;
        let s1 = a.assess("write", &serde_json::json!({"file_path": "src/main.rs"}));
        let s2 = a.assess("write", &serde_json::json!({"file_path": ".env"}));
        assert!(s2.composite_score() > s1.composite_score());
    }

    #[test]
    fn depth_increases_risk() {
        let a = RiskAssessor;
        let s0 = a.assess("write", &serde_json::json!({"file_path": "x.rs"}));
        let s2 = a.assess_with_depth("write", &serde_json::json!({"file_path": "x.rs"}), 2);
        assert!(s2.composite_score() > s0.composite_score());
    }

    #[test]
    fn unknown_tool_requires_confirmation() {
        let a = RiskAssessor;
        let level = a.assess_level("some_plugin", &serde_json::json!({}));
        assert_eq!(level, RiskLevel::RequireConfirmation);
    }
}
