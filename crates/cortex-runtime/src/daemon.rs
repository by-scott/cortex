use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, RwLock};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use cortex_kernel::{Journal, SessionStore};
use cortex_turn::context::SummaryCache;
use cortex_turn::meta::MetaMonitor;
use cortex_types::{ConfirmationResponse, PermissionDecision, RiskLevel, SessionMetadata};

use crate::format::{fmt_tokens, format_duration};
use crate::rpc::{self, RpcHandler};
use crate::runtime::CortexRuntime;
use crate::session_manager::SessionManager;
use crate::shutdown::{abort_and_join, join_with_grace, shutdown_signal};
use crate::turn_executor::{TurnCallbacks, TurnExecutor, TurnExecutorConfig};

mod broadcast;
mod channel_tasks;
mod config;
mod heartbeat_actions;
mod http_api;
mod http_memory;
mod http_meta;
mod http_operator;
mod http_rpc;
mod http_server;
mod http_sessions;
mod http_turn;
mod line_protocol;
mod rpc_batch;
mod session_state;
mod slash_commands;
mod sse_stream;
mod transport_payloads;
mod turn_tasks;
mod ws_stream;

pub use self::broadcast::{BroadcastEvent, BroadcastMessage, PendingPermissionInfo};
pub use self::config::DaemonConfig;
use self::session_state::{DaemonSession, restore_failed_turn_history};
pub(crate) use self::turn_tasks::{
    BlockingStreamingTurnRequest, run_blocking_streaming_turn_with_timeout,
    run_blocking_turn_with_timeout,
};

struct PendingPermissionEntry {
    info: PendingPermissionInfo,
    decision: Mutex<Option<ConfirmationResponse>>,
    ready: Condvar,
}

impl PendingPermissionEntry {
    const fn new(info: PendingPermissionInfo) -> Self {
        Self {
            info,
            decision: Mutex::new(None),
            ready: Condvar::new(),
        }
    }

    fn resolve(&self, response: ConfirmationResponse) -> bool {
        let mut decision = self
            .decision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if decision.is_some() {
            return false;
        }
        *decision = Some(response);
        drop(decision);
        self.ready.notify_all();
        true
    }
}

pub(crate) enum SlashCommandAction {
    Output(String),
    Prompt(String),
    NotFound(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InjectMessageResult {
    Accepted,
    InputClosed,
    NoActiveTurn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ForegroundSlotError {
    ShuttingDown,
    Timeout,
}

impl ForegroundSlotError {
    pub(crate) const fn operator_detail(self) -> &'static str {
        match self {
            Self::ShuttingDown => "service shutting down",
            Self::Timeout => "another turn is in progress -- timed out after 30s",
        }
    }

    pub(crate) const fn user_message(self) -> &'static str {
        match self {
            Self::ShuttingDown => "Turn queue unavailable.",
            Self::Timeout => "Another turn is in progress. Please wait.",
        }
    }
}

struct BuildExecutorInput<'a> {
    cfg: &'a cortex_types::config::CortexConfig,
    resume: &'a cortex_types::ResumePacket,
    session_id: &'a str,
    actor: &'a str,
    source: &'a str,
    execution_scope: cortex_sdk::ExecutionScope,
    turns_since_extract: usize,
    skill_summaries: Option<String>,
    retrieved_evidence: &'a [cortex_types::EvidenceItem],
    tracer: &'a dyn cortex_turn::orchestrator::TurnTracer,
    control: Option<cortex_turn::orchestrator::TurnControl>,
    on_tpn_complete: Option<&'a (dyn Fn() + Send + Sync)>,
}

// ── Shared Daemon State ───────────────────────────────────────

/// Shared state accessible by all transports.
/// Memory subsystem components initialized together.
struct MemorySubsystem {
    store: Arc<cortex_kernel::MemoryStore>,
    embedding_client: Option<Arc<cortex_kernel::EmbeddingClient>>,
    embedding_store: Option<Arc<cortex_kernel::EmbeddingStore>>,
    embedding_health: Arc<cortex_turn::memory::recall::EmbeddingHealthStatus>,
}

struct LlmBindings {
    llm: Box<dyn cortex_turn::llm::LlmClient>,
    vision_llm: Option<Box<dyn cortex_turn::llm::LlmClient>>,
    direct_image_input: bool,
}

pub struct DaemonState {
    journal: Journal,
    session_store: SessionStore,
    task_store: Arc<cortex_kernel::TaskStore>,
    goal_store: Arc<cortex_kernel::GoalStore>,
    sessions: Mutex<HashMap<String, DaemonSession>>,
    /// Serializes foreground turn execution. GWT principle: the foreground
    /// workspace processes one task at a time. Concurrent turn requests
    /// queue here rather than running in parallel (which causes runtime
    /// conflicts between `spawn_blocking` and `block_in_place`).
    pub(crate) turn_semaphore: tokio::sync::Semaphore,
    start_time: chrono::DateTime<chrono::Utc>,
    active_transports: Mutex<Vec<String>>,
    config: RwLock<cortex_types::config::CortexConfig>,
    providers: RwLock<cortex_types::config::ProviderRegistry>,
    llm: Box<dyn cortex_turn::llm::LlmClient>,
    /// Vision-capable LLM used when images are present in a turn.
    /// Resolved from `[api.vision]` config or provider's `vision_model` field.
    vision_llm: Option<Box<dyn cortex_turn::llm::LlmClient>>,
    /// Whether raw image attachments should be sent directly to the LLM turn
    /// path instead of being pre-summarized by a fallback media tool.
    direct_image_input: bool,
    /// Per-group LLM clients for sub-endpoint routing.
    group_llms: HashMap<String, Box<dyn cortex_turn::llm::LlmClient>>,
    tools: cortex_turn::tools::ToolRegistry,
    prompt_manager: cortex_kernel::PromptManager,
    memory_store: Arc<cortex_kernel::MemoryStore>,
    embedding_client: Option<Arc<cortex_kernel::EmbeddingClient>>,
    embedding_store: Option<Arc<cortex_kernel::EmbeddingStore>>,
    embedding_health: Arc<cortex_turn::memory::recall::EmbeddingHealthStatus>,
    skill_registry: Arc<cortex_turn::skills::SkillRegistry>,
    home_dir: PathBuf,
    data_dir: PathBuf,
    max_output_tokens: usize,
    metrics: crate::metrics::MetricsCollector,
    pub(crate) rate_limiter: crate::rate_limiter::RateLimiter,
    heartbeat_state: Arc<crate::heartbeat::HeartbeatState>,
    /// Per-session event broadcasters.  Clients subscribe to a session's
    /// channel to receive real-time turn events (text, tool, trace, done).
    pub(crate) session_channels:
        Mutex<HashMap<String, tokio::sync::broadcast::Sender<BroadcastMessage>>>,
    /// Per-session turn control handles for active foreground turns.
    turn_controls: Mutex<HashMap<String, cortex_turn::orchestrator::TurnControl>>,
    /// Pending tool permission confirmations, keyed by short confirmation id.
    pending_permissions: Mutex<HashMap<String, Arc<PendingPermissionEntry>>>,
    /// The currently active foreground turn, used by `/stop`.
    active_turn_session: Mutex<Option<String>>,
    /// Last selected session per client transport (`rpc`, `http`, `ws`,
    /// `sock`, `stdio`), persisted under `data/client_sessions.json`.
    client_sessions: Mutex<HashMap<String, String>>,
    /// Last selected session per actor, persisted under `data/actor_sessions.json`.
    actor_sessions: Mutex<HashMap<String, String>>,
    /// Optional actor aliases so multiple channel identities can map to the
    /// same canonical user, persisted under `actors.toml`.
    actor_aliases: RwLock<HashMap<String, String>>,
    /// Optional transport-to-actor bindings so non-channel clients can act as
    /// a specific canonical user instead of the default local admin actor,
    /// persisted under `actors.toml`.
    transport_actors: RwLock<HashMap<String, String>>,
}

struct RuntimeBindings {
    client_sessions: HashMap<String, String>,
    actor_sessions: HashMap<String, String>,
    actor_aliases: HashMap<String, String>,
    transport_actors: HashMap<String, String>,
}

struct RuntimeArtifacts {
    journal: Journal,
    session_store: SessionStore,
    task_store: cortex_kernel::TaskStore,
    goal_store: cortex_kernel::GoalStore,
    memory_store: cortex_kernel::MemoryStore,
    prompt_manager: cortex_kernel::PromptManager,
}

/// RAII guard that marks the foreground runtime as busy for the duration of an
/// active foreground execution.
struct ForegroundActivity(Arc<crate::heartbeat::HeartbeatState>);

impl ForegroundActivity {
    fn acquire(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        state
            .foreground_busy
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self(Arc::clone(state))
    }
}

impl Drop for ForegroundActivity {
    fn drop(&mut self) {
        self.0
            .foreground_busy
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.0.touch();
    }
}

/// Unified foreground execution scope that keeps queue ownership and heartbeat
/// busy-state aligned for the lifetime of one user-visible turn.
pub(crate) struct ForegroundExecution<'a> {
    _permit: Option<tokio::sync::SemaphorePermit<'a>>,
    _activity: ForegroundActivity,
}

impl<'a> ForegroundExecution<'a> {
    fn queued(
        permit: tokio::sync::SemaphorePermit<'a>,
        state: &Arc<crate::heartbeat::HeartbeatState>,
    ) -> Self {
        Self {
            _permit: Some(permit),
            _activity: ForegroundActivity::acquire(state),
        }
    }

    fn immediate(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        Self {
            _permit: None,
            _activity: ForegroundActivity::acquire(state),
        }
    }
}

struct TurnControlRegistration<'a> {
    state: &'a DaemonState,
    session_id: String,
    control: cortex_turn::orchestrator::TurnControl,
}

impl<'a> TurnControlRegistration<'a> {
    fn new(state: &'a DaemonState, session_id: &str) -> Self {
        let control = cortex_turn::orchestrator::TurnControl::new();
        state
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string(), control.clone());
        *state
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(session_id.to_string());
        Self {
            state,
            session_id: session_id.to_string(),
            control,
        }
    }

    fn control(&self) -> cortex_turn::orchestrator::TurnControl {
        self.control.clone()
    }
}

impl Drop for TurnControlRegistration<'_> {
    fn drop(&mut self) {
        self.state
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.session_id);
        let mut active = self
            .state
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if active.as_deref() == Some(self.session_id.as_str()) {
            *active = None;
        }
    }
}

struct RuntimePermissionGate<'a> {
    state: &'a DaemonState,
    session_id: &'a str,
    actor: &'a str,
    source: &'a str,
    auto_approve_up_to: RiskLevel,
    control: Option<&'a cortex_turn::orchestrator::TurnControl>,
    on_event: Option<&'a (dyn Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync)>,
}

impl RuntimePermissionGate<'_> {
    fn confirmation_id() -> String {
        cortex_types::CorrelationId::new()
            .to_string()
            .chars()
            .take(8)
            .collect()
    }
}

