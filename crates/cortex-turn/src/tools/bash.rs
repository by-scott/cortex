use super::{Tool, ToolError, ToolResult};
use std::collections::{HashMap, VecDeque};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Stdio};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const DEFAULT_RUN_TIMEOUT: Duration = Duration::from_mins(10);
const MAX_RUN_TIMEOUT_SECS: u64 = 6 * 60 * 60;
const PROCESS_OUTPUT_LIMIT: usize = 128 * 1024;
const DEFAULT_READ_CHARS: usize = 20_000;
const MAX_READ_CHARS: usize = 100_000;
const MAX_BACKGROUND_PROCESSES: usize = 16;
const READ_BUFFER_SIZE: usize = 8192;
const MAX_FOLLOW_SECS: u64 = 60;

static PROCESS_MANAGER: LazyLock<Mutex<BashProcessManager>> =
    LazyLock::new(|| Mutex::new(BashProcessManager::default()));

const BASH_INPUT_SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "action": {
      "type": "string",
      "enum": ["run", "spawn", "list", "status", "read", "write", "stop", "kill", "remove"],
      "description": "Bash action. Omit action for the legacy one-shot run behavior."
    },
    "command": {
      "type": "string",
      "description": "Command passed to bash -c. Required for run and spawn."
    },
    "cwd": {
      "type": "string",
      "description": "Working directory for run or spawn. Defaults to the current daemon working directory."
    },
    "process_id": {
      "type": "string",
      "description": "Managed process id for status, read, write, stop, kill, or remove. May also be supplied for spawn."
    },
    "name": {
      "type": "string",
      "description": "Friendly process id for spawn, or an alias for process_id on management actions."
    },
    "input": {
      "type": "string",
      "description": "Text written to a managed process stdin for write."
    },
    "append_newline": {
      "type": "boolean",
      "default": false,
      "description": "Append a newline to input when writing to stdin."
    },
    "stdout_cursor": {
      "type": "integer",
      "minimum": 0,
      "description": "Cursor returned by a prior read for stdout."
    },
    "stderr_cursor": {
      "type": "integer",
      "minimum": 0,
      "description": "Cursor returned by a prior read for stderr."
    },
    "max_chars": {
      "type": "integer",
      "minimum": 1,
      "maximum": 100000,
      "description": "Maximum characters returned by read."
    },
    "follow_secs": {
      "type": "integer",
      "minimum": 0,
      "maximum": 60,
      "description": "For read, wait up to this many seconds for new output after the supplied cursors."
    },
    "force": {
      "type": "boolean",
      "default": false,
      "description": "Allow remove to kill a running managed process first."
    },
    "timeout_secs": {
      "type": "integer",
      "minimum": 1,
      "maximum": 21600,
      "description": "Timeout for run in seconds. Defaults to 600. Background spawn has no timeout."
    }
  }
}"#;

pub struct BashTool;

impl Tool for BashTool {
    fn name(&self) -> &'static str {
        "bash"
    }

    fn description(&self) -> &'static str {
        "Run bash commands or manage named background bash processes. Use run for a captured \
         one-shot command, spawn for long-running or interactive commands, read/write/status/list \
         to interact with managed processes, and stop/kill/remove to clean them up."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::from_str(BASH_INPUT_SCHEMA).unwrap_or_else(|err| {
            serde_json::json!({
                "type": "object",
                "properties": {},
                "error": format!("invalid bash input schema: {err}"),
            })
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("run");
        match action {
            "run" => run_command(&input),
            "spawn" => manager_action(|manager| manager.spawn(&input)),
            "list" => manager_action(BashProcessManager::list),
            "status" => manager_action(|manager| manager.status(&process_ref(&input)?)),
            "read" => manager_action(|manager| manager.read(&process_ref(&input)?, &input)),
            "write" => manager_action(|manager| manager.write(&process_ref(&input)?, &input)),
            "stop" | "kill" => manager_action(|manager| manager.kill(&process_ref(&input)?)),
            "remove" => manager_action(|manager| manager.remove(&process_ref(&input)?, &input)),
            other => Err(ToolError::InvalidInput(format!(
                "unknown bash action '{other}'"
            ))),
        }
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::RunProcess)
                .with_target("command"),
        )
    }
}

#[derive(Default)]
struct BashProcessManager {
    next_id: u64,
    processes: HashMap<String, ManagedBashProcess>,
}

