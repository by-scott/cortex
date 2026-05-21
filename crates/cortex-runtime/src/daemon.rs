use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};

use cortex_kernel::{Journal, SessionStore};
use cortex_types::ConfirmationResponse;

pub(crate) use crate::rpc::RpcHandler;
use crate::session_manager::SessionManager;

mod bootstrap;
mod broadcast;
mod channel_tasks;
mod config;
mod cron_scheduler;
mod foreground;
mod heartbeat_actions;
mod hot_reload;
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

pub(crate) use self::bootstrap::RuntimeBindings;
pub use self::broadcast::{BroadcastEvent, BroadcastMessage, PendingPermissionInfo};
pub use self::config::DaemonConfig;
pub(crate) use self::foreground::{ForegroundExecution, ForegroundSlotError};
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
    foreground_waiters: AtomicUsize,
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
    cron_queue: Arc<cortex_turn::tools::cron::CronQueue>,
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
    pub(crate) const fn session_manager(&self) -> SessionManager<'_> {
        SessionManager::new(&self.journal, &self.session_store)
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

    pub(crate) fn cron_queue(&self) -> &cortex_turn::tools::cron::CronQueue {
        &self.cron_queue
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