impl cortex_turn::risk::PermissionGate for RuntimePermissionGate<'_> {
    fn check(&self, tool_name: &str, risk_level: RiskLevel) -> PermissionDecision {
        self.check_with_explanation(tool_name, risk_level, "")
    }

    fn check_with_explanation(
        &self,
        tool_name: &str,
        risk_level: RiskLevel,
        explanation: &str,
    ) -> PermissionDecision {
        if risk_level == RiskLevel::Block {
            return PermissionDecision::Denied;
        }
        if risk_level <= self.auto_approve_up_to {
            return PermissionDecision::Approved;
        }

        let id = Self::confirmation_id();
        let expires_at = chrono::Utc::now() + chrono::Duration::days(36_500);
        let info = PendingPermissionInfo {
            id: id.clone(),
            session_id: self.session_id.to_string(),
            actor: self.actor.to_string(),
            source: self.source.to_string(),
            tool_name: tool_name.to_string(),
            risk_level,
            explanation: explanation.to_string(),
            expires_at,
        };
        let entry = Arc::new(PendingPermissionEntry::new(info.clone()));
        self.state
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(id.clone(), Arc::clone(&entry));

        let _ = self
            .state
            .session_broadcast(self.session_id)
            .send(BroadcastMessage {
                session_id: self.session_id.to_string(),
                source: "permission".to_string(),
                event: BroadcastEvent::PermissionRequested(info),
            });
        if let Some(on_event) = self.on_event {
            on_event(&cortex_turn::orchestrator::TurnStreamEvent::Text {
                lane: cortex_turn::orchestrator::StreamLane::Observer,
                source: Some("permission".to_string()),
                content: entry.info.prompt_text(),
            });
        }

        let poll_interval = std::time::Duration::from_millis(200);
        let decision = loop {
            if self
                .control
                .is_some_and(cortex_turn::orchestrator::TurnControl::is_cancel_requested)
            {
                break ConfirmationResponse::Denied;
            }
            let wait_result = {
                let guard = entry
                    .decision
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                entry.ready.wait_timeout(guard, poll_interval)
            };
            let Ok((guard, wait_result)) = wait_result else {
                break ConfirmationResponse::Denied;
            };
            if let Some(response) = *guard {
                break response;
            }
            if wait_result.timed_out()
                && self
                    .control
                    .is_some_and(cortex_turn::orchestrator::TurnControl::is_cancel_requested)
            {
                break ConfirmationResponse::Denied;
            }
        };

        self.state
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);

        match decision {
            ConfirmationResponse::Approved => PermissionDecision::Approved,
            ConfirmationResponse::Denied => PermissionDecision::Denied,
        }
    }
}

impl DaemonState {
    pub(crate) fn cancel_turn_for_actor(
        &self,
        actor: &str,
        session_id: Option<&str>,
    ) -> Result<String, CancelTurnError> {
        let canonical = self.canonical_actor(actor);
        let target_session = if let Some(session_id) = session_id {
            let visible = self
                .session_lookup(session_id)
                .is_some_and(|session| self.session_visible_to_actor(&canonical, &session));
            if !visible {
                return Err(CancelTurnError::SessionNotFound);
            }
            Some(session_id.to_string())
        } else if Self::is_admin_actor(&canonical) {
            self.stop_target_session(None)
        } else {
            self.active_actor_session(&canonical)
        };

        let Some(control) = self.control_for_stop(target_session.as_deref()) else {
            return Err(CancelTurnError::NoActiveTurn);
        };
        control.request_cancel();
        if let Some(target_session) = target_session.as_deref() {
            self.deny_pending_permissions_for_session(target_session);
        }
        Ok(target_session.unwrap_or_else(|| "active".to_string()))
    }

    #[must_use]
    pub fn pending_permission_info(&self, id: &str) -> Option<PendingPermissionInfo> {
        self.pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(id)
            .map(|entry| entry.info.clone())
    }

    fn resolve_pending_permission(
        &self,
        session_id: Option<&str>,
        id: &str,
        response: ConfirmationResponse,
    ) -> String {
        let entry = {
            self.pending_permissions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(id)
                .cloned()
        };
        let Some(entry) = entry else {
            return format!("No pending permission request found for {id}.");
        };
        if let Some(session_id) = session_id
            && entry.info.session_id != session_id
        {
            return "That permission request belongs to another session.".into();
        }
        let decision = entry
            .decision
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if decision.is_some() {
            return format!("Permission request {id} was already resolved.");
        }
        drop(decision);
        let _ = entry.resolve(response);
        match response {
            ConfirmationResponse::Approved => format!("Approved tool '{}'.", entry.info.tool_name),
            ConfirmationResponse::Denied => format!("Denied tool '{}'.", entry.info.tool_name),
        }
    }
}

pub(crate) enum CancelTurnError {
    SessionNotFound,
    NoActiveTurn,
}

type OnTpnComplete<'a> = &'a (dyn Fn() + Send + Sync);

/// Turn tracer that emits events via the `tracing` crate (stderr / journald).
pub(crate) struct TracingTurnTracer {
    pub(crate) config: cortex_types::config::TurnTraceConfig,
}

impl cortex_turn::orchestrator::TurnTracer for TracingTurnTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat_str = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat_str) >= level {
            tracing::info!(category = cat_str.as_str(), "{message}");
        }
    }
}

/// Turn tracer that emits to both tracing (stderr) and an mpsc channel
/// for Socket streaming delivery.
struct ChannelTurnTracer {
    config: cortex_types::config::TurnTraceConfig,
    tx: tokio::sync::mpsc::Sender<String>,
}

impl cortex_turn::orchestrator::TurnTracer for ChannelTurnTracer {
    fn trace_at(
        &self,
        category: cortex_turn::orchestrator::TraceCategory,
        level: cortex_types::TraceLevel,
        message: &str,
    ) {
        let cat_str = format!("{category:?}").to_lowercase();
        if self.config.level_for(&cat_str) < level {
            return;
        }

        // Emit to tracing (stderr / journald)
        tracing::info!(category = cat_str.as_str(), "{message}");

        // Emit to channel as NDJSON event
        let payload = serde_json::json!({
            "event": "trace",
            "data": {
                "category": cat_str,
                "level": format!("{level:?}").to_lowercase(),
                "message": message,
            }
        });
        if let Ok(json) = serde_json::to_string(&payload) {
            let _ = self.tx.try_send(json);
        }
    }
}

impl DaemonState {
    fn paths(&self) -> cortex_kernel::CortexPaths {
        cortex_kernel::CortexPaths::from_instance_home(&self.home_dir)
    }