impl BashProcessManager {
    fn spawn(&mut self, input: &serde_json::Value) -> Result<ToolResult, ToolError> {
        self.prune_finished()?;
        if self.processes.len() >= MAX_BACKGROUND_PROCESSES {
            return Err(ToolError::ExecutionFailed(format!(
                "managed bash process limit reached ({MAX_BACKGROUND_PROCESSES})"
            )));
        }

        let command = required_string(input, "command")?;
        let cwd = command_cwd(input)?;
        let id = self.allocate_process_id(input)?;
        let process = ManagedBashProcess::spawn(&id, command, &cwd)?;
        let status = process.status_value();
        self.processes.insert(id, process);
        Ok(json_result(&status))
    }

    fn list(&mut self) -> Result<ToolResult, ToolError> {
        for process in self.processes.values_mut() {
            process.refresh()?;
        }
        let mut processes: Vec<serde_json::Value> = self
            .processes
            .values()
            .map(ManagedBashProcess::status_value)
            .collect();
        processes.sort_by(|left, right| {
            left.get("process_id")
                .and_then(serde_json::Value::as_str)
                .cmp(&right.get("process_id").and_then(serde_json::Value::as_str))
        });
        Ok(json_result(&serde_json::json!({ "processes": processes })))
    }

    fn status(&mut self, process_id: &str) -> Result<ToolResult, ToolError> {
        let process = self.process_mut(process_id)?;
        process.refresh()?;
        Ok(json_result(&process.status_value()))
    }

    fn read(
        &mut self,
        process_id: &str,
        input: &serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let stdout_cursor = optional_u64(input, "stdout_cursor")?;
        let stderr_cursor = optional_u64(input, "stderr_cursor")?;
        let max_chars = read_limit(input)?;
        let follow = follow_duration(input)?;
        let process = self.process_mut(process_id)?;
        process.wait_for_output(stdout_cursor, stderr_cursor, follow)?;
        process.refresh()?;
        Ok(json_result(&process.read_value(
            stdout_cursor,
            stderr_cursor,
            max_chars,
        )))
    }

    fn write(
        &mut self,
        process_id: &str,
        input: &serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let text = required_string(input, "input")?;
        let append_newline = input
            .get("append_newline")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        let process = self.process_mut(process_id)?;
        process.write(text, append_newline)?;
        Ok(json_result(&serde_json::json!({
            "process_id": process.id,
            "written": text.len() + usize::from(append_newline),
        })))
    }

    fn kill(&mut self, process_id: &str) -> Result<ToolResult, ToolError> {
        let process = self.process_mut(process_id)?;
        process.kill()?;
        Ok(json_result(&process.status_value()))
    }

    fn remove(
        &mut self,
        process_id: &str,
        input: &serde_json::Value,
    ) -> Result<ToolResult, ToolError> {
        let force = input
            .get("force")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);
        {
            let process = self.process_mut(process_id)?;
            process.refresh()?;
            if process.is_running() && !force {
                return Err(ToolError::ExecutionFailed(
                    "process is still running; stop it or set force=true".to_string(),
                ));
            }
            if process.is_running() {
                process.kill()?;
            }
        }
        let removed = self.processes.remove(process_id).is_some();
        Ok(json_result(&serde_json::json!({
            "process_id": process_id,
            "removed": removed,
        })))
    }

    fn process_mut(&mut self, process_id: &str) -> Result<&mut ManagedBashProcess, ToolError> {
        if self.processes.contains_key(process_id) {
            return self.processes.get_mut(process_id).ok_or_else(|| {
                ToolError::ExecutionFailed("managed bash process disappeared".to_string())
            });
        }
        let ids = self.process_ids();
        Err(ToolError::InvalidInput(format!(
            "unknown bash process '{process_id}'; managed processes: {}",
            ids.join(", ")
        )))
    }

    fn allocate_process_id(&mut self, input: &serde_json::Value) -> Result<String, ToolError> {
        if let Some(id) = optional_process_ref(input) {
            validate_process_id(id)?;
            if self.processes.contains_key(id) {
                return Err(ToolError::InvalidInput(format!(
                    "bash process '{id}' already exists"
                )));
            }
            return Ok(id.to_string());
        }

        loop {
            self.next_id = self.next_id.saturating_add(1);
            let id = format!("bash-{}", self.next_id);
            if !self.processes.contains_key(&id) {
                return Ok(id);
            }
        }
    }

    fn prune_finished(&mut self) -> Result<(), ToolError> {
        for process in self.processes.values_mut() {
            process.refresh()?;
        }
        Ok(())
    }

    fn process_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.processes.keys().cloned().collect();
        ids.sort();
        ids
    }
}

