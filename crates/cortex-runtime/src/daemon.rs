use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use cortex_kernel::{Journal, SessionStore};
use cortex_types::ConfirmationResponse;

pub(crate) use crate::rpc::RpcHandler;
use crate::runtime::CortexRuntime;
use crate::session_manager::SessionManager;

mod broadcast;
mod channel_tasks;
mod config;
mod foreground;
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
mod permissions;
mod rpc_batch;
mod server;
mod session_routing;
mod session_state;
mod slash_commands;
mod sse_stream;
mod status;
mod transport_payloads;
mod turn_control;
mod turn_execution;
mod turn_tasks;
mod turn_tracing;
mod ws_stream;

pub use self::broadcast::{BroadcastEvent, BroadcastMessage, PendingPermissionInfo};
pub use self::config::DaemonConfig;
pub(crate) use self::foreground::ForegroundSlotError;
use self::permissions::PendingPermissionEntry;
pub use self::server::DaemonServer;
use self::session_state::DaemonSession;
pub(crate) use self::turn_tasks::{
    BlockingStreamingTurnRequest, run_blocking_streaming_turn_with_timeout,
    run_blocking_turn_with_timeout,
};
pub(crate) use self::turn_tracing::{ChannelTurnTracer, TracingTurnTracer};

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

    fn format_status_for_session(&self, session_id: Option<&str>) -> String {
        status::format_status_for_session(self, session_id)
    }

    pub(crate) fn status(&self) -> serde_json::Value {
        status::status(self)
    }

    pub(crate) fn operator_dashboard(&self, requested_limit: usize) -> serde_json::Value {
        status::operator_dashboard(self, requested_limit)
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
