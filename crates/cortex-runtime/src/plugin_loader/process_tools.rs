use std::collections::BTreeMap;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use cortex_sdk::{Tool, ToolCapabilities, ToolError, ToolResult};
use cortex_types::plugin::{PluginManifest, ProcessToolConfig};
use cortex_types::{EffectConfirmation, ToolEffect, ToolEffectKind};

use crate::{PluginInfo, PluginRegistry, ToolRegistry};

const DEFAULT_PROCESS_OUTPUT_LIMIT: usize = 1024 * 1024;

pub(super) fn load_process_tools(
    sub: &Path,
    manifest: &PluginManifest,
    plugin_registry: &mut PluginRegistry,
    tool_registry: &mut ToolRegistry,
) -> Result<(), String> {
    let Some(native) = &manifest.native else {
        return Err(format!(
            "plugin '{}' requests process isolation but has no [native] section",
            manifest.name
        ));
    };
    if native.tools.is_empty() {
        return Err(format!(
            "plugin '{}' requests process isolation but declares no [[native.tools]]",
            manifest.name
        ));
    }

    let internal_info = PluginInfo {
        name: manifest.name.clone(),
        version: manifest.version.clone(),
        description: manifest.description.clone(),
        plugin_type: cortex_types::PluginType::Tool,
    };
    plugin_registry.register_tool_info(&internal_info);

    for tool in &native.tools {
        validate_process_tool(manifest, sub, tool)?;
        tool_registry.register_from_plugin(
            &manifest.name,
            boxed_process_tool(sub, &manifest.capabilities, tool),
        );
    }

    tracing::info!(
        plugin = %manifest.name,
        tools = native.tools.len(),
        "process-isolated plugin tools registered"
    );
    Ok(())
}

pub(super) fn validate_process_tool(
    manifest: &PluginManifest,
    sub: &Path,
    tool: &ProcessToolConfig,
) -> Result<(), String> {
    if tool.name.trim().is_empty() {
        return Err(format!(
            "plugin '{}' declares a process tool with an empty name",
            manifest.name
        ));
    }
    if tool.description.trim().is_empty() {
        return Err(format!(
            "plugin '{}' process tool '{}' has an empty description",
            manifest.name, tool.name
        ));
    }
    let command = resolve_process_command(sub, &tool.command);
    if !tool.allow_host_paths {
        ensure_plugin_relative_path(sub, &command, "command", manifest, tool)?;
    }
    if !command.is_file() {
        return Err(format!(
            "plugin '{}' process tool '{}' command not found: {}",
            manifest.name,
            tool.name,
            command.display()
        ));
    }
    if let Some(working_dir) = &tool.working_dir {
        let working_dir = resolve_process_command(sub, working_dir);
        if !tool.allow_host_paths {
            ensure_plugin_relative_path(sub, &working_dir, "working_dir", manifest, tool)?;
        }
        if !working_dir.is_dir() {
            return Err(format!(
                "plugin '{}' process tool '{}' working_dir not found: {}",
                manifest.name,
                tool.name,
                working_dir.display()
            ));
        }
    }
    Ok(())
}

fn ensure_plugin_relative_path(
    sub: &Path,
    path: &Path,
    field: &str,
    manifest: &PluginManifest,
    tool: &ProcessToolConfig,
) -> Result<(), String> {
    let plugin_dir = sub.canonicalize().map_err(|err| {
        format!(
            "plugin '{}' process tool '{}' cannot canonicalize plugin dir {}: {err}",
            manifest.name,
            tool.name,
            sub.display()
        )
    })?;
    let candidate = path.canonicalize().map_err(|err| {
        format!(
            "plugin '{}' process tool '{}' cannot canonicalize {field} {}: {err}",
            manifest.name,
            tool.name,
            path.display()
        )
    })?;
    if candidate.starts_with(&plugin_dir) {
        Ok(())
    } else {
        Err(format!(
            "plugin '{}' process tool '{}' {field} escapes plugin directory: {}",
            manifest.name,
            tool.name,
            candidate.display()
        ))
    }
}