struct ManagedBashProcess {
    id: String,
    command: String,
    cwd: String,
    started_at: u64,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: SharedOutput,
    stderr: SharedOutput,
    exit: Option<ProcessExit>,
}

impl ManagedBashProcess {
    fn spawn(id: &str, command: &str, cwd: &Path) -> Result<Self, ToolError> {
        let mut process = crate::process::command_with_policy("bash", cwd);
        process
            .arg("-c")
            .arg(command)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = process
            .spawn()
            .map_err(|err| ToolError::ExecutionFailed(format!("spawn bash process: {err}")))?;
        let stdin = child.stdin.take();
        let Some(stdout) = child.stdout.take() else {
            kill_unusable_child(&mut child);
            return Err(ToolError::ExecutionFailed(
                "managed bash stdout unavailable".to_string(),
            ));
        };
        let Some(stderr) = child.stderr.take() else {
            kill_unusable_child(&mut child);
            return Err(ToolError::ExecutionFailed(
                "managed bash stderr unavailable".to_string(),
            ));
        };

        let stdout_buffer = SharedOutput::default();
        let stderr_buffer = SharedOutput::default();
        spawn_reader(stdout, &stdout_buffer);
        spawn_reader(stderr, &stderr_buffer);

        Ok(Self {
            id: id.to_string(),
            command: command.to_string(),
            cwd: cwd.display().to_string(),
            started_at: unix_seconds(),
            child,
            stdin,
            stdout: stdout_buffer,
            stderr: stderr_buffer,
            exit: None,
        })
    }

    fn refresh(&mut self) -> Result<(), ToolError> {
        if self.exit.is_some() {
            return Ok(());
        }
        if let Some(status) = self
            .child
            .try_wait()
            .map_err(|err| ToolError::ExecutionFailed(format!("check bash process: {err}")))?
        {
            self.stdin = None;
            self.exit = Some(ProcessExit {
                code: status.code(),
                success: status.success(),
            });
        }
        Ok(())
    }

    const fn is_running(&self) -> bool {
        self.exit.is_none()
    }

    fn write(&mut self, text: &str, append_newline: bool) -> Result<(), ToolError> {
        self.refresh()?;
        if !self.is_running() {
            return Err(ToolError::ExecutionFailed(
                "cannot write to an exited bash process".to_string(),
            ));
        }
        let Some(stdin) = self.stdin.as_mut() else {
            return Err(ToolError::ExecutionFailed(
                "managed bash stdin is closed".to_string(),
            ));
        };
        stdin
            .write_all(text.as_bytes())
            .and_then(|()| {
                if append_newline {
                    stdin.write_all(b"\n")
                } else {
                    Ok(())
                }
            })
            .and_then(|()| stdin.flush())
            .map_err(|err| ToolError::ExecutionFailed(format!("write bash stdin: {err}")))
    }

    fn kill(&mut self) -> Result<(), ToolError> {
        self.refresh()?;
        if self.exit.is_some() {
            return Ok(());
        }
        self.stdin = None;
        self.child
            .kill()
            .map_err(|err| ToolError::ExecutionFailed(format!("kill bash process: {err}")))?;
        let status = self
            .child
            .wait()
            .map_err(|err| ToolError::ExecutionFailed(format!("wait bash process: {err}")))?;
        self.exit = Some(ProcessExit {
            code: status.code(),
            success: status.success(),
        });
        Ok(())
    }

    fn status_value(&self) -> serde_json::Value {
        serde_json::json!({
            "process_id": self.id,
            "command": self.command,
            "cwd": self.cwd,
            "started_at": self.started_at,
            "running": self.is_running(),
            "exit": self.exit.as_ref().map(ProcessExit::to_value),
            "stdout_cursor": self.stdout.cursor(),
            "stderr_cursor": self.stderr.cursor(),
        })
    }

    fn read_value(
        &self,
        stdout_cursor: Option<u64>,
        stderr_cursor: Option<u64>,
        max_chars: usize,
    ) -> serde_json::Value {
        let stdout = self.stdout.snapshot(stdout_cursor, max_chars);
        let stderr = self.stderr.snapshot(stderr_cursor, max_chars);
        serde_json::json!({
            "process": self.status_value(),
            "stdout": stdout.text,
            "stderr": stderr.text,
            "stdout_cursor": stdout.cursor,
            "stderr_cursor": stderr.cursor,
            "stdout_truncated": stdout.truncated,
            "stderr_truncated": stderr.truncated,
        })
    }

