use std::ffi::OsStr;
use std::io::{self, Read};
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const SAFE_ENV_KEYS: &[&str] = &[
    "PATH",
    "HOME",
    "CORTEX_HOME",
    "TMPDIR",
    "TEMP",
    "TMP",
    "LANG",
    "LC_ALL",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
];

#[derive(Debug)]
pub struct CapturedOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

#[derive(Debug)]
pub enum ProcessError {
    Spawn(io::Error),
    Wait(io::Error),
    Timeout {
        command: String,
        timeout: Duration,
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    PipeRead(io::Error),
    ReaderPanic(&'static str),
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(err) => write!(f, "failed to spawn process: {err}"),
            Self::Wait(err) => write!(f, "failed to wait for process: {err}"),
            Self::Timeout {
                command, timeout, ..
            } => write!(
                f,
                "process '{command}' timed out after {}s",
                timeout.as_secs()
            ),
            Self::PipeRead(err) => write!(f, "failed to read process output: {err}"),
            Self::ReaderPanic(pipe) => write!(f, "process {pipe} reader panicked"),
        }
    }
}

impl std::error::Error for ProcessError {}

pub fn command_with_policy(program: impl AsRef<OsStr>, cwd: &Path) -> Command {
    let mut command = Command::new(program);
    apply_env_policy(&mut command);
    command.current_dir(cwd);
    command
}

pub fn apply_env_policy(command: &mut Command) {
    command.env_clear();
    for key in SAFE_ENV_KEYS {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

pub fn run_captured(
    command: &mut Command,
    timeout: Duration,
) -> Result<CapturedOutput, ProcessError> {
    let command_name = command.get_program().to_string_lossy().into_owned();
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let mut child = command.spawn().map_err(ProcessError::Spawn)?;
    let stdout = child.stdout.take().map(read_pipe);
    let stderr = child.stderr.take().map(read_pipe);
    let started_at = Instant::now();
    let mut timed_out = false;
    let status = loop {
        if let Some(status) = child.try_wait().map_err(ProcessError::Wait)? {
            break status;
        }
        if started_at.elapsed() >= timeout {
            timed_out = true;
            let _ = child.kill();
            break child.wait().map_err(ProcessError::Wait)?;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let stdout = join_pipe(stdout, "stdout")?;
    let stderr = join_pipe(stderr, "stderr")?;
    if timed_out {
        return Err(ProcessError::Timeout {
            command: command_name,
            timeout,
            stdout,
            stderr,
        });
    }
    Ok(CapturedOutput {
        status,
        stdout,
        stderr,
    })
}

fn read_pipe(mut pipe: impl Read + Send + 'static) -> JoinHandle<io::Result<Vec<u8>>> {
    std::thread::spawn(move || {
        let mut output = Vec::new();
        pipe.read_to_end(&mut output)?;
        Ok(output)
    })
}

fn join_pipe(
    handle: Option<JoinHandle<io::Result<Vec<u8>>>>,
    name: &'static str,
) -> Result<Vec<u8>, ProcessError> {
    let Some(handle) = handle else {
        return Ok(Vec::new());
    };
    handle
        .join()
        .map_err(|_| ProcessError::ReaderPanic(name))?
        .map_err(ProcessError::PipeRead)
}