fn resolve_process_command(sub: &Path, command: &str) -> PathBuf {
    let path = PathBuf::from(command);
    if path.is_absolute() {
        path
    } else {
        sub.join(path)
    }
}

pub(super) fn boxed_process_tool(
    sub: &Path,
    manifest_capabilities: &cortex_types::plugin::PluginCapabilities,
    config: &ProcessToolConfig,
) -> Box<dyn Tool> {
    Box::new(ProcessPluginTool::new(sub, manifest_capabilities, config))
}

struct ProcessPluginTool {
    name: &'static str,
    description: &'static str,
    input_schema: serde_json::Value,
    command: PathBuf,
    args: Vec<String>,
    working_dir: PathBuf,
    inherit_env: Vec<String>,
    env: BTreeMap<String, String>,
    timeout_secs: Option<u64>,
    max_output_bytes: usize,
    max_memory_bytes: Option<u64>,
    max_cpu_secs: Option<u64>,
    effects: Vec<ToolEffect>,
}

impl ProcessPluginTool {
    fn new(
        sub: &Path,
        manifest_capabilities: &cortex_types::plugin::PluginCapabilities,
        config: &ProcessToolConfig,
    ) -> Self {
        let working_dir = config.working_dir.as_deref().map_or_else(
            || sub.to_path_buf(),
            |dir| resolve_process_command(sub, dir),
        );
        let inherit_env = if config.inherit_env.is_empty() {
            vec!["PATH".to_string()]
        } else {
            config.inherit_env.clone()
        };
        Self {
            name: Box::leak(config.name.clone().into_boxed_str()),
            description: Box::leak(config.description.clone().into_boxed_str()),
            input_schema: config.input_schema.clone(),
            command: resolve_process_command(sub, &config.command),
            args: config.args.clone(),
            working_dir,
            inherit_env,
            env: config.env.clone(),
            timeout_secs: config.timeout_secs,
            max_output_bytes: config
                .max_output_bytes
                .unwrap_or(DEFAULT_PROCESS_OUTPUT_LIMIT),
            max_memory_bytes: config.max_memory_bytes,
            max_cpu_secs: config.max_cpu_secs,
            effects: process_tool_effects(manifest_capabilities, config),
        }
    }
}

fn process_tool_effects(
    manifest_capabilities: &cortex_types::plugin::PluginCapabilities,
    config: &ProcessToolConfig,
) -> Vec<ToolEffect> {
    let mut effects = manifest_capabilities.declared_effects();
    effects.extend(config.effects.clone());
    if !effects
        .iter()
        .any(|effect| effect.kind == ToolEffectKind::RunProcess)
    {
        effects.push(
            ToolEffect::new(ToolEffectKind::RunProcess)
                .with_target("plugin subprocess")
                .with_confirmation(EffectConfirmation::Always),
        );
    }
    effects
}

fn runtime_effect_to_sdk(effect: &cortex_types::ToolEffect) -> cortex_sdk::ToolEffect {
    cortex_sdk::ToolEffect {
        kind: runtime_effect_kind_to_sdk(effect.kind),
        target: effect.target.clone(),
        reversibility: runtime_reversibility_to_sdk(effect.reversibility),
        confirmation: runtime_confirmation_to_sdk(effect.confirmation),
        dry_run: runtime_dry_run_to_sdk(effect.dry_run),
    }
}