    fn wait_for_output(
        &mut self,
        stdout_cursor: Option<u64>,
        stderr_cursor: Option<u64>,
        follow: Duration,
    ) -> Result<(), ToolError> {
        if follow.is_zero() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        loop {
            self.refresh()?;
            if self.has_new_output(stdout_cursor, stderr_cursor) || !self.is_running() {
                return Ok(());
            }
            if started.elapsed() >= follow {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    fn has_new_output(&self, stdout_cursor: Option<u64>, stderr_cursor: Option<u64>) -> bool {
        self.stdout.cursor() > stdout_cursor.unwrap_or(0)
            || self.stderr.cursor() > stderr_cursor.unwrap_or(0)
    }
}

impl Drop for ManagedBashProcess {
    fn drop(&mut self) {
        if self.exit.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

#[derive(Clone, Default)]
struct SharedOutput {
    inner: Arc<Mutex<ProcessOutput>>,
}

impl SharedOutput {
    fn append(&self, bytes: &[u8]) {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .append(bytes);
    }

    fn cursor(&self) -> u64 {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .total
    }

    fn snapshot(&self, cursor: Option<u64>, max_chars: usize) -> OutputSnapshot {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .snapshot(cursor, max_chars)
    }
}

#[derive(Default)]
struct ProcessOutput {
    bytes: VecDeque<u8>,
    total: u64,
}

impl ProcessOutput {
    fn append(&mut self, chunk: &[u8]) {
        self.total = self
            .total
            .saturating_add(u64::try_from(chunk.len()).unwrap_or(u64::MAX));
        self.bytes.extend(chunk.iter().copied());
        while self.bytes.len() > PROCESS_OUTPUT_LIMIT {
            let _ = self.bytes.pop_front();
        }
    }

    fn snapshot(&self, cursor: Option<u64>, max_chars: usize) -> OutputSnapshot {
        let stored_len = u64::try_from(self.bytes.len()).unwrap_or(u64::MAX);
        let earliest = self.total.saturating_sub(stored_len);
        let start = cursor.unwrap_or(earliest).max(earliest);
        let skip = usize::try_from(start.saturating_sub(earliest))
            .unwrap_or(self.bytes.len())
            .min(self.bytes.len());
        let bytes: Vec<u8> = self.bytes.iter().skip(skip).copied().collect();
        let text = String::from_utf8_lossy(&bytes);
        let (text, text_truncated) = truncate_chars(&text, max_chars);
        OutputSnapshot {
            text,
            cursor: self.total,
            truncated: text_truncated || cursor.is_some_and(|value| value < earliest),
        }
    }
}

struct OutputSnapshot {
    text: String,
    cursor: u64,
    truncated: bool,
}

struct ProcessExit {
    code: Option<i32>,
    success: bool,
}

impl ProcessExit {
    fn to_value(&self) -> serde_json::Value {
        serde_json::json!({
            "code": self.code,
            "success": self.success,
        })
    }
}

fn run_command(input: &serde_json::Value) -> Result<ToolResult, ToolError> {
    let command = required_string(input, "command")?;
    let cwd = command_cwd(input)?;
    let timeout = run_timeout(input)?;
    let mut process = crate::process::command_with_policy("bash", &cwd);
    process.arg("-c").arg(command).stdin(Stdio::null());

    match crate::process::run_captured(&mut process, timeout) {
        Ok(output) => Ok(format_captured_output(&output)),
        Err(crate::process::ProcessError::Timeout {
            timeout,
            stdout,
            stderr,
            ..
        }) => Ok(ToolResult::error(format!(
            "timed out after {}s: {}",
            timeout.as_secs(),
            timeout_text(&stdout, &stderr)
        ))),
        Err(err) => Ok(ToolResult::error(format!("failed to execute: {err}"))),
    }
}

fn format_captured_output(output: &crate::process::CapturedOutput) -> ToolResult {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    if output.status.success() {
        let mut result = stdout.trim().to_string();
        if !stderr.is_empty() {
            result.push_str("\n[stderr] ");
            result.push_str(stderr.trim());
        }
        ToolResult::success(result)
    } else {
        let msg = if stderr.is_empty() {
            stdout.trim().to_string()
        } else {
            stderr.trim().to_string()
        };
        ToolResult::error(format!(
            "exit code {}: {msg}",
            output.status.code().unwrap_or(-1),
        ))
    }
}

fn manager_action(
    action: impl FnOnce(&mut BashProcessManager) -> Result<ToolResult, ToolError>,
) -> Result<ToolResult, ToolError> {
    let mut manager = PROCESS_MANAGER
        .lock()
        .map_err(|err| ToolError::ExecutionFailed(format!("bash process lock failed: {err}")))?;
    action(&mut manager)
}

fn spawn_reader(mut pipe: impl Read + Send + 'static, output: &SharedOutput) {
    let output = output.clone();
    drop(std::thread::spawn(move || {
        let mut buffer = [0_u8; READ_BUFFER_SIZE];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(size) => output.append(&buffer[..size]),
            }
        }
    }));
}

fn kill_unusable_child(child: &mut Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn process_ref(input: &serde_json::Value) -> Result<String, ToolError> {
    optional_process_ref(input)
        .map(str::to_string)
        .ok_or_else(|| ToolError::InvalidInput("missing process_id".to_string()))
}

fn optional_process_ref(input: &serde_json::Value) -> Option<&str> {
    input
        .get("process_id")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            input
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|value| !value.trim().is_empty())
        })
}

fn validate_process_id(id: &str) -> Result<(), ToolError> {
    if id.len() > 64 {
        return Err(ToolError::InvalidInput(
            "bash process_id must not exceed 64 characters".to_string(),
        ));
    }
    if !id
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
    {
        return Err(ToolError::InvalidInput(
            "bash process_id must contain only alphanumeric characters, dots, underscores, or hyphens"
                .to_string(),
        ));
    }
    Ok(())
}

fn command_cwd(input: &serde_json::Value) -> Result<PathBuf, ToolError> {
    let cwd = input
        .get("cwd")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or(".");
    resolve_cwd(cwd)
}

fn resolve_cwd(cwd: &str) -> Result<PathBuf, ToolError> {
    let path = PathBuf::from(cwd);
    if path.is_absolute() {
        return Ok(path);
    }
    std::env::current_dir()
        .map(|current| current.join(path))
        .map_err(|err| ToolError::ExecutionFailed(format!("resolve bash cwd: {err}")))
}

fn required_string<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolError> {
    input
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| ToolError::InvalidInput(format!("missing {key}")))
}

