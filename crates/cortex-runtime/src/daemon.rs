use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cortex_types::{
    ActorId, AuthContext, ClientId, SessionId, TenantId, TokenUsage, TransportCapabilities, TurnId,
};
use serde::{Deserialize, Serialize};

use crate::{CortexRuntime, RuntimeError};

#[derive(Debug)]
pub enum DaemonError {
    ExistingDaemon(PathBuf),
    Io(std::io::Error),
    Json(serde_json::Error),
    Runtime(RuntimeError),
    Protocol(String),
}

impl From<std::io::Error> for DaemonError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for DaemonError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<RuntimeError> for DaemonError {
    fn from(error: RuntimeError) -> Self {
        Self::Runtime(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonConfig {
    pub socket_path: PathBuf,
    pub journal_path: PathBuf,
    pub state_path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubmittedTurn {
    pub session_id: SessionId,
    pub turn_id: TurnId,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonStatus {
    pub version: String,
    pub socket_path: String,
    pub tenants: usize,
    pub clients: usize,
    pub sessions: usize,
    pub persistent: bool,
    pub journal_mode: Option<String>,
    pub wal_autocheckpoint_pages: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonBootstrap {
    #[serde(default)]
    pub tenants: Vec<DaemonTenantConfig>,
    #[serde(default)]
    pub clients: Vec<DaemonClientConfig>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonTenantConfig {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DaemonClientConfig {
    pub tenant_id: String,
    pub actor_id: String,
    pub client_id: String,
    #[serde(default = "default_max_chars")]
    pub max_chars: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonRequest {
    Status,
    Shutdown,
    RegisterTenant {
        tenant_id: TenantId,
        name: String,
    },
    BindClient {
        context: AuthContext,
        capabilities: TransportCapabilities,
    },
    EnsureSession {
        context: AuthContext,
    },
    SubmitUserMessage {
        context: AuthContext,
        input: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DaemonResponse {
    Ack,
    Error {
        message: String,
    },
    Session {
        session_id: SessionId,
    },
    Status {
        status: DaemonStatus,
    },
    SubmittedTurn {
        turn: SubmittedTurn,
        usage: TokenUsage,
    },
}

pub struct DaemonServer {
    config: DaemonConfig,
    runtime: CortexRuntime,
    shutdown: bool,
}

impl DaemonConfig {
    #[must_use]
    pub fn new(data_dir: impl AsRef<Path>, socket_path: impl AsRef<Path>) -> Self {
        let data_dir = data_dir.as_ref();
        Self {
            socket_path: socket_path.as_ref().to_path_buf(),
            journal_path: data_dir.join("journal.jsonl"),
            state_path: data_dir.join("state.sqlite"),
        }
    }
}

impl DaemonBootstrap {
    /// # Errors
    /// Returns an error when the bootstrap file cannot be read or parsed.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, DaemonError> {
        let raw = fs::read_to_string(path)?;
        serde_json::from_str(&raw).map_err(DaemonError::from)
    }
}

impl DaemonServer {
    /// # Errors
    /// Returns an error when persistent runtime state cannot be opened.
    pub fn open(config: DaemonConfig) -> Result<Self, DaemonError> {
        let runtime = CortexRuntime::open_persistent(&config.journal_path, &config.state_path)?;
        Ok(Self {
            config,
            runtime,
            shutdown: false,
        })
    }

    /// # Errors
    /// Returns an error when a bootstrap tenant or client cannot be registered
    /// in the durable runtime.
    pub fn bootstrap(&mut self, bootstrap: &DaemonBootstrap) -> Result<(), DaemonError> {
        for tenant in &bootstrap.tenants {
            self.runtime
                .register_tenant(&TenantId::from_raw(tenant.id.clone()), tenant.name.clone())?;
        }
        for client in &bootstrap.clients {
            let context = AuthContext::new(
                TenantId::from_raw(client.tenant_id.clone()),
                ActorId::from_raw(client.actor_id.clone()),
                ClientId::from_raw(client.client_id.clone()),
            );
            self.runtime
                .bind_client(&context, TransportCapabilities::plain(client.max_chars))?;
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error when the socket cannot be bound, a live daemon already
    /// owns the socket, or a request cannot be read or written.
    pub fn serve(mut self) -> Result<(), DaemonError> {
        let listener = bind_listener(&self.config.socket_path)?;
        listener.set_nonblocking(true)?;
        while !self.shutdown {
            match listener.accept() {
                Ok((stream, _address)) => self.handle_stream(stream)?,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(error) => return Err(DaemonError::Io(error)),
            }
        }
        remove_socket(&self.config.socket_path)?;
        Ok(())
    }

    fn handle_stream(&mut self, stream: UnixStream) -> Result<(), DaemonError> {
        let mut reader = BufReader::new(stream);
        let mut raw = String::new();
        reader.read_line(&mut raw)?;
        let response = match serde_json::from_str::<DaemonRequest>(&raw) {
            Ok(request) => self.handle_request(request),
            Err(error) => DaemonResponse::Error {
                message: format!("invalid request: {error}"),
            },
        };
        let mut stream = reader.into_inner();
        serde_json::to_writer(&mut stream, &response)?;
        stream.write_all(b"\n")?;
        stream.flush()?;
        Ok(())
    }

    fn handle_request(&mut self, request: DaemonRequest) -> DaemonResponse {
        match self.apply_request(request) {
            Ok(response) => response,
            Err(error) => DaemonResponse::Error {
                message: format!("{error:?}"),
            },
        }
    }

    fn apply_request(&mut self, request: DaemonRequest) -> Result<DaemonResponse, DaemonError> {
        match request {
            DaemonRequest::Status => Ok(DaemonResponse::Status {
                status: self.status()?,
            }),
            DaemonRequest::Shutdown => {
                self.shutdown = true;
                Ok(DaemonResponse::Ack)
            }
            DaemonRequest::RegisterTenant { tenant_id, name } => {
                self.runtime.register_tenant(&tenant_id, name)?;
                Ok(DaemonResponse::Ack)
            }
            DaemonRequest::BindClient {
                context,
                capabilities,
            } => {
                self.runtime.bind_client(&context, capabilities)?;
                Ok(DaemonResponse::Ack)
            }
            DaemonRequest::EnsureSession { context } => {
                let session_id = self.runtime.ensure_session_for_turn(&context)?;
                Ok(DaemonResponse::Session { session_id })
            }
            DaemonRequest::SubmitUserMessage { context, input } => {
                let turn = self.runtime.submit_user_message(&context, &input)?;
                Ok(DaemonResponse::SubmittedTurn {
                    turn,
                    usage: TokenUsage::default(),
                })
            }
        }
    }

    fn status(&self) -> Result<DaemonStatus, DaemonError> {
        let health = self.runtime.store_health()?;
        Ok(DaemonStatus {
            version: env!("CARGO_PKG_VERSION").to_string(),
            socket_path: self.config.socket_path.display().to_string(),
            tenants: self.runtime.tenant_count(),
            clients: self.runtime.client_binding_count(),
            sessions: self.runtime.session_count(),
            persistent: self.runtime.is_persistent(),
            journal_mode: health.as_ref().map(|health| health.journal_mode.clone()),
            wal_autocheckpoint_pages: health.map(|health| health.wal_autocheckpoint_pages),
        })
    }
}

/// # Errors
/// Returns an error when the daemon socket cannot be opened or the request
/// cannot be encoded, sent, read, or decoded.
pub fn send_request(
    socket_path: impl AsRef<Path>,
    request: &DaemonRequest,
) -> Result<DaemonResponse, DaemonError> {
    let mut stream = UnixStream::connect(socket_path)?;
    serde_json::to_writer(&mut stream, request)?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut raw = String::new();
    reader.read_line(&mut raw)?;
    if raw.is_empty() {
        return Err(DaemonError::Protocol("empty daemon response".to_string()));
    }
    serde_json::from_str(&raw).map_err(DaemonError::from)
}

fn bind_listener(socket_path: &Path) -> Result<UnixListener, DaemonError> {
    if socket_path.exists() {
        if UnixStream::connect(socket_path).is_ok() {
            return Err(DaemonError::ExistingDaemon(socket_path.to_path_buf()));
        }
        fs::remove_file(socket_path)?;
    }
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }
    UnixListener::bind(socket_path).map_err(DaemonError::from)
}

fn remove_socket(socket_path: &Path) -> Result<(), DaemonError> {
    match fs::remove_file(socket_path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(DaemonError::Io(error)),
    }
}

const fn default_max_chars() -> usize {
    4_096
}