const fn runtime_effect_kind_to_sdk(
    kind: cortex_types::ToolEffectKind,
) -> cortex_sdk::ToolEffectKind {
    match kind {
        cortex_types::ToolEffectKind::ReadFile => cortex_sdk::ToolEffectKind::ReadFile,
        cortex_types::ToolEffectKind::ReadSecret => cortex_sdk::ToolEffectKind::ReadSecret,
        cortex_types::ToolEffectKind::WriteFile => cortex_sdk::ToolEffectKind::WriteFile,
        cortex_types::ToolEffectKind::DeleteFile => cortex_sdk::ToolEffectKind::DeleteFile,
        cortex_types::ToolEffectKind::RunProcess => cortex_sdk::ToolEffectKind::RunProcess,
        cortex_types::ToolEffectKind::NetworkRequest => cortex_sdk::ToolEffectKind::NetworkRequest,
        cortex_types::ToolEffectKind::SendMessage => cortex_sdk::ToolEffectKind::SendMessage,
        cortex_types::ToolEffectKind::SpendMoney => cortex_sdk::ToolEffectKind::SpendMoney,
        cortex_types::ToolEffectKind::Deploy => cortex_sdk::ToolEffectKind::Deploy,
        cortex_types::ToolEffectKind::ModifyCredential => {
            cortex_sdk::ToolEffectKind::ModifyCredential
        }
        cortex_types::ToolEffectKind::PersistMemory => cortex_sdk::ToolEffectKind::PersistMemory,
        cortex_types::ToolEffectKind::PublishContent => cortex_sdk::ToolEffectKind::PublishContent,
        cortex_types::ToolEffectKind::ScheduleTask => cortex_sdk::ToolEffectKind::ScheduleTask,
        cortex_types::ToolEffectKind::GenerateMedia => cortex_sdk::ToolEffectKind::GenerateMedia,
        cortex_types::ToolEffectKind::IntrospectRuntime => {
            cortex_sdk::ToolEffectKind::IntrospectRuntime
        }
        cortex_types::ToolEffectKind::DelegateWork => cortex_sdk::ToolEffectKind::DelegateWork,
    }
}

const fn runtime_reversibility_to_sdk(
    reversibility: cortex_types::EffectReversibility,
) -> cortex_sdk::EffectReversibility {
    match reversibility {
        cortex_types::EffectReversibility::Reversible => {
            cortex_sdk::EffectReversibility::Reversible
        }
        cortex_types::EffectReversibility::PartiallyReversible => {
            cortex_sdk::EffectReversibility::PartiallyReversible
        }
        cortex_types::EffectReversibility::Irreversible => {
            cortex_sdk::EffectReversibility::Irreversible
        }
    }
}

const fn runtime_confirmation_to_sdk(
    confirmation: cortex_types::EffectConfirmation,
) -> cortex_sdk::EffectConfirmation {
    match confirmation {
        cortex_types::EffectConfirmation::Never => cortex_sdk::EffectConfirmation::Never,
        cortex_types::EffectConfirmation::OnRisk => cortex_sdk::EffectConfirmation::OnRisk,
        cortex_types::EffectConfirmation::Always => cortex_sdk::EffectConfirmation::Always,
    }
}

const fn runtime_dry_run_to_sdk(dry_run: cortex_types::DryRunSupport) -> cortex_sdk::DryRunSupport {
    match dry_run {
        cortex_types::DryRunSupport::NotSupported => cortex_sdk::DryRunSupport::NotSupported,
        cortex_types::DryRunSupport::Supported => cortex_sdk::DryRunSupport::Supported,
        cortex_types::DryRunSupport::RequiredBeforeExecute => {
            cortex_sdk::DryRunSupport::RequiredBeforeExecute
        }
    }
}