fn optional_u64(input: &serde_json::Value, key: &str) -> Result<Option<u64>, ToolError> {
    input.get(key).map_or(Ok(None), |value| {
        value
            .as_u64()
            .map(Some)
            .ok_or_else(|| ToolError::InvalidInput(format!("{key} must be a non-negative integer")))
    })
}

fn read_limit(input: &serde_json::Value) -> Result<usize, ToolError> {
    let raw = input
        .get("max_chars")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_else(|| u64::try_from(DEFAULT_READ_CHARS).unwrap_or(u64::MAX));
    let limit = usize::try_from(raw).map_err(|_| {
        ToolError::InvalidInput("max_chars is too large for this platform".to_string())
    })?;
    Ok(limit.clamp(1, MAX_READ_CHARS))
}

fn follow_duration(input: &serde_json::Value) -> Result<Duration, ToolError> {
    let secs = input.get("follow_secs").map_or(Ok(0), |value| {
        value.as_u64().ok_or_else(|| {
            ToolError::InvalidInput("follow_secs must be a non-negative integer".to_string())
        })
    })?;
    Ok(Duration::from_secs(secs.min(MAX_FOLLOW_SECS)))
}

fn run_timeout(input: &serde_json::Value) -> Result<Duration, ToolError> {
    let secs = input
        .get("timeout_secs")
        .map_or(Ok(DEFAULT_RUN_TIMEOUT.as_secs()), |value| {
            value.as_u64().ok_or_else(|| {
                ToolError::InvalidInput("timeout_secs must be a positive integer".to_string())
            })
        })?;
    if secs == 0 {
        return Err(ToolError::InvalidInput(
            "timeout_secs must be greater than zero".to_string(),
        ));
    }
    Ok(Duration::from_secs(secs.min(MAX_RUN_TIMEOUT_SECS)))
}

fn json_result(value: &serde_json::Value) -> ToolResult {
    ToolResult::success(value.to_string())
}

fn timeout_text(stdout: &[u8], stderr: &[u8]) -> String {
    if stderr.is_empty() {
        String::from_utf8_lossy(stdout).trim().to_string()
    } else {
        String::from_utf8_lossy(stderr).trim().to_string()
    }
}

fn truncate_chars(text: &str, max_chars: usize) -> (String, bool) {
    let mut chars = text.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        (truncated, true)
    } else {
        (text.to_string(), false)
    }
}

fn unix_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}
