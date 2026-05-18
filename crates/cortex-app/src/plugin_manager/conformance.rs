use std::path::Path;

use cortex_types::plugin::{PluginConformanceCheck, PluginManifest, PluginPackageMetadata};

pub(super) fn conformance_state(package: &PluginPackageMetadata, local_passed: bool) -> String {
    let package_state =
        package
            .conformance
            .as_ref()
            .map_or("no package certificate", |certificate| {
                if certificate.passed {
                    "package certificate passed"
                } else {
                    "package certificate failed"
                }
            });
    if local_passed {
        format!("local checks passed; {package_state}")
    } else {
        format!("local checks failed; {package_state}")
    }
}

pub(super) fn recommended_risk_profile(manifest: &PluginManifest) -> Vec<String> {
    manifest
        .capabilities
        .declared_effects()
        .into_iter()
        .map(|effect| {
            format!(
                "[risk.tools.{}] effect = {:?} target = \"{}\" floor = {:?}",
                manifest.name,
                effect.kind,
                effect.target,
                effect.risk_floor()
            )
        })
        .collect()
}

pub(super) fn conformance_checks(
    dir: &Path,
    manifest: &PluginManifest,
) -> Vec<PluginConformanceCheck> {
    let mut checks = vec![
        check("manifest identity", !manifest.name.trim().is_empty(), ""),
        check(
            "manifest version",
            !manifest.version.trim().is_empty(),
            "version is required",
        ),
        check(
            "cortex version target",
            cortex_types::plugin::check_plugin_version(manifest, env!("CARGO_PKG_VERSION"))
                .accepted,
            "cortex_version must be less than or equal to this Cortex release",
        ),
        check(
            "capability declaration",
            !manifest.capabilities.provides.is_empty(),
            "capabilities.provides must not be empty",
        ),
    ];

    if let Some(native) = &manifest.native {
        checks.push(check(
            "native isolation boundary",
            native.isolation == cortex_types::plugin::NativePluginIsolation::Process
                || native.abi_version == Some(cortex_sdk::NATIVE_ABI_VERSION),
            "trusted native plugins must declare the current ABI",
        ));
        for tool in &native.tools {
            checks.extend(process_tool_checks(dir, manifest, tool));
        }
    }
    checks
}

fn check(name: &str, passed: bool, message: &str) -> PluginConformanceCheck {
    PluginConformanceCheck {
        name: name.to_string(),
        passed,
        message: if passed {
            String::new()
        } else {
            message.to_string()
        },
    }
}

fn process_tool_checks(
    dir: &Path,
    manifest: &PluginManifest,
    tool: &cortex_types::plugin::ProcessToolConfig,
) -> Vec<PluginConformanceCheck> {
    let command = resolve_plugin_tool_path(dir, &tool.command);
    let command_bound = tool.allow_host_paths || path_stays_under(dir, &command);
    let mut checks = vec![
        check(
            &format!("tool {} command path", tool.name),
            command_bound,
            "command escapes plugin directory",
        ),
        check(
            &format!("tool {} command exists", tool.name),
            command.is_file(),
            "command file is missing",
        ),
        check(
            &format!("tool {} output limit", tool.name),
            tool.max_output_bytes.unwrap_or(1) > 0,
            "max_output_bytes must be positive",
        ),
        check(
            &format!("tool {} timeout", tool.name),
            tool.timeout_secs.unwrap_or(1) > 0,
            "timeout_secs must be positive when set",
        ),
    ];
    if let Some(working_dir) = &tool.working_dir {
        let working_dir = resolve_plugin_tool_path(dir, working_dir);
        checks.push(check(
            &format!("tool {} working_dir path", tool.name),
            tool.allow_host_paths || path_stays_under(dir, &working_dir),
            "working_dir escapes plugin directory",
        ));
        checks.push(check(
            &format!("tool {} working_dir exists", tool.name),
            working_dir.is_dir(),
            "working_dir is missing",
        ));
    }
    if !manifest.capabilities.secrets {
        checks.push(check(
            &format!("tool {} env allowlist", tool.name),
            !tool
                .inherit_env
                .iter()
                .any(|name| looks_like_secret_env_name(name)),
            "secret-like inherited env requires capabilities.secrets = true",
        ));
    }
    checks
}

fn resolve_plugin_tool_path(dir: &Path, value: &str) -> std::path::PathBuf {
    let path = Path::new(value);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        dir.join(path)
    }
}

fn path_stays_under(root: &Path, candidate: &Path) -> bool {
    let Ok(root) = root.canonicalize() else {
        return false;
    };
    let Ok(candidate) = candidate.canonicalize() else {
        return false;
    };
    candidate.starts_with(root)
}

fn looks_like_secret_env_name(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    ["TOKEN", "SECRET", "KEY", "PASSWORD", "CREDENTIAL"]
        .iter()
        .any(|needle| upper.contains(needle))
}
