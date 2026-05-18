use std::fs;
use std::path::{Path, PathBuf};

use crate::deploy::{parse_home_arg, parse_instance_id, parse_system_flag, resolve_cortex_home};

const DEMO_INSTANCE_ID: &str = "demo";
const DEMO_MODEL: &str = "qwen2.5-coder:7b";

/// `cortex demo [--id ID] [--home PATH] [--force]`
///
/// # Errors
/// Returns an error string if the fixture cannot be created or if the target
/// instance already has a config and `--force` was not supplied.
pub fn cmd_demo(args: &[String]) -> Result<(), String> {
    if parse_system_flag(args) {
        return Err("cortex demo creates a user-local fixture; omit --system".to_string());
    }

    let force = args.iter().any(|arg| arg == "--force" || arg == "-f");
    let paths = resolve_demo_paths(args)?;
    let instance_home = paths.instance_home();
    let workspace_dir = demo_workspace_dir(&paths);
    let config_path = paths.config_path();

    if config_path.exists() && !force {
        return Err(format!(
            "demo instance already exists at {}; rerun with --force to refresh demo-owned files",
            config_path.display()
        ));
    }

    cortex_kernel::ensure_base_dirs(paths.base_dir())
        .map_err(|err| format!("failed to create {}: {err}", paths.base_dir().display()))?;
    cortex_kernel::ensure_home_dirs(&instance_home)
        .map_err(|err| format!("failed to create {}: {err}", instance_home.display()))?;
    let _ = cortex_kernel::load_providers_for_paths(&paths)
        .map_err(|err| format!("failed to initialize providers.toml: {err}"))?;

    write_demo_file(&config_path, &demo_config_toml(), true)?;
    write_demo_file(&paths.mcp_path(), DEMO_MCP_TOML, force)?;
    write_demo_file(
        &paths
            .skills_dir()
            .join("local-coding-demo")
            .join("SKILL.md"),
        DEMO_LOCAL_CODING_SKILL,
        force,
    )?;
    write_demo_file(
        &workspace_dir.join("README.md"),
        DEMO_WORKSPACE_README,
        force,
    )?;
    write_demo_file(
        &workspace_dir.join("src").join("formatter.py"),
        DEMO_FORMATTER_PY,
        force,
    )?;
    write_demo_file(
        &workspace_dir.join("tests").join("test_formatter.py"),
        DEMO_FORMATTER_TEST,
        force,
    )?;

    eprintln!("Cortex demo fixture ready");
    eprintln!("  Instance:  {}", paths.instance_id());
    eprintln!("  Home:      {}", instance_home.display());
    eprintln!("  Workspace: {}", workspace_dir.display());
    eprintln!("  Model:     ollama / {DEMO_MODEL}");
    eprintln!("  Skill:     local-coding-demo");
    eprintln!();
    eprintln!("Next:");
    eprintln!("  ollama pull {DEMO_MODEL}");
    eprintln!("  cortex doctor --id {}", paths.instance_id());
    eprintln!("  cortex policy lint --id {}", paths.instance_id());
    eprintln!("  cortex install --id {}", paths.instance_id());
    Ok(())
}

fn resolve_demo_paths(args: &[String]) -> Result<cortex_kernel::CortexPaths, String> {
    let id = parse_instance_id(args).unwrap_or_else(|| DEMO_INSTANCE_ID.to_string());
    crate::cli::validate_instance_id(&id)?;
    let base = parse_home_arg(args).unwrap_or_else(resolve_cortex_home);
    Ok(cortex_kernel::CortexPaths::new(base, id))
}

fn demo_workspace_dir(paths: &cortex_kernel::CortexPaths) -> PathBuf {
    paths
        .base_dir()
        .join("workspaces")
        .join(paths.instance_id())
}

fn write_demo_file(path: &Path, content: &str, overwrite: bool) -> Result<(), String> {
    if path.exists() && !overwrite {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }
    fs::write(path, content).map_err(|err| format!("failed to write {}: {err}", path.display()))
}

fn demo_config_toml() -> String {
    format!(
        "\
# Cortex demo fixture.
# Local-first default: use Ollama if available, keep plugins disabled, and keep
# permission mode balanced. Policy/risk gates are review controls, not sandbox
# containment.

[api]
provider = \"ollama\"
api_key = \"\"
model = \"{DEMO_MODEL}\"
preset = \"minimal\"

[embedding]
provider = \"ollama\"
model = \"nomic-embed-text\"

[daemon]
addr = \"127.0.0.1:0\"

[turn]
max_tool_iterations = 32
execution_timeout_secs = 0
tool_timeout_secs = 600
strip_think_tags = true

[memory]
max_recall = 5
auto_extract = false
extract_min_turns = 5

[plugins]
enabled = []

[risk]
auto_approve_up_to = \"Review\"

[tools]
disabled = []
"
    )
}

const DEMO_MCP_TOML: &str = "\
# Demo fixture MCP config.
# Add reviewed MCP servers here only after checking their commands and env.
servers = []
";

const DEMO_LOCAL_CODING_SKILL: &str = r#"---
name: local-coding-demo
description: Practice Cortex's local coding loop on the generated demo workspace without touching runtime state.
when_to_use: Use when the user asks for the first-run local coding demo or wants a bounded coding exercise.
required_tools: []
execution_mode: inline
timeout_secs: 300
tags: ["demo", "coding", "local-first"]
user_invocable: true
agent_invocable: true
---
Use the workspace path supplied by the user as the only project scope.

Operating rules:
- Read the workspace files before proposing edits.
- Treat the Cortex instance home, config, prompts, memory, journal, sessions, channels, plugins, and providers registry as protected runtime state.
- Do not enable plugins or broaden permissions for the demo.
- Prefer the smallest edit that makes the included verification pass.
- Verify with `python3 -m unittest discover -s tests` from the demo workspace when Python is available.
- Report files changed, verification result, remaining risks, and next steps.

Suggested first task:
Fix `src/formatter.py`, then explain the change.
"#;

const DEMO_WORKSPACE_README: &str = "\
# Cortex Local Coding Demo

This workspace is generated by `cortex demo`.

It is intentionally outside the Cortex instance home so normal tools can work
on project files without touching protected runtime state.

Try this prompt after the demo instance is installed and running:

```text
Use the local-coding-demo skill on this workspace. Fix the formatter test and
verify it with python3 -m unittest discover -s tests.
```

The fixture uses no plugins and no external services beyond the configured
model provider. `cortex doctor` and `cortex policy lint` report readiness and
policy posture; they are not sandbox containment.
";

const DEMO_FORMATTER_PY: &str = "\
def normalize_title(value):
    \"\"\"Return a compact title-case label for UI display.\"\"\"
    return value.strip().title()
";

const DEMO_FORMATTER_TEST: &str = "\
import unittest

from src.formatter import normalize_title


class FormatterTests(unittest.TestCase):
    def test_normalize_title_collapses_internal_whitespace(self):
        self.assertEqual(normalize_title(\"  cortex   local   demo  \"), \"Cortex Local Demo\")

    def test_normalize_title_handles_tabs_and_newlines(self):
        self.assertEqual(normalize_title(\"risk\\tgate\\nreview\"), \"Risk Gate Review\")


if __name__ == \"__main__\":
    unittest.main()
";