impl Tool for ProcessPluginTool {
    fn name(&self) -> &'static str {
        self.name
    }

    fn description(&self) -> &'static str {
        self.description
    }

    fn input_schema(&self) -> serde_json::Value {
        self.input_schema.clone()
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let request = serde_json::json!({
            "tool": self.name,
            "input": input,
        });
        let mut command = std::process::Command::new(&self.command);
        command
            .args(&self.args)
            .current_dir(&self.working_dir)
            .env_clear()
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        for name in &self.inherit_env {
            if let Ok(value) = std::env::var(name) {
                command.env(name, value);
            }
        }
        command.envs(&self.env);
        configure_process_limits(&mut command, self.max_memory_bytes, self.max_cpu_secs);

        let mut child = command.spawn().map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "failed to spawn process-isolated tool '{}': {e}",
                self.name
            ))
        })?;

        if let Some(mut stdin) = child.stdin.take() {
            use std::io::Write;
            serde_json::to_writer(&mut stdin, &request).map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to encode request for process-isolated tool '{}': {e}",
                    self.name
                ))
            })?;
            stdin.write_all(b"\n").map_err(|e| {
                ToolError::ExecutionFailed(format!(
                    "failed to write request for process-isolated tool '{}': {e}",
                    self.name
                ))
            })?;
        }

        let timed_out = wait_for_process(&mut child, self.timeout_secs).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "process-isolated tool '{}' failed to wait: {e}",
                self.name
            ))
        })?;
        let output = child.wait_with_output().map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "process-isolated tool '{}' failed to collect output: {e}",
                self.name
            ))
        })?;

        if timed_out {
            return Ok(ToolResult::error(format!(
                "process-isolated tool '{}' timed out after {}s",
                self.name,
                self.timeout_secs.unwrap_or_default()
            )));
        }

        if output.stdout.len().saturating_add(output.stderr.len()) > self.max_output_bytes {
            return Ok(ToolResult::error(format!(
                "process-isolated tool '{}' exceeded output limit of {} bytes",
                self.name, self.max_output_bytes
            )));
        }

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Ok(ToolResult::error(if stderr.is_empty() {
                format!(
                    "process-isolated tool '{}' exited with status {}",
                    self.name, output.status
                )
            } else {
                stderr
            }));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        decode_process_tool_result(self.name, stdout.trim())
    }

    fn timeout_secs(&self) -> Option<u64> {
        self.timeout_secs
    }

    fn capabilities(&self) -> ToolCapabilities {
        ToolCapabilities::default().with_effects(self.effects.iter().map(runtime_effect_to_sdk))
    }
}

fn configure_process_limits(
    command: &mut std::process::Command,
    max_memory_bytes: Option<u64>,
    max_cpu_secs: Option<u64>,
) {
    #[cfg(unix)]
    {
        if max_memory_bytes.is_none() && max_cpu_secs.is_none() {
            return;
        }
        // SAFETY: `pre_exec` runs in the child after fork and before exec.
        // The closure only calls async-signal-safe libc `setrlimit` and
        // constructs `io::Error` from errno on failure.
        unsafe {
            command.pre_exec(move || {
                if let Some(bytes) = max_memory_bytes {
                    set_child_rlimit(libc::RLIMIT_AS, bytes)?;
                }
                if let Some(secs) = max_cpu_secs {
                    set_child_rlimit(libc::RLIMIT_CPU, secs)?;
                }
                Ok(())
            });
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (command, max_memory_bytes, max_cpu_secs);
    }
}

#[cfg(unix)]
fn set_child_rlimit(resource: libc::__rlimit_resource_t, limit: u64) -> std::io::Result<()> {
    let value: libc::rlim_t = limit;
    let rlimit = libc::rlimit {
        rlim_cur: value,
        rlim_max: value,
    };
    // SAFETY: `resource` is supplied from libc RLIMIT constants and `rlimit`
    // points to a valid stack value for the duration of the call.
    let rc = unsafe { libc::setrlimit(resource, &raw const rlimit) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

fn wait_for_process(
    child: &mut std::process::Child,
    timeout_secs: Option<u64>,
) -> std::io::Result<bool> {
    let Some(timeout_secs) = timeout_secs.filter(|secs| *secs > 0) else {
        return Ok(false);
    };
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        if child.try_wait()?.is_some() {
            return Ok(false);
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            return Ok(true);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn decode_process_tool_result(tool_name: &str, stdout: &str) -> Result<ToolResult, ToolError> {
    if stdout.is_empty() {
        return Ok(ToolResult::success(""));
    }

    let value: serde_json::Value = serde_json::from_str(stdout).map_err(|e| {
        ToolError::ExecutionFailed(format!(
            "process-isolated tool '{tool_name}' returned invalid JSON: {e}"
        ))
    })?;

    if let Some(s) = value.as_str() {
        return Ok(ToolResult::success(s));
    }

    let output = value
        .get("output")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "process-isolated tool '{tool_name}' must return a JSON string or object with string field 'output'"
            ))
        })?;
    let is_error = value
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    if is_error {
        Ok(ToolResult::error(output))
    } else {
        Ok(ToolResult::success(output))
    }
}