    /// Create daemon state from a fully initialized runtime.
    ///
    /// Re-creates subsystems from the runtime's home path. All subsystem
    /// constructors are idempotent (they open existing DBs).
    ///
    /// # Errors
    ///
    /// Returns an error string if essential subsystems (journal, memory,
    /// prompt manager, LLM endpoint) fail to initialize.
    pub fn from_runtime(rt: &mut CortexRuntime) -> Result<Self, String> {
        let home = rt.home().to_path_buf();
        let paths = cortex_kernel::CortexPaths::from_instance_home(&home);
        let data_dir = rt.data_dir().to_path_buf();
        let config = rt.config().clone();
        let providers = rt.providers().clone();
        let max_output_tokens = rt.max_output_tokens();

        let RuntimeArtifacts {
            journal,
            session_store,
            task_store,
            goal_store,
            memory_store,
            prompt_manager,
        } = Self::open_runtime_artifacts(&paths, &home)?;

        let LlmBindings {
            llm,
            vision_llm,
            direct_image_input,
        } = Self::init_llm_bindings(&config.api, &providers, &paths)?;
        let group_llms = Self::init_group_llms(&config, &providers);
        let skill_registry = Self::init_skill_registry(&home, &journal);
        let RuntimeBindings {
            client_sessions,
            actor_sessions,
            actor_aliases,
            transport_actors,
        } = Self::load_runtime_bindings(&data_dir);

        Self::load_plugin_skills(rt, &skill_registry);

        let cron_queue = Arc::new(cortex_turn::tools::cron::CronQueue::open(&data_dir));
        let mut tools = Self::init_tools(&config, &skill_registry);

        // Merge plugin-registered tools from the runtime into the daemon's registry.
        rt.drain_plugin_tools(&mut tools);
        let mem = Self::init_memory_subsystem(
            &config,
            &providers,
            &paths,
            &data_dir,
            memory_store,
            &mut tools,
            &cron_queue,
        );

        // Connect to configured MCP servers and register their tools as bridged tools.
        // `from_runtime` is sync but always called from within a tokio runtime,
        // so we use `block_in_place` + `Handle::current().block_on()` to drive
        // the async MCP handshake without spawning a nested runtime.
        if !config.mcp.servers.is_empty() {
            let mcp_manager = cortex_turn::mcp::McpManager::new();
            let before = tools.tool_names().len();
            let mcp_warnings = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current()
                    .block_on(mcp_manager.connect_and_register(&config.mcp, &mut tools))
            });
            let bridged = tools.tool_names().len() - before;
            tracing::info!(
                servers = config.mcp.servers.len(),
                bridged,
                "MCP client initialized"
            );
            for w in &mcp_warnings {
                tracing::warn!("MCP: {w}");
            }
        }

        let rate_limiter = crate::rate_limiter::RateLimiter::new(
            config.rate_limit.per_session_rpm,
            config.rate_limit.global_rpm,
        );

        // Register self-introspection tools (audit, prompt_inspect).
        crate::introspect_tools::register_introspect_tools(&mut tools, &home);

        Ok(Self {
            journal,
            session_store,
            task_store: Arc::new(task_store),
            goal_store: Arc::new(goal_store),
            sessions: Mutex::new(HashMap::new()),
            turn_semaphore: tokio::sync::Semaphore::new(1),
            start_time: chrono::Utc::now(),
            active_transports: Mutex::new(Vec::new()),
            config: RwLock::new(config),
            providers: RwLock::new(providers),
            llm,
            vision_llm,
            direct_image_input,
            group_llms,
            tools,
            prompt_manager,
            memory_store: mem.store,
            embedding_client: mem.embedding_client,
            embedding_store: mem.embedding_store,
            embedding_health: mem.embedding_health,
            skill_registry,
            home_dir: home,
            data_dir,
            max_output_tokens,
            metrics: crate::metrics::MetricsCollector::new(),
            rate_limiter,
            heartbeat_state: Self::init_heartbeat_state(cron_queue),
            session_channels: Mutex::new(HashMap::new()),
            turn_controls: Mutex::new(HashMap::new()),
            pending_permissions: Mutex::new(HashMap::new()),
            active_turn_session: Mutex::new(None),
            client_sessions: Mutex::new(client_sessions),
            actor_sessions: Mutex::new(actor_sessions),
            actor_aliases: RwLock::new(actor_aliases),
            transport_actors: RwLock::new(transport_actors),
        })
    }

    pub(crate) const fn session_manager(&self) -> SessionManager<'_> {
        SessionManager::new(&self.journal, &self.session_store)
    }

    fn init_heartbeat_state(
        cron_queue: Arc<cortex_turn::tools::cron::CronQueue>,
    ) -> Arc<crate::heartbeat::HeartbeatState> {
        let mut heartbeat = crate::heartbeat::HeartbeatState::new();
        heartbeat.cron_queue = Some(cron_queue);
        Arc::new(heartbeat)
    }

    fn storage_paths(data_dir: &Path) -> cortex_kernel::CortexPaths {
        let instance_home = data_dir.parent().unwrap_or(data_dir);
        cortex_kernel::CortexPaths::from_instance_home(instance_home)
    }

    fn runtime_state_store(data_dir: &Path) -> cortex_kernel::RuntimeStateStore {
        cortex_kernel::RuntimeStateStore::from_paths(&Self::storage_paths(data_dir))
    }

    fn actor_bindings_store(data_dir: &Path) -> cortex_kernel::ActorBindingsStore {
        cortex_kernel::ActorBindingsStore::from_paths(&Self::storage_paths(data_dir))
    }

    fn load_client_sessions(data_dir: &Path) -> HashMap<String, String> {
        Self::runtime_state_store(data_dir).client_sessions()
    }

    fn load_actor_sessions(data_dir: &Path) -> HashMap<String, String> {
        Self::runtime_state_store(data_dir).actor_sessions()
    }

    fn load_actor_bindings(data_dir: &Path) -> cortex_kernel::ActorBindingsStore {
        Self::actor_bindings_store(data_dir)
    }

    fn load_runtime_bindings(data_dir: &Path) -> RuntimeBindings {
        let client_sessions = Self::load_client_sessions(data_dir);
        let actor_sessions = Self::load_actor_sessions(data_dir);
        let actor_bindings = Self::load_actor_bindings(data_dir);
        let actor_aliases = actor_bindings.actor_aliases().into_iter().collect();
        let transport_actors = actor_bindings.transport_actors().into_iter().collect();
        RuntimeBindings {
            client_sessions,
            actor_sessions,
            actor_aliases,
            transport_actors,
        }
    }

    fn open_runtime_artifacts(
        paths: &cortex_kernel::CortexPaths,
        home: &Path,
    ) -> Result<RuntimeArtifacts, String> {
        let journal = Journal::open(paths.cortex_db_path())
            .map_err(|e| format!("daemon: journal open: {e}"))?;
        let session_store = SessionStore::open(&paths.sessions_dir())
            .map_err(|e| format!("daemon: session store open: {e}"))?;
        let task_store = cortex_kernel::TaskStore::open(&paths.data_dir().join("tasks.db"))
            .map_err(|e| format!("daemon: task store open: {e}"))?;
        let goal_store = cortex_kernel::GoalStore::open(&paths.data_dir().join("goals.db"))
            .map_err(|e| format!("daemon: goal store open: {e}"))?;
        let memory_store = cortex_kernel::MemoryStore::open(&paths.memory_dir())
            .map_err(|e| format!("daemon: memory open: {e}"))?;
        let prompt_manager = cortex_kernel::PromptManager::new(home)
            .map_err(|e| format!("daemon: prompt manager: {e}"))?;
        Ok(RuntimeArtifacts {
            journal,
            session_store,
            task_store,
            goal_store,
            memory_store,
            prompt_manager,
        })
    }

    fn load_plugin_skills(
        rt: &CortexRuntime,
        skill_registry: &Arc<cortex_turn::skills::SkillRegistry>,
    ) {
        for skill_dir in &rt.plugin_skill_dirs {
            skill_registry.reload_from(skill_dir, &cortex_types::SkillSource::Plugin);
        }
    }

    fn save_client_sessions(&self) {
        let sessions = self
            .client_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Self::runtime_state_store(&self.data_dir).save_client_sessions(&sessions);
    }

    fn save_actor_sessions(&self) {
        let sessions = self
            .actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Self::runtime_state_store(&self.data_dir).save_actor_sessions(&sessions);
    }

    #[must_use]
    pub(crate) const fn local_actor() -> &'static str {
        "local:default"
    }

    #[must_use]
    pub(crate) fn channel_actor(platform: &str, user_id: &str) -> String {
        format!("{platform}:{user_id}")
    }

    fn normalize_transport(transport: &str) -> &str {
        match transport {
            "sock" => "socket",
            other => other,
        }
    }

    pub(crate) fn transport_actor(&self, transport: &str) -> String {
        let transport = Self::normalize_transport(transport);
        self.transport_actors
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(transport)
            .cloned()
            .unwrap_or_else(|| Self::local_actor().to_string())
    }

    fn canonical_actor(&self, actor: &str) -> String {
        let mut current = actor.to_string();
        let mut visited = std::collections::HashSet::new();
        let actor_aliases = self
            .actor_aliases
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while let Some(next) = actor_aliases.get(&current) {
            if !visited.insert(current.clone()) {
                break;
            }
            current.clone_from(next);
        }
        current
    }

    fn is_admin_actor(actor: &str) -> bool {
        actor == Self::local_actor()
    }

    fn session_lookup(&self, session_id: &str) -> Option<SessionMetadata> {
        self.session_manager()
            .list_sessions()
            .into_iter()
            .find(|session| {
                session.id.to_string() == session_id || session.name.as_deref() == Some(session_id)
            })
    }

    fn session_token_total(&self, session_id: Option<&str>) -> Option<u64> {
        let session_id = session_id?;
        let in_memory_tokens = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .map(|session| session.meta.total_tokens());
        in_memory_tokens.or_else(|| {
            self.session_lookup(session_id)
                .map(|session| session.total_tokens())
        })
    }

    fn session_id_or_name_exists(&self, session_id: &str) -> bool {
        self.session_lookup(session_id).is_some()
            || self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(session_id)
    }

    fn session_visible_to_actor(&self, actor: &str, session: &SessionMetadata) -> bool {
        let canonical = self.canonical_actor(actor);
        let owner = self.canonical_actor(&session.owner_actor);
        Self::is_admin_actor(&canonical) || owner == canonical
    }

    pub(crate) fn actor_can_access_session(&self, actor: &str, session_id: &str) -> bool {
        self.session_lookup(session_id)
            .is_some_and(|session| self.session_visible_to_actor(actor, &session))
    }

    pub(crate) fn transport_can_access_session(&self, transport: &str, session_id: &str) -> bool {
        let actor = self.transport_actor(transport);
        self.actor_can_access_session(&actor, session_id)
    }

    pub(crate) fn active_actor_session(&self, actor: &str) -> Option<String> {
        let actor_sessions = self
            .actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor_sessions.get(actor).cloned().filter(|session_id| {
            self.session_lookup(session_id).is_some_and(|session| {
                session.is_active() && self.session_visible_to_actor(actor, &session)
            })
        })
    }

    pub(crate) fn resolve_actor_session(&self, actor: &str) -> String {
        if let Some(existing) = self.active_actor_session(actor) {
            return existing;
        }

        let canonical = self.canonical_actor(actor);
        let linked_session = {
            let actor_sessions = self
                .actor_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let canonical_fallback = if actor == canonical {
                None
            } else {
                actor_sessions.get(&canonical).cloned()
            };
            let alias_fallback = actor_sessions.iter().find_map(|(bound_actor, session_id)| {
                if bound_actor == actor || bound_actor == &canonical {
                    return None;
                }
                (self.canonical_actor(bound_actor) == canonical).then(|| session_id.clone())
            });
            let linked_session =
                canonical_fallback
                    .into_iter()
                    .chain(alias_fallback)
                    .find(|session_id| {
                        self.session_lookup(session_id).is_some_and(|session| {
                            session.is_active() && self.session_visible_to_actor(actor, &session)
                        })
                    });
            drop(actor_sessions);
            linked_session
        };
        if let Some(existing) = linked_session {
            self.set_actor_session(actor, &existing);
            return existing;
        }

        if let Some(existing) = self
            .visible_sessions(&canonical)
            .into_iter()
            .filter(cortex_types::SessionMetadata::is_active)
            .max_by_key(|session| session.created_at)
            .map(|session| session.id.to_string())
        {
            self.set_actor_session(actor, &existing);
            return existing;
        }

        let (sid, _meta) = self.session_manager().create_session_for_actor(&canonical);
        let sid = sid.to_string();
        self.set_actor_session(actor, &sid);
        sid
    }

    pub(crate) fn set_actor_session(&self, actor: &str, session_id: &str) {
        self.actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(actor.to_string(), session_id.to_string());
        self.save_actor_sessions();
    }

    pub(crate) fn clear_actor_session(&self, actor: &str) {
        self.actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(actor);
        self.save_actor_sessions();
    }

    pub(crate) fn visible_sessions(&self, actor: &str) -> Vec<SessionMetadata> {
        let canonical = self.canonical_actor(actor);
        self.session_manager()
            .list_sessions()
            .into_iter()
            .filter(|session| self.session_visible_to_actor(&canonical, session))
            .collect()
    }

    pub(crate) fn visible_sessions_for_transport(&self, transport: &str) -> Vec<SessionMetadata> {
        let actor = self.transport_actor(transport);
        self.visible_sessions(&actor)
    }

    pub(crate) fn create_session_for_actor(&self, actor: &str) -> (String, SessionMetadata) {
        let canonical = self.canonical_actor(actor);
        let (sid, meta) = self.session_manager().create_session_for_actor(&canonical);
        let sid = sid.to_string();
        self.set_actor_session(actor, &sid);
        (sid, meta)
    }

    fn active_session_bindings(&self) -> Vec<(String, Vec<String>)> {
        let mut bindings: HashMap<String, Vec<String>> = HashMap::new();

        {
            let client_sessions = self
                .client_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (client, session_id) in &*client_sessions {
                if !session_id.is_empty() && self.session_exists_and_active(session_id) {
                    bindings
                        .entry(session_id.clone())
                        .or_default()
                        .push(client.clone());
                }
            }
        }

        {
            let actor_sessions = self
                .actor_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (actor, session_id) in &*actor_sessions {
                if actor == Self::local_actor() {
                    continue;
                }
                if self.session_exists_and_active(session_id) {
                    bindings
                        .entry(session_id.clone())
                        .or_default()
                        .push(actor.clone());
                }
            }
        }

        let mut grouped: Vec<(String, Vec<String>)> = bindings
            .into_iter()
            .map(|(session_id, mut owners)| {
                owners.sort();
                (session_id, owners)
            })
            .collect();
        grouped.sort_by(|(left_id, left_owners), (right_id, right_owners)| {
            right_owners
                .len()
                .cmp(&left_owners.len())
                .then_with(|| left_id.cmp(right_id))
        });
        grouped
    }

    fn session_exists_and_active(&self, session_id: &str) -> bool {
        self.session_manager().list_sessions().into_iter().any(|s| {
            (s.id.to_string() == session_id || s.name.as_deref() == Some(session_id))
                && s.ended_at.is_none()
        })
    }

    pub(crate) fn resolve_client_session(&self, client: &str) -> String {
        let actor = self.transport_actor(client);
        let sid = self.resolve_actor_session(&actor);
        self.set_client_session(client, &sid);
        sid
    }

    pub(crate) fn set_client_session(&self, client: &str, session_id: &str) {
        let client = Self::normalize_transport(client);
        self.client_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(client.to_string(), session_id.to_string());
        self.save_client_sessions();
    }

    fn tracks_client_session(source: &str) -> bool {
        matches!(source, "rpc" | "http" | "ws" | "socket" | "sock" | "stdio")
    }

    /// Get or create a broadcast sender for a session.
    pub(crate) fn session_broadcast(
        &self,
        session_id: &str,
    ) -> tokio::sync::broadcast::Sender<BroadcastMessage> {
        let mut channels = self
            .session_channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        channels
            .entry(session_id.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0)
            .clone()
    }

    /// Subscribe to a session's event stream.
    pub(crate) fn subscribe_session(
        &self,
        session_id: &str,
    ) -> tokio::sync::broadcast::Receiver<BroadcastMessage> {
        self.session_broadcast(session_id).subscribe()
    }

    /// Execute a Turn in the given session.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limit
    /// is exceeded, or the LLM turn fails.
    fn execute_turn_inner(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        attachments: &[cortex_types::Attachment],
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner_with_scope(
            session_id,
            prompt,
            source,
            attachments,
            inline_images,
            cortex_sdk::ExecutionScope::Foreground,
        )
    }

    fn execute_turn_inner_with_scope(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        attachments: &[cortex_types::Attachment],
        inline_images: &[(String, String)],
        execution_scope: cortex_sdk::ExecutionScope,
    ) -> Result<String, String> {
        if Self::tracks_client_session(source) {
            self.set_client_session(source, session_id);
        }

        // Reject early if API key is not configured
        if self.config().api.api_key.is_empty() {
            return Err(
                "API key not configured. Edit config.toml [api].api_key or reinstall with CORTEX_API_KEY".into(),
            );
        }

        // Rate limit check
        if let crate::rate_limiter::RateLimitResult::SessionLimited
        | crate::rate_limiter::RateLimitResult::GlobalLimited =
            self.rate_limiter.check(session_id)
        {
            return Err("rate limit exceeded".into());
        }

        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let tracer = TracingTurnTracer {
            config: cfg.turn.trace.clone(),
        };
        let actor = self.transport_actor(source);
        let mut session = self.take_or_create_session(session_id);
        let resume = self.resume_for_actor(&actor);
        let history_len_before_turn = session.history.len();
        let result = self.with_registered_turn_control(session_id, |control, on_tpn_complete| {
            let executor = self.build_executor(BuildExecutorInput {
                cfg: &cfg,
                resume: &resume,
                session_id,
                actor: &actor,
                source,
                execution_scope,
                turns_since_extract: session.turns_since_extract,
                skill_summaries,
                retrieved_evidence: &[],
                tracer: &tracer,
                control: Some(control.clone()),
                on_tpn_complete: Some(on_tpn_complete),
            });

            let callbacks = TurnCallbacks { on_event: None };

            let turn_input = crate::turn_executor::TurnInput {
                text: prompt,
                attachments,
                inline_images,
            };
            let gate = RuntimePermissionGate {
                state: self,
                session_id,
                actor: &actor,
                source,
                auto_approve_up_to: cfg.risk.auto_approve_up_to,
                control: Some(&control),
                on_event: None,
            };
            executor.execute(
                &turn_input,
                &mut session.history,
                &gate,
                &mut session.monitor,
                &mut session.summary_cache,
                &callbacks,
            )
        });

        if let Err(error) = &result {
            restore_failed_turn_history(
                &mut session.history,
                history_len_before_turn,
                &crate::turn_executor::TurnInput {
                    text: prompt,
                    attachments,
                    inline_images,
                },
                error,
            );
        }
        let output = self.process_turn_result(&result, &mut session);
        if let (Ok(text), Ok(turn_output)) = (&output, &result) {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::done(text.clone(), turn_output.response_parts.clone()),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
    }

    /// Execute a turn in the given session.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limiting
    /// blocks the turn, or the underlying turn execution fails.
    pub fn execute_turn(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner(session_id, prompt, source, &[], inline_images)
    }

    /// Execute a background turn that should not consume foreground queue
    /// ownership or mark the foreground runtime as busy.
    ///
    /// # Errors
    ///
    /// Returns an error string if the API key is not configured, rate limiting
    /// blocks the turn, or the underlying turn execution fails.
    pub(crate) fn execute_background_turn(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        self.execute_turn_inner_with_scope(
            session_id,
            prompt,
            source,
            &[],
            inline_images,
            cortex_sdk::ExecutionScope::Background,
        )
    }

    /// Execute a Turn with streaming callbacks for SSE delivery.
    ///
    /// Similar to `execute_turn` but wires up a unified event callback so
    /// callers can stream partial user-visible text, observer text, and tool progress.
    fn execute_turn_streaming_inner(
        &self,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        if Self::tracks_client_session(source) {
            self.set_client_session(source, session_id);
        }

        // Reject early if API key is not configured
        if self.config().api.api_key.is_empty() {
            return Err(
                "API key not configured. Edit config.toml [api].api_key or reinstall with CORTEX_API_KEY".into(),
            );
        }

        // Rate limit check
        if let crate::rate_limiter::RateLimitResult::SessionLimited
        | crate::rate_limiter::RateLimitResult::GlobalLimited =
            self.rate_limiter.check(session_id)
        {
            return Err("rate limit exceeded".into());
        }

        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let actor = self.transport_actor(source);
        let mut session = self.take_or_create_session(session_id);
        let resume = self.resume_for_actor(&actor);
        let history_len_before_turn = session.history.len();
        let result = self.with_registered_turn_control(session_id, |control, on_tpn_complete| {
            let executor = self.build_executor(BuildExecutorInput {
                cfg: &cfg,
                resume: &resume,
                session_id,
                actor: &actor,
                source,
                execution_scope: cortex_sdk::ExecutionScope::Foreground,
                turns_since_extract: session.turns_since_extract,
                skill_summaries,
                retrieved_evidence: &[],
                tracer,
                control: Some(control.clone()),
                on_tpn_complete: Some(on_tpn_complete),
            });

            // Wrap callbacks to also broadcast events on the session channel
            let bc_tx = self.session_broadcast(session_id);
            let bc_sid = session_id.to_string();
            let bc_src = source.to_string();
            let wrapped_on_event = move |event: &cortex_turn::orchestrator::TurnStreamEvent| {
                on_event(event);
                if let Some(broadcast_event) = BroadcastEvent::from_turn_stream_event(event) {
                    let _ = bc_tx.send(BroadcastMessage {
                        session_id: bc_sid.clone(),
                        source: bc_src.clone(),
                        event: broadcast_event,
                    });
                }
            };

            let callbacks = TurnCallbacks {
                on_event: Some(&wrapped_on_event),
            };

            let gate = RuntimePermissionGate {
                state: self,
                session_id,
                actor: &actor,
                source,
                auto_approve_up_to: cfg.risk.auto_approve_up_to,
                control: Some(&control),
                on_event: Some(&wrapped_on_event),
            };
            executor.execute(
                input,
                &mut session.history,
                &gate,
                &mut session.monitor,
                &mut session.summary_cache,
                &callbacks,
            )
        });
        if let Err(error) = &result {
            restore_failed_turn_history(
                &mut session.history,
                history_len_before_turn,
                input,
                error,
            );
        }
        let output = self.process_turn_output_result_streaming(result, &mut session);
        if let Ok(turn_output) = &output {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::done(
                    turn_output.response_text.clone().unwrap_or_default(),
                    turn_output.response_parts.clone(),
                ),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
    }

    pub(crate) fn execute_turn_streaming(
        &self,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        self.execute_turn_streaming_inner(session_id, input, source, on_event, tracer)
    }

    pub(crate) fn execute_foreground_turn_streaming(
        &self,
        _foreground: &ForegroundExecution<'_>,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_event: impl Fn(&cortex_turn::orchestrator::TurnStreamEvent) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        self.execute_turn_streaming_inner(session_id, input, source, on_event, tracer)
    }

    /// Build skill summaries for system prompt injection.
    fn build_skill_summaries(&self, cfg: &cortex_types::config::CortexConfig) -> Option<String> {
        use std::fmt::Write as _;
        if !cfg.skills.inject_summaries {
            return None;
        }
        let sums = self
            .skill_registry
            .summaries(cfg.skills.max_active_summaries);
        if sums.is_empty() {
            return None;
        }
        let mut text = String::from("## Skills\n\nReusable procedures available this turn:\n");
        for s in &sums {
            let _ = writeln!(text, "- {}: {}", s.name, s.description);
        }
        Some(text)
    }

    fn resume_for_actor(&self, actor: &str) -> cortex_types::ResumePacket {
        let goals = self
            .goal_store
            .list_open_for_actor(actor)
            .unwrap_or_default()
            .into_iter()
            .take(8)
            .map(|goal| goal.context_line())
            .collect();
        cortex_types::ResumePacket {
            goals,
            ..cortex_types::ResumePacket::default()
        }
    }

    /// Build a `TurnExecutor` with the standard subsystem references.
    fn build_executor<'a>(&'a self, input: BuildExecutorInput<'a>) -> TurnExecutor<'a> {
        let BuildExecutorInput {
            cfg,
            resume,
            session_id,
            actor,
            source,
            execution_scope,
            turns_since_extract,
            skill_summaries,
            retrieved_evidence,
            tracer,
            control,
            on_tpn_complete,
        } = input;
        TurnExecutor::new(TurnExecutorConfig {
            config: cfg,
            journal: &self.journal,
            memory_store: &self.memory_store,
            llm: self.llm.as_ref(),
            tools: &self.tools,
            prompt_manager: &self.prompt_manager,
            embedding_client: self.embedding_client.as_deref(),
            embedding_store: self.embedding_store.as_deref(),
            embedding_health: Some(&*self.embedding_health),
            skill_summaries,
            retrieved_evidence,
            skill_registry: Some(&self.skill_registry),
            data_dir: &self.data_dir,
            max_output_tokens: self.max_output_tokens,
            resume,
            turns_since_extract,
            endpoint_llm: Some(self),
            tracer,
            vision_llm: self.vision_llm.as_deref(),
            control,
            on_tpn_complete,
            session_id,
            actor,
            source,
            execution_scope,
        })
    }

    /// Take a session from the in-memory map or restore/create it.
    fn take_or_create_session(&self, session_id: &str) -> DaemonSession {
        let mut sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        sessions
            .remove(session_id)
            .unwrap_or_else(|| self.restore_or_create_session(session_id))
    }

    /// Process a Turn result: update counters, record metrics, extract text.
    fn process_turn_result(
        &self,
        result: &Result<crate::turn_executor::TurnOutput, String>,
        session: &mut DaemonSession,
    ) -> Result<String, String> {
        match result {
            Ok(output) => {
                self.record_turn_metrics(output);
                self.update_session_after_turn(output, session);
                transport_payloads::extract_final_response_text(output)
            }
            Err(e) => {
                self.metrics.record_turn_error();
                Err(e.clone())
            }
        }
    }

    fn process_turn_output_result_streaming(
        &self,
        result: Result<crate::turn_executor::TurnOutput, String>,
        session: &mut DaemonSession,
    ) -> Result<crate::turn_executor::TurnOutput, String> {
        match result {
            Ok(output) => {
                self.record_turn_metrics(&output);
                self.update_session_after_turn(&output, session);
                if output
                    .response_text
                    .as_ref()
                    .is_some_and(|text| !text.trim().is_empty())
                    || !output.response_parts.is_empty()
                {
                    Ok(output)
                } else {
                    Ok(transport_payloads::synthesize_empty_turn_output(output))
                }
            }
            Err(e) => {
                self.metrics.record_turn_error();
                Err(e)
            }
        }
    }

    fn record_turn_metrics(&self, output: &crate::turn_executor::TurnOutput) {
        self.metrics.record_turn();
        self.metrics.record_tokens(
            output.total_input_tokens as u64,
            output.total_output_tokens as u64,
            output.total_cache_read_input_tokens as u64,
            output.total_cache_creation_input_tokens as u64,
        );
        if output.last_call_input_tokens > 0
            || output.last_call_output_tokens > 0
            || output.last_call_cache_read_input_tokens > 0
            || output.last_call_cache_creation_input_tokens > 0
        {
            self.metrics.record_last_call_tokens(
                output.last_call_input_tokens as u64,
                output.last_call_output_tokens as u64,
                output.last_call_cache_read_input_tokens as u64,
                output.last_call_cache_creation_input_tokens as u64,
            );
        }
        for _ in 0..output.tool_call_count {
            self.metrics.record_tool_call(false);
        }
        for _ in 0..output.tool_error_count {
            self.metrics.record_tool_call(true);
        }
        for _ in 0..output.extracted_memory_count {
            self.metrics.record_memory_capture();
        }
        for _ in &output.alerts {
            self.metrics.record_alert();
        }
    }

    /// Update session counters and heartbeat state after a successful Turn.
    fn update_session_after_turn(
        &self,
        output: &crate::turn_executor::TurnOutput,
        session: &mut DaemonSession,
    ) {
        session.turn_count += 1;
        session.meta.total_input_tokens = session
            .meta
            .total_input_tokens
            .saturating_add(output.total_input_tokens as u64);
        session.meta.total_output_tokens = session
            .meta
            .total_output_tokens
            .saturating_add(output.total_output_tokens as u64);
        session.turns_since_extract += 1;
        // Reset extract counter: after successful extraction, or if we've
        // overshot the threshold (extraction tried but produced nothing).
        let threshold = self.config().memory.extract_min_turns;
        if output.extracted_memory_count > 0 || session.turns_since_extract > threshold {
            session.turns_since_extract = 0;
        }
        if output.extracted_memory_count > 0 {
            let count = u32::try_from(output.extracted_memory_count).unwrap_or(u32::MAX);
            self.heartbeat_state
                .pending_consolidation
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
            self.heartbeat_state
                .pending_embeddings
                .fetch_add(count, std::sync::atomic::Ordering::Relaxed);
        }
    }

    /// Persist session to disk and reinsert into the in-memory map.
    fn persist_and_reinsert(&self, session_id: &str, mut session: DaemonSession) {
        session.meta.turn_count = session.turn_count;
        let _ = self
            .session_store
            .save_history(&session.meta.id, &session.history);
        let _ = self.session_store.save(&session.meta);
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string(), session);
    }

    /// Try to restore a session from disk (preserving history and turn count),
    /// or create a fresh one if the `session_id` doesn't exist on disk.
    /// Ended sessions (with `ended_at` set) are not restored -- a new session
    /// is created instead.
    fn restore_or_create_session(&self, session_id: &str) -> DaemonSession {
        // Try to restore from SessionStore
        if let Some(meta) = self
            .session_store
            .list()
            .into_iter()
            .find(|m| m.id.to_string() == session_id)
        {
            // Do not restore already-ended sessions.
            if meta.ended_at.is_some() {
                return self.new_daemon_session();
            }
            let history = self.session_store.load_history(&meta.id);
            let turn_count = meta.turn_count;
            let cfg = self.config();
            return DaemonSession {
                meta,
                turn_count,
                turns_since_extract: turn_count, // resume from persisted count
                history,
                monitor: MetaMonitor::new(
                    cfg.metacognition.doom_loop_threshold,
                    cfg.metacognition.fatigue_threshold,
                    cfg.metacognition.duration_limit_secs,
                    cfg.metacognition.frame_anchoring_threshold,
                    cfg.metacognition.frame_audit.clone(),
                ),
                summary_cache: SummaryCache::new(),
            };
        }
        self.new_daemon_session()
    }

    fn new_daemon_session(&self) -> DaemonSession {
        let (_, meta) = self.session_manager().create_session();
        let cfg = self.config();
        DaemonSession {
            meta,
            history: Vec::new(),
            turn_count: 0,
            turns_since_extract: 0,
            monitor: MetaMonitor::new(
                cfg.metacognition.doom_loop_threshold,
                cfg.metacognition.fatigue_threshold,
                cfg.metacognition.duration_limit_secs,
                cfg.metacognition.frame_anchoring_threshold,
                cfg.metacognition.frame_audit.clone(),
            ),
            summary_cache: SummaryCache::new(),
        }
    }

    pub(crate) fn end_session(&self, session_id: &str) {
        let removed = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session_id);
        if let Some(mut session) = removed {
            self.session_manager()
                .end_session(&mut session.meta, session.turn_count);
        } else {
            let sm = self.session_manager();
            if let Some(mut meta) = sm
                .list_sessions()
                .into_iter()
                .find(|s| s.id.to_string() == session_id || s.name.as_deref() == Some(session_id))
                && meta.ended_at.is_none()
            {
                let tc = meta.turn_count;
                sm.end_session(&mut meta, tc);
            }
        }
        // Remove the per-session broadcast channel so it can be collected.
        self.session_channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session_id);
    }

    fn control_for_stop(
        &self,
        session_id: Option<&str>,
    ) -> Option<cortex_turn::orchestrator::TurnControl> {
        if let Some(session_id) = session_id {
            return self
                .turn_controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(session_id)
                .cloned();
        }
        let active_session = self
            .active_turn_session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        active_session.as_deref().and_then(|active_session| {
            self.turn_controls
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .get(active_session)
                .cloned()
        })
    }

    fn stop_target_session(&self, session_id: Option<&str>) -> Option<String> {
        session_id.map(str::to_owned).or_else(|| {
            self.active_turn_session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clone()
        })
    }

    fn deny_pending_permissions_for_session(&self, session_id: &str) {
        let pending: Vec<(String, Arc<PendingPermissionEntry>)> = self
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .filter(|(_, entry)| entry.info.session_id == session_id)
            .map(|(id, entry)| (id.clone(), Arc::clone(entry)))
            .collect();
        if pending.is_empty() {
            return;
        }
        for (_, entry) in &pending {
            let _ = entry.resolve(ConfirmationResponse::Denied);
        }
        let mut permissions = self
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (id, _) in pending {
            permissions.remove(&id);
        }
    }

    fn with_registered_turn_control<T>(
        &self,
        session_id: &str,
        execute: impl FnOnce(cortex_turn::orchestrator::TurnControl, OnTpnComplete<'_>) -> T,
    ) -> T {
        let registration = TurnControlRegistration::new(self, session_id);
        let tpn_control = registration.control();
        let release_inbox = move || tpn_control.close_input_window();
        execute(registration.control(), &release_inbox)
    }

    pub(crate) async fn acquire_foreground_execution(
        &self,
        timeout: std::time::Duration,
    ) -> Result<ForegroundExecution<'_>, ForegroundSlotError> {
        match tokio::time::timeout(timeout, self.turn_semaphore.acquire()).await {
            Ok(Ok(permit)) => Ok(ForegroundExecution::queued(permit, &self.heartbeat_state)),
            Ok(Err(_)) => Err(ForegroundSlotError::ShuttingDown),
            Err(_) => Err(ForegroundSlotError::Timeout),
        }
    }

    pub(crate) fn begin_foreground_execution(&self) -> ForegroundExecution<'_> {
        ForegroundExecution::immediate(&self.heartbeat_state)
    }

    fn format_status_for_session(&self, session_id: Option<&str>) -> String {
        use std::fmt::Write as _;

        let snap = self.metrics.snapshot();
        let session_tokens = self.session_token_total(session_id);
        let cfg = self.config().clone();
        let model = cfg.api.model.clone();
        let thinking_output = if cfg.turn.strip_think_tags {
            "request off / output hidden"
        } else {
            "request on / output shown"
        };
        let trace_level = format!("{:?}", cfg.turn.trace.level).to_lowercase();
        let tool_count = self.tools.tool_names().len();
        let pending_memories = self
            .heartbeat_state
            .pending_consolidation
            .load(std::sync::atomic::Ordering::Relaxed);
        let pending_embeddings = self
            .heartbeat_state
            .pending_embeddings
            .load(std::sync::atomic::Ordering::Relaxed);
        let uptime_secs = chrono::Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();
        let uptime = format_duration(uptime_secs);
        let session_count = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let persisted_sessions = self.session_manager().list_sessions();
        let persisted_session_count = persisted_sessions.len();
        let persisted_turn_count: usize = persisted_sessions.iter().map(|s| s.turn_count).sum();
        let journal_event_count = self.journal.event_count().unwrap_or(0);
        let busy = self.turn_semaphore.available_permits() == 0;
        let queue_depth = 1usize.saturating_sub(self.turn_semaphore.available_permits());
        let active_bindings = self.active_session_bindings();
        let shared_bindings: Vec<(String, Vec<String>)> = active_bindings
            .iter()
            .filter(|(_, owners)| owners.len() > 1)
            .cloned()
            .collect();
        let shared_owner_count: usize =
            shared_bindings.iter().map(|(_, owners)| owners.len()).sum();
        let transports = self
            .active_transports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .join(" \u{b7} ");

        let dot = if busy { "\u{1f7e2}" } else { "\u{26aa}" };
        let tool_success = if snap.tool_calls == 0 {
            "n/a".to_string()
        } else {
            format!("{:.0}%", snap.tool_success_rate * 100.0)
        };

        let mut out = String::new();
        let _ = writeln!(
            out,
            "{dot} Cortex v{} \u{b7} {uptime}",
            env!("CARGO_PKG_VERSION")
        );
        let _ = writeln!(out);
        let _ = writeln!(out, "🔄 State      {}", if busy { "busy" } else { "idle" });
        let _ = writeln!(out, "🧠 Model      {model}");
        let _ = writeln!(out, "💭 Thinking   {thinking_output}");
        if !transports.is_empty() {
            let _ = writeln!(out, "🔌 Transports {transports}");
        }
        let _ = writeln!(
            out,
            "🗂️ Sessions   {session_count} active  Queue {queue_depth}  Trace {trace_level}"
        );
        let _ = writeln!(
            out,
            "🔗 Bindings   {} targets  {} shared sessions / {} clients",
            active_bindings.len(),
            shared_bindings.len(),
            shared_owner_count
        );
        let _ = writeln!(out, "🛠️ Tools      {tool_count} loaded");
        let _ = writeln!(out);
        let _ = writeln!(
            out,
            "💬 Turns      {} (errors: {})",
            snap.turn_count, snap.turn_errors
        );
        let _ = writeln!(
            out,
            "💾 Persisted  {persisted_turn_count} turns / {persisted_session_count} sessions / {journal_event_count} events"
        );
        Self::write_status_counters(
            &mut out,
            &snap,
            session_tokens,
            &tool_success,
            pending_memories,
            pending_embeddings,
        );
        Self::write_shared_bindings(&mut out, &shared_bindings);
        out
    }

    fn write_status_counters(
        out: &mut String,
        snap: &crate::metrics::LiveMetrics,
        session_tokens: Option<u64>,
        tool_success: &str,
        pending_memories: u32,
        pending_embeddings: u32,
    ) {
        use std::fmt::Write as _;

        let _ = writeln!(
            out,
            "🪟 Context    call {} in / {} out",
            fmt_tokens(snap.last_call_input_tokens),
            fmt_tokens(snap.last_call_output_tokens),
        );
        let _ = writeln!(
            out,
            "🧊 Cache      call {} read / {} write",
            fmt_tokens(snap.last_call_cache_read_input_tokens),
            fmt_tokens(snap.last_call_cache_creation_input_tokens),
        );
        let session_tokens = session_tokens.map_or_else(|| "n/a".to_string(), fmt_tokens);
        let _ = writeln!(
            out,
            "🧮 Tokens     total {} / session {session_tokens}",
            fmt_tokens(snap.total_tokens),
        );
        let _ = writeln!(
            out,
            "🛠️ Tools run  {} calls / {} errors / {} success",
            snap.tool_calls, snap.tool_errors, tool_success
        );
        let _ = writeln!(
            out,
            "🧠 Memory     {} captures / {} recalls / {} alerts",
            snap.memory_captures, snap.memory_recalls, snap.alerts_fired,
        );
        let _ = writeln!(
            out,
            "📦 Backlog    {pending_memories} consolidate / {pending_embeddings} embed",
        );
    }

    fn write_shared_bindings(out: &mut String, shared_bindings: &[(String, Vec<String>)]) {
        use std::fmt::Write as _;

        if shared_bindings.is_empty() {
            return;
        }

        let _ = writeln!(out);
        for (idx, (session_id, owners)) in shared_bindings.iter().take(5).enumerate() {
            let short_id = &session_id[..session_id.len().min(12)];
            let label = if idx == 0 { "Shared" } else { "          " };
            let _ = writeln!(out, "{label}    {short_id} <= {}", owners.join(", "));
        }
        if shared_bindings.len() > 5 {
            let _ = writeln!(
                out,
                "          ... {} more shared sessions",
                shared_bindings.len() - 5
            );
        }
    }

    /// Inject a message into a running turn.
    pub(crate) fn inject_message(&self, session_id: &str, text: String) -> InjectMessageResult {
        let control = self
            .turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned();
        control.map_or(InjectMessageResult::NoActiveTurn, |control| {
            if control.inject_message(text) {
                InjectMessageResult::Accepted
            } else {
                InjectMessageResult::InputClosed
            }
        })
    }

    #[must_use]
    pub(crate) fn has_active_turn(&self, session_id: &str) -> bool {
        self.turn_controls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(session_id)
    }

    #[must_use]
    pub(crate) fn session_has_recent_user_message(&self, session_id: &str, text: &str) -> bool {
        let in_memory = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .map(|session| session.history.clone());
        let history = in_memory.unwrap_or_else(|| {
            if let Some(meta) = self
                .session_store
                .list()
                .into_iter()
                .find(|meta| meta.id.to_string() == session_id)
            {
                self.session_store.load_history(&meta.id)
            } else {
                Vec::new()
            }
        });

        history
            .iter()
            .rev()
            .filter_map(|message| match message.role {
                cortex_types::Role::User => Some(message.text_content()),
                cortex_types::Role::Assistant => None,
            })
            .take(8)
            .any(|content| content.trim() == text.trim())
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        let uptime = chrono::Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();
        let session_count = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let transports = self
            .active_transports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let metrics = self.metrics.snapshot();
        let auto_approve_up_to = {
            let config = self.config();
            config.risk.auto_approve_up_to
        };
        let auto_approve_up_to = format!("{auto_approve_up_to:?}");

        serde_json::json!({
            "uptime_secs": uptime,
            "session_count": session_count,
            "transports": transports,
            "metrics": {
                "total_input_tokens": metrics.total_input_tokens,
                "total_output_tokens": metrics.total_output_tokens,
                "total_tokens": metrics.total_tokens,
                "last_turn_input_tokens": metrics.last_turn_input_tokens,
                "last_turn_output_tokens": metrics.last_turn_output_tokens,
                "last_turn_tokens": metrics.last_turn_tokens,
                "last_call_input_tokens": metrics.last_call_input_tokens,
                "last_call_output_tokens": metrics.last_call_output_tokens,
                "last_call_tokens": metrics.last_call_tokens,
                "total_cache_read_input_tokens": metrics.total_cache_read_input_tokens,
                "total_cache_creation_input_tokens": metrics.total_cache_creation_input_tokens,
                "last_turn_cache_read_input_tokens": metrics.last_turn_cache_read_input_tokens,
                "last_turn_cache_creation_input_tokens": metrics.last_turn_cache_creation_input_tokens,
                "last_call_cache_read_input_tokens": metrics.last_call_cache_read_input_tokens,
                "last_call_cache_creation_input_tokens": metrics.last_call_cache_creation_input_tokens,
                "turn_count": metrics.turn_count,
                "turn_errors": metrics.turn_errors,
            },
            "risk": {
                "auto_approve_up_to": auto_approve_up_to,
            },
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    pub(crate) fn operator_dashboard(&self, requested_limit: usize) -> serde_json::Value {
        let limit = crate::dashboard::timeline_limit(requested_limit);
        let events = self.journal.recent_events(limit).unwrap_or_default();
        let timeline: Vec<serde_json::Value> = events
            .iter()
            .map(crate::dashboard::timeline_entry)
            .collect();
        let metrics = self.metrics.snapshot();
        let config = self.config().clone();
        let providers = self
            .providers
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let persisted_sessions = self.session_manager().list_sessions();
        let active_session_count = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let active_bindings = self.active_session_bindings();
        let shared_bindings: Vec<serde_json::Value> = active_bindings
            .iter()
            .filter(|(_, owners)| owners.len() > 1)
            .map(|(session_id, owners)| {
                serde_json::json!({
                    "session_id": session_id,
                    "owners": owners,
                })
            })
            .collect();
        let active_transports = self
            .active_transports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let pending_permissions = self
            .pending_permissions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let busy = self.turn_semaphore.available_permits() == 0;
        let queue_depth = 1usize.saturating_sub(self.turn_semaphore.available_permits());
        let registry = cortex_types::ModelCapabilityRegistry::from_config(&config, &providers);
        serde_json::json!({
            "generated_at": chrono::Utc::now().to_rfc3339(),
            "version": env!("CARGO_PKG_VERSION"),
            "state": {
                "busy": busy,
                "queue_depth": queue_depth,
                "trace": format!("{:?}", config.turn.trace.level).to_lowercase(),
                "uptime_secs": chrono::Utc::now()
                    .signed_duration_since(self.start_time)
                    .num_seconds(),
            },
            "provider": {
                "primary": {
                    "provider": &config.api.provider,
                    "model": &config.api.model,
                    "preset": format!("{:?}", config.api.preset).to_lowercase(),
                },
                "profiles": crate::dashboard::model_profiles_json(&registry.profiles),
            },
            "transports": active_transports,
            "sessions": {
                "active": active_session_count,
                "persisted": persisted_sessions.len(),
                "persisted_turns": persisted_sessions.iter().map(|session| session.turn_count).sum::<usize>(),
                "active_bindings": active_bindings.len(),
                "shared_bindings": shared_bindings,
            },
            "tools": {
                "loaded": self.tools.tool_names().len(),
                "pending_permissions": pending_permissions,
            },
            "backlog": {
                "consolidate": self.heartbeat_state.pending_consolidation.load(std::sync::atomic::Ordering::Relaxed),
                "embed": self.heartbeat_state.pending_embeddings.load(std::sync::atomic::Ordering::Relaxed),
            },
            "risk": {
                "auto_approve_up_to": format!("{:?}", config.risk.auto_approve_up_to),
            },
            "metrics": serde_json::to_value(&metrics).unwrap_or_default(),
            "timeline": {
                "limit": limit,
                "counts": crate::dashboard::timeline_counts(&events),
                "events": timeline,
            },
        })
    }

    pub(crate) fn tool_names_for_actor(&self, actor: Option<&str>) -> Vec<String> {
        self.tools.tool_names_for_actor(actor)
    }

    pub(crate) fn skill_registry(&self) -> &cortex_turn::skills::SkillRegistry {
        &self.skill_registry
    }

    /// Read-lock access to the live configuration.
    pub fn config(&self) -> std::sync::RwLockReadGuard<'_, cortex_types::config::CortexConfig> {
        self.config
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[must_use]
    pub(crate) const fn supports_direct_image_input(&self) -> bool {
        self.direct_image_input
    }

    fn init_llm_bindings(
        api_config: &cortex_types::config::ApiConfig,
        providers: &cortex_types::config::ProviderRegistry,
        paths: &cortex_kernel::CortexPaths,
    ) -> Result<LlmBindings, String> {
        let mut endpoint =
            cortex_types::config::ResolvedEndpoint::resolve_primary(api_config, providers)
                .map_err(|e| format!("daemon: resolve endpoint: {e}"))?;
        Self::attach_capability_cache_path(&mut endpoint, paths);
        let llm = cortex_turn::llm::create_llm_client(&endpoint);
        let mut vision_endpoint =
            cortex_types::config::ResolvedEndpoint::resolve_vision_endpoint(api_config, providers)
                .map_err(|e| format!("daemon: resolve vision endpoint: {e}"))?;
        if let Some(endpoint) = &mut vision_endpoint {
            Self::attach_capability_cache_path(endpoint, paths);
            tracing::info!(
                provider = endpoint.provider,
                model = endpoint.model,
                protocol = ?endpoint.protocol,
                "Vision LLM resolved"
            );
        }
        let vision_llm = vision_endpoint
            .as_ref()
            .map(cortex_turn::llm::create_llm_client);
        let direct_image_input = vision_endpoint.as_ref().map_or_else(
            || endpoint.supports_direct_image_input(),
            cortex_types::config::ResolvedEndpoint::supports_direct_image_input,
        );
        Ok(LlmBindings {
            llm,
            vision_llm,
            direct_image_input,
        })
    }

    fn attach_capability_cache_path(
        endpoint: &mut cortex_types::config::ResolvedEndpoint,
        paths: &cortex_kernel::CortexPaths,
    ) {
        endpoint.capability_cache_path = paths
            .model_info_dir()
            .join("model_info.json")
            .to_string_lossy()
            .to_string();
    }

    fn init_group_llms(
        config: &cortex_types::config::CortexConfig,
        providers: &cortex_types::config::ProviderRegistry,
    ) -> HashMap<String, Box<dyn cortex_turn::llm::LlmClient>> {
        let mut group_llms: HashMap<String, Box<dyn cortex_turn::llm::LlmClient>> = HashMap::new();
        for group_name in config.llm_groups.keys() {
            let ep = cortex_types::config::ApiEndpointConfig {
                group: group_name.clone(),
                ..Default::default()
            };
            if let Ok(resolved) = cortex_types::config::ResolvedEndpoint::resolve_with_groups(
                &ep,
                &config.api,
                providers,
                &config.llm_groups,
            ) {
                group_llms.insert(
                    group_name.clone(),
                    cortex_turn::llm::create_llm_client(&resolved),
                );
            }
        }
        group_llms
    }

    /// Load skill registry with layered override (system < instance/plugin).
    fn init_skill_registry(
        home: &Path,
        journal: &Journal,
    ) -> Arc<cortex_turn::skills::SkillRegistry> {
        let skills_dir = cortex_kernel::CortexPaths::from_instance_home(home).skills_dir();
        let system_skills_dir = skills_dir.join("system");
        cortex_turn::skills::defaults::ensure_system_skills(&system_skills_dir);

        let persisted_utilities = journal.load_skill_utilities().unwrap_or_default();
        let skill_registry = cortex_turn::skills::SkillRegistry::new();
        skill_registry.load_utilities(persisted_utilities);
        skill_registry.set_instance_dir(skills_dir.clone());

        for s in cortex_turn::skills::loader::load_skills(
            &system_skills_dir,
            &cortex_types::SkillSource::System,
        ) {
            skill_registry.register(s);
        }
        for s in cortex_turn::skills::loader::load_skills(
            &skills_dir,
            &cortex_types::SkillSource::Instance,
        ) {
            skill_registry.register(s);
        }
        Arc::new(skill_registry)
    }

    /// Create the tool registry with only the skill tool.
    ///
    /// Core tools (`bash`, `read`, `write`, `edit`, `memory_search`, `memory_save`,
    /// `agent`) are registered later by [`init_memory_subsystem`] once the
    /// memory store is available.  Plugin tools (`cron`, `self_modify`,
    /// `delegate_instance`) are loaded separately via the plugin system.
    fn init_tools(
        config: &cortex_types::config::CortexConfig,
        skill_registry: &Arc<cortex_turn::skills::SkillRegistry>,
    ) -> cortex_turn::tools::ToolRegistry {
        let mut tools = cortex_turn::tools::ToolRegistry::new();
        // Skill tool (core — needs SkillRegistry, registered here)
        tools.register(Box::new(cortex_turn::skills::skill_tool::SkillTool::new(
            Arc::clone(skill_registry),
        )));
        tools.apply_disabled_filter(&config.tools.disabled);
        tools
    }

    /// Set up embedding clients, wrap memory store in Arc, register memory tools.
    fn init_memory_subsystem(
        config: &cortex_types::config::CortexConfig,
        providers: &cortex_types::config::ProviderRegistry,
        paths: &cortex_kernel::CortexPaths,
        data_dir: &Path,
        memory_store: cortex_kernel::MemoryStore,
        tools: &mut cortex_turn::tools::ToolRegistry,
        cron_queue: &Arc<cortex_turn::tools::cron::CronQueue>,
    ) -> MemorySubsystem {
        let embedding_client = providers.get(&config.embedding.provider).map(|p| {
            Arc::new(cortex_kernel::EmbeddingClient::new(
                p,
                &config.embedding.api_key,
                &config.embedding.model,
            ))
        });
        let embedding_store = cortex_kernel::EmbeddingStore::open(&paths.embedding_store_path())
            .ok()
            .map(Arc::new);
        let memory_store = Arc::new(memory_store);
        let embedding_health = Arc::new(cortex_turn::memory::recall::EmbeddingHealthStatus::new());

        let recall_ctx = Arc::new(cortex_turn::tools::memory_tools::MemoryRecallComponents {
            store: Arc::clone(&memory_store),
            embedding_client: embedding_client.clone(),
            embedding_store: embedding_store.clone(),
            embedding_health: Some(Arc::clone(&embedding_health)),
            data_dir: data_dir.to_path_buf(),
            max_recall: config.memory.max_recall,
        });
        cortex_turn::tools::register_core_tools(
            tools,
            recall_ctx,
            config.web.clone(),
            config.media.clone(),
            config.acp.clone(),
            &config.api.api_key,
            Arc::clone(cron_queue),
        );

        MemorySubsystem {
            store: memory_store,
            embedding_client,
            embedding_store,
            embedding_health,
        }
    }

    pub(crate) fn memory_store(&self) -> &cortex_kernel::MemoryStore {
        &self.memory_store
    }

    pub(crate) fn task_store(&self) -> &cortex_kernel::TaskStore {
        &self.task_store
    }

    pub(crate) fn goal_store(&self) -> &cortex_kernel::GoalStore {
        &self.goal_store
    }

    pub fn home(&self) -> &Path {
        &self.home_dir
    }

    pub(crate) const fn journal(&self) -> &Journal {
        &self.journal
    }

    pub(crate) const fn sessions(&self) -> &Mutex<HashMap<String, DaemonSession>> {
        &self.sessions
    }

    pub(crate) const fn start_time(&self) -> chrono::DateTime<chrono::Utc> {
        self.start_time
    }

    pub(crate) const fn metrics(&self) -> &crate::metrics::MetricsCollector {
        &self.metrics
    }

    pub(crate) fn heartbeat_state(&self) -> &crate::heartbeat::HeartbeatState {
        &self.heartbeat_state
    }

    /// Handle an MCP method by delegating to `McpServer`.
    ///
    /// Maps the daemon RPC method name (e.g. `mcp/initialize`) to the
    /// MCP protocol method name (e.g. `initialize`). Returns `Ok(result)`
    /// on success or `Err((code, message))` on MCP-level error.
    pub(crate) fn mcp_handle(
        &self,
        method: &str,
        params: &serde_json::Value,
        actor: &str,
    ) -> Result<serde_json::Value, (i32, String)> {
        use cortex_turn::mcp::McpServer;

        // Strip "mcp/" prefix to get original MCP method name
        let mcp_method = method.strip_prefix("mcp/").unwrap_or(method);
        // Remap daemon-friendly names to MCP protocol names
        let mcp_method = match mcp_method {
            "tools-list" => "tools/list",
            "tools-call" => "tools/call",
            other => other,
        };

        let mcp_request = cortex_types::mcp::McpRequest {
            jsonrpc: "2.0".into(),
            method: mcp_method.into(),
            id: 0, // Placeholder -- daemon RPC manages IDs
            params: params.clone(),
        };

        let server = McpServer::new(&self.tools, Some(actor));
        let response = server.handle_request(&mcp_request);

        if let Some(err) = response.error {
            let code = i32::try_from(err.code).unwrap_or(-32_603);
            Err((code, err.message))
        } else {
            Ok(response.result.unwrap_or(serde_json::Value::Null))
        }
    }

    pub fn add_transport(&self, name: &str) {
        self.active_transports
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(name.to_string());
    }

    fn save_all_sessions(&self) {
        let sessions = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for session in sessions.values() {
            let mut meta = session.meta.clone();
            meta.turn_count = session.turn_count;
            let _ = self.session_store.save_history(&meta.id, &session.history);
            let _ = self.session_store.save(&meta);
        }
    }
}

impl crate::turn_executor::EndpointLlmResolver for DaemonState {
    fn resolve(&self, endpoint_name: &str) -> Option<&dyn cortex_turn::llm::LlmClient> {
        let config = self.config();
        let configured_group = config.api.endpoint_group(endpoint_name).map(str::to_string);
        let mut request = cortex_types::ModelRouteRequest::for_endpoint(endpoint_name);
        if let Some(group) = &configured_group {
            request = request.prefer_group(group.clone());
        }
        let providers = self
            .providers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let decision =
            cortex_types::ModelCapabilityRegistry::from_config(&config, &providers).route(&request);
        for line in &decision.explanation {
            tracing::debug!(endpoint = endpoint_name, route = %line, "LLM route decision");
        }
        let selected_group = decision.selected_group().map(str::to_string);
        drop(providers);
        drop(config);
        if let Some(client) = selected_group
            .as_deref()
            .and_then(|group| self.group_llms.get(group))
        {
            return Some(client.as_ref());
        }
        let client = configured_group
            .as_deref()
            .and_then(|group| self.group_llms.get(group))?;
        Some(client.as_ref())
    }
}

impl crate::hot_reload::ReloadTarget for DaemonState {
    fn reload_config(&self) {
        let paths = self.paths();
        let files = paths.config_files();
        let Ok(content) = std::fs::read_to_string(&files.config) else {
            return;
        };
        if toml::from_str::<cortex_types::config::CortexConfig>(&content).is_err() {
            tracing::warn!("Config reload: parse error, keeping current config");
            return;
        }

        let (new_providers, resolved) = match cortex_kernel::load_providers_for_paths(&paths) {
            Ok(value) => value,
            Err(err) => {
                tracing::warn!("Providers reload failed, keeping current providers: {err}");
                return;
            }
        };
        let new_config =
            cortex_kernel::load_config_for_paths(&paths, resolved.as_deref(), &new_providers);
        let old_config = self
            .config
            .read()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let RuntimeBindings {
            actor_aliases,
            transport_actors,
            ..
        } = Self::load_runtime_bindings(&self.data_dir);

        if old_config.api.provider != new_config.api.provider
            || old_config.api.model != new_config.api.model
            || old_config.api.api_key != new_config.api.api_key
        {
            tracing::warn!("Config: LLM provider/model/key changed — restart to apply");
        }

        // Hot-reload tools.disabled filter
        self.tools.apply_disabled_filter(&new_config.tools.disabled);
        self.tools
            .apply_plugin_enabled_filter(&new_config.plugins.enabled);
        *self
            .actor_aliases
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = actor_aliases;
        *self
            .transport_actors
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = transport_actors;

        if let Ok(mut guard) = self.config.write() {
            *guard = new_config.clone();
        }

        if let Ok(mut guard) = self.providers.write() {
            *guard = new_providers;
        }

        if old_config.plugins.enabled != new_config.plugins.enabled {
            let warnings = crate::plugin_loader::reload_process_plugin_tools(
                self.home(),
                &new_config.plugins,
                &self.tools,
            );
            for warning in warnings {
                tracing::warn!(plugin_warning = %warning, "plugin hot-reload warning");
            }
            tracing::info!("Plugin enablement hot-reloaded");
        }

        self.tools.unregister_prefixed_tools("mcp_");
        if !new_config.mcp.servers.is_empty() {
            let warnings = tokio::runtime::Handle::try_current().map_or_else(
                |_| match tokio::runtime::Runtime::new() {
                    Ok(runtime) => runtime.block_on(async {
                        let mcp_manager = cortex_turn::mcp::McpManager::new();
                        mcp_manager
                            .connect_and_register_live(&new_config.mcp, &self.tools)
                            .await
                    }),
                    Err(err) => {
                        tracing::warn!("MCP hot-reload runtime init failed: {err}");
                        Vec::new()
                    }
                },
                |handle| {
                    tokio::task::block_in_place(|| {
                        handle.block_on(async {
                            let mcp_manager = cortex_turn::mcp::McpManager::new();
                            mcp_manager
                                .connect_and_register_live(&new_config.mcp, &self.tools)
                                .await
                        })
                    })
                },
            );
            for warning in warnings {
                tracing::warn!("MCP: {warning}");
            }
        }
        if toml::to_string(&old_config.mcp).ok() != toml::to_string(&new_config.mcp).ok() {
            tracing::info!("MCP tools hot-reloaded");
        }

        tracing::info!("Config reloaded");
    }

    fn restore_config(&self) {
        let paths = self.paths();
        let files = paths.config_files();
        // Structural file deleted — restore default
        if !files.config.exists() {
            let empty = cortex_types::config::ProviderRegistry::new();
            let _ = cortex_kernel::load_config_for_paths(&paths, None, &empty);
            tracing::warn!("config.toml deleted — restored default");
        }
        if !files.providers.exists() {
            let _ = cortex_kernel::load_providers_for_paths(&paths); // (registry, _)
            tracing::warn!("providers.toml deleted — restored default");
        }
        self.reload_config();
    }

    fn reload_prompts(&self) {
        self.prompt_manager.reload();
    }

    fn on_prompt_deleted(&self, path: &std::path::Path) {
        tracing::warn!(
            "Prompt file deleted: {} (not auto-restored — edit is intentional)",
            path.display()
        );
        self.prompt_manager.reload();
    }

    fn reload_skills(&self) {
        self.skill_registry.reload_from(
            &self.paths().skills_dir(),
            &cortex_types::SkillSource::Instance,
        );
    }

    fn on_skill_deleted(&self, path: &std::path::Path) {
        tracing::warn!(
            "Skill file deleted: {} (not auto-restored — edit is intentional)",
            path.display()
        );
        self.reload_skills();
    }

    fn on_plugins_changed(&self, path: &std::path::Path) {
        let cfg = self.config().plugins.clone();
        let warnings =
            crate::plugin_loader::reload_process_plugin_tools(self.home(), &cfg, &self.tools);
        for warning in warnings {
            tracing::warn!(plugin_warning = %warning, "plugin hot-reload warning");
        }
        tracing::info!(
            path = %path.display(),
            "Plugin file changed; process-isolated tools reloaded where possible. In-process native libraries still require daemon restart."
        );
    }
}

// ── DaemonServer ──────────────────────────────────────────────

/// The daemon server that runs all transports concurrently.
pub struct DaemonServer {
    state: Arc<DaemonState>,
    config: DaemonConfig,
}

impl DaemonServer {
    /// Create a new daemon server from a runtime and config.
    ///
    /// # Errors
    ///
    /// Returns an error string if daemon subsystems fail to initialize.
    pub fn new(runtime: &mut CortexRuntime, config: DaemonConfig) -> Result<Self, String> {
        Ok(Self {
            state: Arc::new(DaemonState::from_runtime(runtime)?),
            config,
        })
    }

    /// Run the daemon -- starts all configured transports and blocks until
    /// a shutdown signal is received.
    ///
    /// # Panics
    ///
    /// Panics if the HTTP listener fails to bind.
    pub async fn run(&self) {
        tracing::info!("Starting Cortex daemon...");

        // Start hot-reload before exposing transports so immediate post-start
        // config edits are observed reliably.
        let _hot_reloader =
            crate::hot_reload::HotReloader::start(self.state.home(), Arc::clone(&self.state))
                .map_err(|e| tracing::warn!("Hot-reload watcher failed to start: {e}"))
                .ok();

        let http_handle = self.spawn_http();
        let socket_handle = self.spawn_socket();
        let stdio_handle = if self.config.enable_stdio {
            Some(self.spawn_stdio())
        } else {
            None
        };
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let maintenance_handle =
            self.spawn_heartbeat(Arc::clone(&self.state.heartbeat_state), shutdown_rx.clone());

        // ── Messaging channels ──
        let channel_handles = self.spawn_channels(&shutdown_rx);

        shutdown_signal().await;

        tracing::info!("Shutting down daemon -- saving sessions...");
        // Signal all watchers (heartbeat + channels) to stop gracefully.
        let _ = shutdown_tx.send(true);
        self.state.save_all_sessions();

        let _ = std::fs::remove_file(&self.config.socket_path);

        join_with_grace(
            "heartbeat",
            maintenance_handle,
            std::time::Duration::from_secs(2),
        )
        .await;
        for (idx, handle) in channel_handles.into_iter().enumerate() {
            join_with_grace("channel", handle, std::time::Duration::from_secs(2)).await;
            tracing::debug!(index = idx, "channel task shutdown completed");
        }

        abort_and_join("http", http_handle).await;
        abort_and_join("socket", socket_handle).await;
        if let Some(h) = stdio_handle {
            abort_and_join("stdio", h).await;
        }

        tracing::info!("Daemon stopped.");
    }

    fn spawn_http(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let addr = self.config.http_addr.clone();
        let tls_config = state.config().tls.clone();
        let home_for_tls = self
            .config
            .socket_path
            .parent()
            .map(std::path::Path::to_path_buf);
        let config_path = self
            .config
            .socket_path
            .parent()
            .and_then(std::path::Path::parent)
            .map(|instance_home| {
                cortex_kernel::CortexPaths::from_instance_home(instance_home).config_path()
            });
        state.add_transport("http");

        tokio::spawn(async move {
            let http_state = http_api::build_state(&state);
            let router = http_api::build_router(http_state);

            let addr: std::net::SocketAddr = addr.parse().unwrap_or_else(|e| {
                tracing::error!("Invalid daemon HTTP address: {e}");
                std::net::SocketAddr::from(([127, 0, 0, 1], 0))
            });

            let listener = http_server::bind(addr);
            let actual_addr = listener.local_addr().unwrap_or(addr);
            tracing::info!(addr = %actual_addr, "Daemon HTTP transport listening");

            if addr.port() == 0
                && let Some(ref path) = config_path
            {
                http_server::persist_port_to_config(path, &actual_addr.to_string());
            }

            http_server::serve(listener, router, &tls_config, home_for_tls).await;
        })
    }

    fn spawn_socket(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let socket_path = self.config.socket_path.clone();
        state.add_transport("socket");

        tokio::spawn(async move {
            if socket_path.exists() {
                let _ = std::fs::remove_file(&socket_path);
            }

            let listener = match tokio::net::UnixListener::bind(&socket_path) {
                Ok(l) => l,
                Err(e) => {
                    tracing::error!("Failed to bind Unix socket {}: {e}", socket_path.display());
                    return;
                }
            };
            // Restrict socket permissions to owner only (0700)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ =
                    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o700));
            }
            tracing::info!(path = %socket_path.display(), "Daemon Socket transport listening");

            loop {
                let Ok((stream, _addr)) = listener.accept().await else {
                    break;
                };
                let handler = RpcHandler::new(Arc::clone(&state));
                let conn_state = Arc::clone(&state);
                tokio::spawn(async move {
                    line_protocol::handle_line_protocol(stream, &handler, &conn_state, "socket")
                        .await;
                });
            }
        })
    }

    fn spawn_stdio(&self) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        state.add_transport("stdio");

        tokio::spawn(async move {
            let handler = RpcHandler::new(Arc::clone(&state));
            let stdin = tokio::io::stdin();
            let mut stdout = tokio::io::stdout();
            let reader = BufReader::new(stdin);
            let mut lines = reader.lines();

            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }

                // Try batch (JSON array) first
                if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&line) {
                    let payload = rpc_batch::batch_payload(batch.iter(), |request| {
                        handler.handle_for_client(request, "stdio")
                    });
                    if let Some(json) = payload.and_then(|value| serde_json::to_string(&value).ok())
                    {
                        let _ = stdout.write_all(json.as_bytes()).await;
                        let _ = stdout.write_all(b"\n").await;
                        let _ = stdout.flush().await;
                    }
                    continue;
                }

                // Intercept session/prompt for streaming event delivery.
                if let Ok(req) = rpc::parse_request(&line)
                    && req.method == "session/prompt"
                {
                    line_protocol::handle_streaming_prompt(&req, &mut stdout, &state, "stdio")
                        .await;
                    continue;
                }

                let response = match rpc::parse_request(&line) {
                    Ok(req) => handler.handle_for_client(&req, "stdio"),
                    Err(err_resp) => *err_resp,
                };

                // JSON-RPC 2.0: notifications (null id) must not receive a response.
                if response.id.as_ref().is_some_and(serde_json::Value::is_null)
                    && response.error.is_none()
                {
                    continue;
                }

                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = stdout.write_all(json.as_bytes()).await;
                    let _ = stdout.write_all(b"\n").await;
                    let _ = stdout.flush().await;
                }
            }
        })
    }

    fn spawn_heartbeat(
        &self,
        heartbeat_state: std::sync::Arc<crate::heartbeat::HeartbeatState>,
        mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    ) -> tokio::task::JoinHandle<()> {
        let state = Arc::clone(&self.state);
        let hb_state = heartbeat_state;
        tokio::spawn(async move {
            let cfg = state.config().autonomous.clone();
            if !cfg.enabled {
                tracing::info!("Autonomous cognition disabled");
                // Wait for shutdown signal instead of sleeping forever.
                let _ = shutdown_rx.changed().await;
                return;
            }

            let mut engine = crate::heartbeat::HeartbeatEngine::new(&cfg);
            let mut stability = crate::stability::StabilityMonitor::new();
            let tick_duration = std::time::Duration::from_secs(cfg.heartbeat_interval_secs);
            let mut interval = tokio::time::interval(tick_duration);
            interval.tick().await; // skip immediate first tick

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        let actions = engine.tick(&hb_state);
                        for action in &actions {
                            heartbeat_actions::execute(action, &state, &hb_state, &mut stability);
                        }
                    }
                    _ = shutdown_rx.changed() => {
                        tracing::debug!("Heartbeat received shutdown signal");
                        break;
                    }
                }
            }
        })
    }
}
