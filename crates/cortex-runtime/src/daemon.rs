use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};

use axum::Router;
use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Path as PathParam, Query, Request};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, post};
use futures_util::{SinkExt, StreamExt};
use rust_embed::Embed;

#[derive(Embed)]
#[folder = "../../static/"]
struct StaticAssets;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio_stream::wrappers::ReceiverStream;
use tower_http::cors::CorsLayer;

use cortex_kernel::{Journal, SessionStore};
use cortex_turn::context::SummaryCache;
use cortex_turn::meta::MetaMonitor;
use cortex_types::{Message as CortexMessage, SessionMetadata};

use crate::command_registry::{CommandRegistry, CommandResult, DefaultCommandRegistry};
use crate::rpc::{self, RpcHandler};
use crate::runtime::CortexRuntime;
use crate::session_manager::SessionManager;
use crate::turn_executor::{TurnCallbacks, TurnExecutor, TurnExecutorConfig};

// ── Daemon Configuration ──────────────────────────────────────

/// Configuration for the daemon server.
#[derive(Debug, Clone)]
pub struct DaemonConfig {
    /// HTTP listen address (from `[daemon].addr` in config.toml).
    pub http_addr: String,
    /// Unix socket path (default: `{home}/cortex.sock`).
    pub socket_path: PathBuf,
    /// Whether to enable stdio transport.
    pub enable_stdio: bool,
}

impl DaemonConfig {
    /// Create config from `CortexConfig` and home directory.
    #[must_use]
    pub fn from_config(config: &cortex_types::config::CortexConfig, home: &Path) -> Self {
        Self {
            http_addr: config.daemon.addr.clone(),
            socket_path: home.join("data").join("cortex.sock"),
            enable_stdio: false,
        }
    }

    /// Create default config for the given home directory (random port).
    #[must_use]
    pub fn for_home(home: &Path) -> Self {
        Self {
            http_addr: "127.0.0.1:0".into(),
            socket_path: home.join("data").join("cortex.sock"),
            enable_stdio: false,
        }
    }
}

// ── Per-Session State ─────────────────────────────────────────

pub(crate) struct DaemonSession {
    pub meta: SessionMetadata,
    pub history: Vec<CortexMessage>,
    pub turn_count: usize,
    pub turns_since_extract: usize,
    pub monitor: MetaMonitor,
    pub summary_cache: SummaryCache,
}

// ── Broadcast ────────────────────────────────────────────────

/// A message broadcast to subscribers of a session's event channel.
#[derive(Clone, Debug)]
pub struct BroadcastMessage {
    /// Session ID that produced this message.
    pub session_id: String,
    /// Transport that originated this event (`"tg"`, `"wa"`, `"ws"`, `"sse"`,
    /// `"sock"`, `"rpc"`, `"http"`, `"heartbeat"`, or the channel's session
    /// prefix).  Subscribers use this to skip their own events.
    pub source: String,
    /// Event payload.
    pub event: BroadcastEvent,
}

/// Events broadcast across channels — mirrors the streaming event types.
#[derive(Clone, Debug)]
pub enum BroadcastEvent {
    /// Incremental text chunk during generation.
    Text(String),
    /// Tool invocation progress.
    Tool { name: String, status: String },
    /// Trace event (phase, llm, meta, etc.).
    Trace { category: String, message: String },
    /// Turn completed with final response.
    Done(String),
    /// Error during turn execution.
    Error(String),
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

pub struct DaemonState {
    journal: Journal,
    session_store: SessionStore,
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
    /// Cancellation flag for the running turn.  `/stop` sets this to `true`;
    /// the TPN loop checks it each iteration and exits early.
    turn_cancel_requested: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// Per-session message injection inbox.  When a turn starts, a shared
    /// `Arc<Mutex<Vec<String>>>` is created and stored here.  External callers
    /// push messages into it; the TPN loop drains them each iteration.
    message_inbox: Mutex<HashMap<String, std::sync::Arc<std::sync::Mutex<Vec<String>>>>>,
}

/// RAII guard that sets `foreground_busy` to `true` on creation and
/// resets it to `false` + touches the heartbeat on drop.
struct ForegroundGuard(Arc<crate::heartbeat::HeartbeatState>);

impl ForegroundGuard {
    fn acquire(state: &Arc<crate::heartbeat::HeartbeatState>) -> Self {
        state
            .foreground_busy
            .store(true, std::sync::atomic::Ordering::Relaxed);
        Self(Arc::clone(state))
    }
}

impl Drop for ForegroundGuard {
    fn drop(&mut self) {
        self.0
            .foreground_busy
            .store(false, std::sync::atomic::Ordering::Relaxed);
        self.0.touch();
    }
}

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

/// Turn tracer that emits to both tracing (stderr) and an SSE channel.
struct SseTurnTracer {
    config: cortex_types::config::TurnTraceConfig,
    tx: tokio::sync::mpsc::Sender<Result<SseEvent, std::convert::Infallible>>,
}

impl cortex_turn::orchestrator::TurnTracer for SseTurnTracer {
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

        // Emit to SSE
        let payload = serde_json::json!({
            "category": cat_str,
            "level": format!("{level:?}").to_lowercase(),
            "message": message,
        });
        if let Ok(json) = serde_json::to_string(&payload) {
            let event = SseEvent::default().event("trace").data(json);
            let _ = self.tx.try_send(Ok(event));
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
        let data_dir = rt.data_dir().to_path_buf();
        let config = rt.config().clone();
        let providers = rt.providers().clone();
        let max_output_tokens = rt.max_output_tokens();

        let journal = Journal::open(home.join("data").join("cortex.db"))
            .map_err(|e| format!("daemon: journal open: {e}"))?;
        let session_store = SessionStore::open(&home.join("sessions"))
            .map_err(|e| format!("daemon: session store open: {e}"))?;
        let memory_store = cortex_kernel::MemoryStore::open(&home.join("memory"))
            .map_err(|e| format!("daemon: memory open: {e}"))?;
        let prompt_manager = cortex_kernel::PromptManager::new(&home)
            .map_err(|e| format!("daemon: prompt manager: {e}"))?;

        let endpoint =
            cortex_types::config::ResolvedEndpoint::resolve_primary(&config.api, &providers)
                .map_err(|e| format!("daemon: resolve endpoint: {e}"))?;
        let llm = cortex_turn::llm::create_llm_client(&endpoint);
        let vision_llm = Self::resolve_vision_llm(&endpoint, &config.api, &providers);
        let group_llms = Self::init_group_llms(&config, &providers);
        let skill_registry = Self::init_skill_registry(&home, &journal);

        // Load plugin-contributed skills into the daemon's skill registry.
        for skill_dir in &rt.plugin_skill_dirs {
            skill_registry.reload_from(skill_dir, &cortex_types::SkillSource::Plugin);
        }

        let cron_queue = Arc::new(cortex_turn::tools::cron::CronQueue::open(&data_dir));
        let mut tools = Self::init_tools(&config, &skill_registry);

        // Merge plugin-registered tools from the runtime into the daemon's registry.
        rt.drain_plugin_tools(&mut tools);
        let mem = Self::init_memory_subsystem(
            &config,
            &providers,
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
        crate::introspect_tools::register_introspect_tools(&mut tools, &home, &data_dir);

        Ok(Self {
            journal,
            session_store,
            sessions: Mutex::new(HashMap::new()),
            turn_semaphore: tokio::sync::Semaphore::new(1),
            start_time: chrono::Utc::now(),
            active_transports: Mutex::new(Vec::new()),
            config: RwLock::new(config),
            providers: RwLock::new(providers),
            llm,
            vision_llm,
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
            heartbeat_state: {
                let mut hb = crate::heartbeat::HeartbeatState::new();
                hb.cron_queue = Some(cron_queue);
                Arc::new(hb)
            },
            session_channels: Mutex::new(HashMap::new()),
            turn_cancel_requested: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            message_inbox: Mutex::new(HashMap::new()),
        })
    }

    pub(crate) const fn session_manager(&self) -> SessionManager<'_> {
        SessionManager::new(&self.journal, &self.session_store)
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
    pub fn execute_turn(
        &self,
        session_id: &str,
        prompt: &str,
        source: &str,
        inline_images: &[(String, String)],
    ) -> Result<String, String> {
        // Reset cancellation flag at the start of each turn.
        self.turn_cancel_requested
            .store(false, std::sync::atomic::Ordering::Relaxed);

        // Signal heartbeat engine that foreground is busy.
        let _guard = ForegroundGuard::acquire(&self.heartbeat_state);

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

        let mut session = self.take_or_create_session(session_id);
        let resume = cortex_types::ResumePacket::default();
        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let tracer = TracingTurnTracer {
            config: cfg.turn.trace.clone(),
        };

        let inbox = self.create_turn_inbox(session_id);
        let executor = self.build_executor(
            &cfg,
            &resume,
            &session,
            skill_summaries,
            &tracer,
            Some(inbox),
        );

        let callbacks = TurnCallbacks {
            on_text: None,
            on_tool_progress: None,
        };

        let turn_input = crate::turn_executor::TurnInput {
            text: prompt,
            attachments: &[],
            inline_images,
        };
        let result = executor.execute(
            &turn_input,
            &mut session.history,
            &cortex_turn::risk::AutoApproveGate,
            &mut session.monitor,
            &mut session.summary_cache,
            &callbacks,
        );
        self.remove_turn_inbox(session_id);

        let output = self.process_turn_result(&result, &mut session);
        if let Ok(ref text) = output {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::Done(text.clone()),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
    }

    /// Execute a Turn with streaming callbacks for SSE delivery.
    ///
    /// Similar to `execute_turn` but wires up `on_text` and
    /// `on_tool_progress` callbacks so callers can stream partial results.
    pub(crate) fn execute_turn_streaming(
        &self,
        session_id: &str,
        input: &crate::turn_executor::TurnInput<'_>,
        source: &str,
        on_text: impl Fn(&str) + Send + Sync + 'static,
        on_tool_progress: impl Fn(&cortex_turn::orchestrator::ToolProgress) + Send + Sync + 'static,
        tracer: &dyn cortex_turn::orchestrator::TurnTracer,
    ) -> Result<String, String> {
        // Reset cancellation flag at the start of each turn.
        self.turn_cancel_requested
            .store(false, std::sync::atomic::Ordering::Relaxed);

        let _guard = ForegroundGuard::acquire(&self.heartbeat_state);

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

        let mut session = self.take_or_create_session(session_id);

        let resume = cortex_types::ResumePacket::default();
        let cfg = self.config().clone();
        let skill_summaries = self.build_skill_summaries(&cfg);
        let inbox = self.create_turn_inbox(session_id);
        let executor = self.build_executor(
            &cfg,
            &resume,
            &session,
            skill_summaries,
            tracer,
            Some(inbox),
        );

        // Wrap callbacks to also broadcast events on the session channel
        let bc_tx = self.session_broadcast(session_id);
        let bc_sid = session_id.to_string();
        let bc_src = source.to_string();
        let wrapped_on_text = move |text: &str| {
            on_text(text);
            let _ = bc_tx.send(BroadcastMessage {
                session_id: bc_sid.clone(),
                source: bc_src.clone(),
                event: BroadcastEvent::Text(text.to_string()),
            });
        };
        let bc_tx2 = self.session_broadcast(session_id);
        let bc_sid2 = session_id.to_string();
        let bc_src2 = source.to_string();
        let wrapped_on_tool = move |progress: &cortex_turn::orchestrator::ToolProgress| {
            on_tool_progress(progress);
            let status = match progress.status {
                cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
            };
            let _ = bc_tx2.send(BroadcastMessage {
                session_id: bc_sid2.clone(),
                source: bc_src2.clone(),
                event: BroadcastEvent::Tool {
                    name: progress.tool_name.clone(),
                    status: status.to_string(),
                },
            });
        };

        let callbacks = TurnCallbacks {
            on_text: Some(&wrapped_on_text),
            on_tool_progress: Some(&wrapped_on_tool),
        };

        let result = executor.execute(
            input,
            &mut session.history,
            &cortex_turn::risk::AutoApproveGate,
            &mut session.monitor,
            &mut session.summary_cache,
            &callbacks,
        );

        self.remove_turn_inbox(session_id);
        let output = self.process_turn_result_streaming(&result, &mut session);
        if let Ok(ref text) = output {
            let _ = self.session_broadcast(session_id).send(BroadcastMessage {
                session_id: session_id.to_string(),
                source: source.to_string(),
                event: BroadcastEvent::Done(text.clone()),
            });
        }
        self.persist_and_reinsert(session_id, session);
        output
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
        let mut text = String::from("# Available Skills\n\n");
        for s in &sums {
            let _ = writeln!(text, "- **{}**: {}", s.name, s.description);
        }
        Some(text)
    }

    /// Build a `TurnExecutor` with the standard subsystem references.
    fn build_executor<'a>(
        &'a self,
        cfg: &'a cortex_types::config::CortexConfig,
        resume: &'a cortex_types::ResumePacket,
        session: &DaemonSession,
        skill_summaries: Option<String>,
        tracer: &'a dyn cortex_turn::orchestrator::TurnTracer,
        inbox: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
    ) -> TurnExecutor<'a> {
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
            skill_registry: Some(&self.skill_registry),
            data_dir: &self.data_dir,
            max_output_tokens: self.max_output_tokens,
            resume,
            turns_since_extract: session.turns_since_extract,
            endpoint_llm: Some(self),
            tracer,
            vision_llm: self.vision_llm.as_deref(),
            cancel_flag: Some(std::sync::Arc::clone(&self.turn_cancel_requested)),
            message_inbox: inbox,
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
                self.metrics.record_turn();
                self.metrics.record_tokens(
                    output.total_input_tokens as u64,
                    output.total_output_tokens as u64,
                );
                for _ in 0..output.tool_call_count {
                    self.metrics.record_tool_call(false);
                }
                for _ in 0..output.tool_error_count {
                    self.metrics.record_tool_call(true);
                }
                self.update_session_after_turn(output, session);
                for _ in &output.alerts {
                    self.metrics.record_alert();
                }
                Ok(output.response_text.clone().unwrap_or_default())
            }
            Err(e) => {
                self.metrics.record_turn_error();
                Err(e.clone())
            }
        }
    }

    /// Process a streaming Turn result (no extra metrics recording).
    fn process_turn_result_streaming(
        &self,
        result: &Result<crate::turn_executor::TurnOutput, String>,
        session: &mut DaemonSession,
    ) -> Result<String, String> {
        match result {
            Ok(output) => {
                self.update_session_after_turn(output, session);
                Ok(output.response_text.clone().unwrap_or_default())
            }
            Err(e) => Err(e.clone()),
        }
    }

    /// Update session counters and heartbeat state after a successful Turn.
    fn update_session_after_turn(
        &self,
        output: &crate::turn_executor::TurnOutput,
        session: &mut DaemonSession,
    ) {
        session.turn_count += 1;
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

    pub fn dispatch_command(&self, command: &str) -> String {
        let trimmed = command.trim();

        // Handle daemon-level commands that need DaemonState access.
        if trimmed == "/stop" {
            self.turn_cancel_requested
                .store(true, std::sync::atomic::Ordering::Relaxed);
            return "Turn cancellation requested.".into();
        }
        if trimmed == "/status" {
            return self.format_status();
        }

        let sm = self.session_manager();
        // Reuse a transient session context — commands don't need persistent sessions.
        let mut sid = cortex_types::SessionId::new();
        let mut meta = cortex_types::SessionMetadata::new(sid, 0);
        let mut history = Vec::new();
        let mut turn_count = 0;

        let registry = DefaultCommandRegistry::new();
        let cfg = self.config().clone();
        let providers = self
            .providers
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let mut ctx = crate::command_registry::CommandContext {
            session_manager: &sm,
            session_meta: &mut meta,
            session_id: &mut sid,
            history: &mut history,
            turn_count: &mut turn_count,
            config: &cfg,
            providers: &providers,
        };

        match registry.dispatch(command, &mut ctx) {
            CommandResult::Output(text) => text,
            CommandResult::Exit => "exit".into(),
            CommandResult::NotFound(msg) => msg,
        }
    }

    fn format_status(&self) -> String {
        use std::fmt::Write as _;

        let snap = self.metrics.snapshot();
        let duration_limit = self.config().metacognition.duration_limit_secs;
        let uptime = chrono::Utc::now()
            .signed_duration_since(self.start_time)
            .num_seconds();
        let session_count = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .len();
        let busy = self
            .turn_cancel_requested
            .load(std::sync::atomic::Ordering::Relaxed);

        let mut out = String::new();
        let _ = writeln!(out, "Cortex v{}", env!("CARGO_PKG_VERSION"));
        let _ = writeln!(out, "Uptime: {uptime}s  Sessions: {session_count}");
        let _ = writeln!(
            out,
            "Max output tokens: {}  Duration limit: {duration_limit}s",
            self.max_output_tokens
        );
        let _ = writeln!(
            out,
            "Turns: {} (errors: {})  Turn active: {busy}",
            snap.turn_count, snap.turn_errors
        );
        let _ = writeln!(
            out,
            "Tokens: {} in / {} out / {} total",
            snap.total_input_tokens, snap.total_output_tokens, snap.total_tokens
        );
        let _ = writeln!(
            out,
            "Tools: {} calls ({} errors, {:.0}% success)",
            snap.tool_calls,
            snap.tool_errors,
            snap.tool_success_rate * 100.0
        );
        let _ = writeln!(
            out,
            "Memory: {} captures / {} recalls  Alerts: {}",
            snap.memory_captures, snap.memory_recalls, snap.alerts_fired
        );
        out
    }

    /// Inject a message into a running turn.  Returns `true` if the message
    /// was accepted (turn in progress — it will appear in the next TPN
    /// iteration), `false` if no turn is running.
    pub(crate) fn inject_message(&self, session_id: &str, text: String) -> bool {
        let inbox = self
            .message_inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .cloned();
        if let Some(inbox) = inbox {
            inbox
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(text);
            return true;
        }
        false
    }

    /// Create a shared inbox for a turn and register it.
    fn create_turn_inbox(&self, session_id: &str) -> std::sync::Arc<std::sync::Mutex<Vec<String>>> {
        let inbox = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        self.message_inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(session_id.to_string(), std::sync::Arc::clone(&inbox));
        inbox
    }

    /// Remove the inbox for a session (called when a turn finishes).
    fn remove_turn_inbox(&self, session_id: &str) {
        self.message_inbox
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(session_id);
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

        serde_json::json!({
            "uptime_secs": uptime,
            "session_count": session_count,
            "transports": transports,
            "version": env!("CARGO_PKG_VERSION"),
        })
    }

    pub(crate) fn tool_names(&self) -> Vec<String> {
        self.tools.tool_names()
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

    /// Create per-group LLM clients for sub-endpoint routing.
    /// Resolve a vision-capable LLM client for image turns.
    ///
    /// Priority:
    /// 1. Explicit `[api.vision]` config (provider + model)
    /// 2. Provider's `vision_model` field from `providers.toml`
    /// 3. None (primary LLM handles images natively)
    fn resolve_vision_llm(
        primary: &cortex_types::config::ResolvedEndpoint,
        api_config: &cortex_types::config::ApiConfig,
        providers: &cortex_types::config::ProviderRegistry,
    ) -> Option<Box<dyn cortex_turn::llm::LlmClient>> {
        // Level 1: explicit [api.vision] config
        if let Ok(Some(vision_ep)) =
            cortex_types::config::ResolvedEndpoint::resolve_vision(api_config, providers)
        {
            tracing::info!(
                model = vision_ep.model,
                "Vision LLM resolved from [api.vision] config"
            );
            return Some(cortex_turn::llm::create_llm_client(&vision_ep));
        }

        // Level 2: provider's vision_model field
        let provider = providers.get(&api_config.provider)?;
        if provider.vision_model.is_empty() {
            return None;
        }
        let mut vision_ep = primary.clone();
        vision_ep.model.clone_from(&provider.vision_model);
        tracing::info!(
            model = vision_ep.model,
            "Vision LLM resolved from provider vision_model"
        );
        Some(cortex_turn::llm::create_llm_client(&vision_ep))
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

    /// Load skill registry with three-tier override (system < instance < project).
    fn init_skill_registry(
        home: &Path,
        journal: &Journal,
    ) -> Arc<cortex_turn::skills::SkillRegistry> {
        let skills_dir = home.join("skills");
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
        let embedding_store =
            cortex_kernel::EmbeddingStore::open(&data_dir.join("embedding_store.db"))
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
        let media_api_key = config
            .media
            .effective_api_key(&config.api.api_key)
            .to_string();
        cortex_turn::tools::register_core_tools(
            tools,
            recall_ctx,
            config.web.clone(),
            config.media.clone(),
            media_api_key,
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

        let server = McpServer::new(&self.tools);
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
        let sm = self.session_manager();
        for session in sessions.values() {
            let mut meta = session.meta.clone();
            sm.end_session(&mut meta, session.turn_count);
        }
    }
}

impl crate::turn_executor::EndpointLlmResolver for DaemonState {
    fn resolve(&self, endpoint_name: &str) -> Option<&dyn cortex_turn::llm::LlmClient> {
        let group_name = self.config().api.endpoint_group(endpoint_name)?.to_string();
        let client = self.group_llms.get(group_name.as_str())?;
        Some(client.as_ref())
    }
}

impl crate::hot_reload::ReloadTarget for DaemonState {
    fn reload_config(&self) {
        let config_path = self.home_dir.join("config.toml");
        let Ok(content) = std::fs::read_to_string(&config_path) else {
            return;
        };
        let Ok(new_config) = toml::from_str::<cortex_types::config::CortexConfig>(&content) else {
            tracing::warn!("Config reload: parse error, keeping current config");
            return;
        };
        if let Ok(old) = self.config.read()
            && (old.api.provider != new_config.api.provider
                || old.api.model != new_config.api.model
                || old.api.api_key != new_config.api.api_key)
        {
            tracing::warn!("Config: LLM provider/model/key changed — restart to apply");
        }

        // Hot-reload tools.disabled filter
        self.tools.apply_disabled_filter(&new_config.tools.disabled);

        if let Ok(mut guard) = self.config.write() {
            *guard = new_config;
        }

        // Hot-reload providers.toml
        let base = self.home_dir.parent().unwrap_or(&self.home_dir);
        if let Ok((new_providers, _)) = cortex_kernel::load_providers(base)
            && let Ok(mut guard) = self.providers.write()
        {
            *guard = new_providers;
        }

        tracing::info!("Config reloaded");
    }

    fn restore_config(&self) {
        // Structural file deleted — restore default
        let config_path = self.home_dir.join("config.toml");
        if !config_path.exists() {
            let empty = cortex_types::config::ProviderRegistry::new();
            let _ = cortex_kernel::load_config(&self.home_dir, None, &empty);
            tracing::warn!("config.toml deleted — restored default");
        }
        let base = self.home_dir.parent().unwrap_or(&self.home_dir);
        let providers_path = base.join("providers.toml");
        if !providers_path.exists() {
            let _ = cortex_kernel::load_providers(base); // (registry, _)
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
            &self.home_dir.join("skills"),
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

        // Start real-time hot-reload watcher for prompts + skills
        let _hot_reloader =
            crate::hot_reload::HotReloader::start(self.state.home(), Arc::clone(&self.state))
                .map_err(|e| tracing::warn!("Hot-reload watcher failed to start: {e}"))
                .ok();

        shutdown_signal().await;

        tracing::info!("Shutting down daemon -- saving sessions...");
        // Signal all watchers (heartbeat + channels) to stop gracefully.
        let _ = shutdown_tx.send(true);
        self.state.save_all_sessions();

        let _ = std::fs::remove_file(&self.config.socket_path);

        http_handle.abort();
        socket_handle.abort();
        maintenance_handle.abort();
        if let Some(h) = stdio_handle {
            h.abort();
        }
        for h in channel_handles {
            h.abort();
        }

        tracing::info!("Daemon stopped.");
    }

    /// Spawn messaging channel tasks (Telegram, `WhatsApp`) based on config
    /// and `auth.json` files. Returns handles for cleanup on shutdown.
    fn spawn_channels(
        &self,
        shutdown_rx: &tokio::sync::watch::Receiver<bool>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        let mut handles = Vec::new();
        let home = self.state.home();

        // ── Telegram (from channels/telegram/auth.json) ──
        if let Some(tg_auth) = crate::channels::read_channel_auth(home, "telegram") {
            let tg_token = tg_auth
                .get("bot_token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            let tg_mode = tg_auth
                .get("mode")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("polling")
                .to_string();
            let tg_webhook_addr = tg_auth
                .get("webhook_addr")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();

            if !tg_token.is_empty() {
                let store = crate::channels::store::ChannelStore::open(home, "telegram");
                let channel = Arc::new(crate::channels::telegram::TelegramChannel::new(
                    tg_token,
                    store,
                    Arc::clone(&self.state),
                ));
                self.state.add_transport("telegram");

                let rx = shutdown_rx.clone();
                handles.push(tokio::spawn(async move {
                    if tg_mode == "webhook" && !tg_webhook_addr.is_empty() {
                        channel.run_webhook(&tg_webhook_addr, rx).await;
                    } else {
                        channel.run_polling(rx).await;
                    }
                }));
                tracing::info!("Telegram channel started");
            }
        }

        // ── WhatsApp Cloud (from channels/whatsapp/auth.json) ──
        if let Some(wa_auth) = crate::channels::read_channel_auth(home, "whatsapp") {
            let wa_token = wa_auth
                .get("access_token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            let phone_id = wa_auth
                .get("phone_number_id")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            let verify = wa_auth
                .get("verify_token")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("")
                .to_string();
            let addr = wa_auth
                .get("webhook_addr")
                .and_then(serde_json::Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("127.0.0.1:8444")
                .to_string();

            if !wa_token.is_empty() && !phone_id.is_empty() {
                let store = crate::channels::store::ChannelStore::open(home, "whatsapp");
                let channel = Arc::new(crate::channels::whatsapp::WhatsAppCloudChannel::new(
                    wa_token,
                    phone_id,
                    verify,
                    store,
                    Arc::clone(&self.state),
                ));
                self.state.add_transport("whatsapp");

                let rx = shutdown_rx.clone();
                handles.push(tokio::spawn(async move {
                    channel.run_webhook(&addr, rx).await;
                }));
                tracing::info!("WhatsApp Cloud channel started");
            }
        }

        handles
    }

    /// CORS layer: allow only localhost origins with restricted methods/headers.
    fn localhost_cors() -> CorsLayer {
        use axum::http::{Method, header};
        use tower_http::cors::AllowOrigin;
        CorsLayer::new()
            .allow_origin(AllowOrigin::predicate(|origin, _| {
                origin.to_str().is_ok_and(|s| {
                    s.starts_with("http://localhost")
                        || s.starts_with("http://127.0.0.1")
                        || s.starts_with("https://localhost")
                        || s.starts_with("https://127.0.0.1")
                })
            }))
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION, header::ACCEPT])
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
            .map(|p| p.join("config.toml"));
        state.add_transport("http");

        tokio::spawn(async move {
            let http_state = Self::build_http_state(&state);
            let router = Self::build_http_router(http_state);

            let addr: std::net::SocketAddr = addr.parse().unwrap_or_else(|e| {
                tracing::error!("Invalid daemon HTTP address: {e}");
                "127.0.0.1:0".parse().expect("fallback addr is valid")
            });

            let listener = bind_http(addr);
            let actual_addr = listener.local_addr().unwrap_or(addr);
            tracing::info!(addr = %actual_addr, "Daemon HTTP transport listening");

            if addr.port() == 0
                && let Some(ref path) = config_path
            {
                persist_port_to_config(path, &actual_addr.to_string());
            }

            serve_http(listener, router, &tls_config, home_for_tls).await;
        })
    }

    fn build_http_state(state: &Arc<DaemonState>) -> HttpState {
        let handler = Arc::new(RpcHandler::new(Arc::clone(state)));
        HttpState {
            handler,
            daemon: Arc::clone(state),
        }
    }

    fn build_http_router(http_state: HttpState) -> Router<()> {
        use axum::middleware as mw;

        let auth_daemon = Arc::clone(&http_state.daemon);
        let auth_layer = mw::from_fn(move |req: Request, next: Next| {
            let cfg = auth_daemon.config().auth.clone();
            async move { auth_check(cfg, req, next).await }
        });

        let rate_limiter_state = Arc::clone(&http_state.daemon);
        let rate_limit_layer = mw::from_fn(move |req: Request, next: Next| {
            let rl = Arc::clone(&rate_limiter_state);
            async move {
                if req.method() == axum::http::Method::POST {
                    // Use would_allow (check-only, no recording) to avoid
                    // double-counting: individual handlers record via check().
                    let result = rl.rate_limiter.would_allow("__http_global__");
                    if result == crate::rate_limiter::RateLimitResult::GlobalLimited {
                        return (
                            StatusCode::TOO_MANY_REQUESTS,
                            [(
                                axum::http::header::HeaderName::from_static("retry-after"),
                                axum::http::header::HeaderValue::from_static("5"),
                            )],
                            Json(serde_json::json!({"error": "rate limit exceeded"})),
                        )
                            .into_response();
                    }
                }
                next.run(req).await
            }
        });

        let protected = Router::new()
            .route("/api/sessions", get(handle_sessions_list))
            .route("/api/session", post(handle_session_create))
            .route("/api/session/{id}", get(handle_session_get_http))
            .route("/api/turn", post(handle_turn))
            .route(
                "/api/memory",
                get(handle_memory_list).post(handle_memory_save_http),
            )
            .route("/api/meta/alerts", get(handle_meta_alerts))
            .route("/api/audit/summary", get(handle_audit_summary))
            .route("/api/audit/health", get(handle_audit_health))
            .route(
                "/api/audit/decision-path/{id}",
                get(handle_audit_decision_path),
            )
            .route("/api/rpc", post(handle_http_rpc))
            .route("/api/daemon/status", get(handle_http_status))
            .route("/api/turn/stream", post(handle_turn_stream))
            .route("/api/ws", get(handle_ws_upgrade))
            .layer(auth_layer)
            .layer(rate_limit_layer)
            .layer(axum::extract::DefaultBodyLimit::max(2 * 1024 * 1024));

        Router::new()
            .route("/api/health", get(handle_health))
            .route("/api/metrics/structured", get(handle_metrics_structured))
            .merge(protected)
            .layer(Self::localhost_cors())
            .layer(axum::middleware::from_fn(reject_non_localhost_preflight))
            .layer(axum::middleware::from_fn(security_headers))
            .fallback(serve_embedded_static)
            .with_state(http_state)
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
                    handle_line_protocol(stream, &handler, &conn_state).await;
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
                    let responses: Vec<rpc::RpcResponse> =
                        batch.iter().map(|r| handler.handle(r)).collect();
                    if let Ok(json) = serde_json::to_string(&responses) {
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
                    handle_streaming_prompt(&req, &mut stdout, &state).await;
                    continue;
                }

                let response = match rpc::parse_request(&line) {
                    Ok(req) => handler.handle(&req),
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
                            execute_heartbeat_action(action, &state, &hb_state, &mut stability);
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

// ── Heartbeat action dispatch ─────────────────────────────────

fn execute_heartbeat_action(
    action: &crate::heartbeat::HeartbeatAction,
    state: &DaemonState,
    hb: &crate::heartbeat::HeartbeatState,
    stability: &mut crate::stability::StabilityMonitor,
) {
    match action {
        crate::heartbeat::HeartbeatAction::DeprecateExpired => {
            heartbeat_deprecate_expired(state);
        }
        crate::heartbeat::HeartbeatAction::EmbedPending => {
            heartbeat_embed_pending(state, hb);
        }
        crate::heartbeat::HeartbeatAction::ConsolidateMemories => {
            heartbeat_consolidate(state, hb);
        }
        crate::heartbeat::HeartbeatAction::EvolveSkills => {
            heartbeat_evolve_skills(state, hb);
        }
        crate::heartbeat::HeartbeatAction::Checkpoint => {
            heartbeat_checkpoint(state, stability);
        }
        crate::heartbeat::HeartbeatAction::SelfUpdate => {
            heartbeat_autonomous_turn(
                state,
                hb,
                "self-update",
                "Analyze recent interactions and determine if any prompts \
                 (soul/identity/behavioral/user) should be updated based on \
                 accumulated corrections and feedback.",
                |hb_inner| {
                    hb_inner
                        .correction_count
                        .store(0, std::sync::atomic::Ordering::Relaxed);
                },
            );
        }
        crate::heartbeat::HeartbeatAction::DeepReflection => {
            heartbeat_autonomous_turn(
                state,
                hb,
                "reflection",
                "Reflect on recent work. What patterns have emerged? \
                 What could be improved? Are there any unresolved issues \
                 or insights worth remembering?",
                |hb_inner| {
                    hb_inner.touch();
                },
            );
        }
        crate::heartbeat::HeartbeatAction::CronDue(prompt) => {
            heartbeat_autonomous_turn(state, hb, "cron", prompt, |_| {});
        }
    }
}

fn heartbeat_deprecate_expired(state: &DaemonState) {
    let n = cortex_turn::memory::deprecate_expired(state.memory_store(), 0.05).unwrap_or(0);
    if n > 0 {
        tracing::debug!(deprecated = n, "Heartbeat: deprecate");
    }
}

fn heartbeat_embed_pending(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;
    hb.pending_embeddings.store(0, Relaxed);
    let (Some(client), Some(cache)) = (
        state.embedding_client.as_ref(),
        state.embedding_store.as_ref(),
    ) else {
        return;
    };
    let memories = state.memory_store().list_all().unwrap_or_default();
    let mut embedded = 0usize;
    let mut vec_table_ready = false;
    for m in &memories {
        let hash = cortex_kernel::embedding_store::content_hash(&m.content);
        if cache.get(&hash).is_none()
            && let Ok(vec) = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(client.embed(&m.content))
            })
            && !vec.is_empty()
        {
            // Lazily create the vec0 table on first embedding.
            if !vec_table_ready {
                let _ = cache.ensure_vector_table(vec.len());
                vec_table_ready = true;
            }
            let _ = cache.put(&hash, client.model_name(), &vec);
            let _ = cache.upsert_vector(&m.id, &vec);
            embedded += 1;
        } else if !vec_table_ready {
            // If the embedding was already cached, ensure the vec0 table
            // exists using its dimension, then backfill the vector index.
            if let Some(cached) = cache.get(&hash)
                && !cached.is_empty()
            {
                let _ = cache.ensure_vector_table(cached.len());
                vec_table_ready = true;
                let _ = cache.upsert_vector(&m.id, &cached);
            }
        } else {
            // Vec table is ready; backfill from cache if needed.
            if let Some(cached) = cache.get(&hash) {
                let _ = cache.upsert_vector(&m.id, &cached);
            }
        }
    }
    if embedded > 0 {
        tracing::info!(count = embedded, "Heartbeat: embedded pending memories");
    }
}

fn heartbeat_consolidate(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;
    let store = state.memory_store();
    let mut mem = store.list_all().unwrap_or_default();
    let r = cortex_turn::memory::consolidate::consolidate_memories(&mut mem);
    cortex_turn::memory::consolidate::upgrade_episodic_to_semantic(&mut mem);
    cortex_turn::memory::consolidate::apply_decay(&mut mem, 0.05, chrono::Utc::now());
    for m in &mem {
        let _ = store.save(m);
    }
    hb.pending_consolidation.store(0, Relaxed);
    if r.upgraded > 0 {
        tracing::debug!(upgraded = r.upgraded, "Heartbeat: consolidate");
    }
}

fn heartbeat_evolve_skills(state: &DaemonState, hb: &crate::heartbeat::HeartbeatState) {
    use std::sync::atomic::Ordering::Relaxed;
    if let Some(evo) = state.skill_registry().evolve() {
        for name in &evo.created {
            tracing::info!(skill = %name, "Heartbeat: new skill");
        }
    }
    for (name, score) in state.skill_registry().utility_snapshot() {
        let _ = state.journal().save_skill_utility(&name, score);
    }
    hb.tool_calls_since_evolve.store(0, Relaxed);
}

fn heartbeat_checkpoint(state: &DaemonState, stability: &mut crate::stability::StabilityMonitor) {
    let _ = state.journal().gc_unreferenced_blobs();
    let _ = state.journal().create_checkpoint();
    let count = state.journal().event_count().unwrap_or(0);
    stability.record_snapshot(0, count, 0);
    if stability.sample_count() >= 3 {
        let report = stability.generate_report();
        if !report.is_stable {
            tracing::warn!("Stability: {:?}", report.growth_rates);
        }
    }
}

fn heartbeat_autonomous_turn(
    state: &DaemonState,
    hb: &crate::heartbeat::HeartbeatState,
    label: &str,
    prompt: &str,
    on_success: impl FnOnce(&crate::heartbeat::HeartbeatState),
) {
    tracing::info!("Heartbeat: {label} triggered");
    let session_id = format!("autonomous-{label}-{}", chrono::Utc::now().timestamp());
    match state.execute_turn(&session_id, prompt, "heartbeat", &[]) {
        Ok(_) => {
            on_success(hb);
            hb.record_llm_call();
            tracing::info!("Heartbeat: {label} completed");
        }
        Err(e) => tracing::warn!("Heartbeat: {label} failed: {e}"),
    }
}

// ── HTTP Handlers ─────────────────────────────────────────────

#[derive(Clone)]
struct HttpState {
    handler: Arc<RpcHandler>,
    daemon: Arc<DaemonState>,
}

async fn handle_http_rpc(
    State(state): State<HttpState>,
    headers: axum::http::HeaderMap,
    body: String,
) -> impl IntoResponse {
    // Require JSON content type for RPC requests
    let ct = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    if !ct.is_empty() && !ct.contains("json") {
        let resp = rpc::RpcResponse {
            jsonrpc: "2.0".into(),
            id: Some(serde_json::Value::Null),
            result: None,
            error: Some(rpc::RpcError {
                code: -32700,
                message: format!("Unsupported Content-Type: {ct} (expected application/json)"),
                data: None,
            }),
        };
        return (
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            Json(serde_json::to_value(resp).unwrap_or_default()),
        )
            .into_response();
    }
    // Try batch (JSON array) first
    if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&body) {
        let responses: Vec<rpc::RpcResponse> =
            batch.iter().map(|r| state.handler.handle(r)).collect();
        return (
            StatusCode::OK,
            Json(serde_json::to_value(responses).unwrap_or_default()),
        )
            .into_response();
    }
    // Single request
    let response = match rpc::parse_request(&body) {
        Ok(req) => {
            let is_notification = req.id.is_null();
            let resp = state.handler.handle(&req);
            if is_notification {
                return StatusCode::NO_CONTENT.into_response();
            }
            resp
        }
        Err(err_resp) => *err_resp,
    };
    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap_or_default()),
    )
        .into_response()
}

async fn serve_embedded_static(uri: axum::http::Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    let path = if path.is_empty() { "index.html" } else { path };
    match StaticAssets::get(path) {
        Some(content) => {
            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();
            (
                StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, mime)],
                content.data.to_vec(),
            )
                .into_response()
        }
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

async fn handle_http_status(State(state): State<HttpState>) -> impl IntoResponse {
    let status = state.daemon.status();
    (StatusCode::OK, Json(status))
}

#[derive(serde::Deserialize)]
struct TurnStreamRequest {
    session_id: Option<String>,
    input: String,
    #[serde(default)]
    images: Vec<cortex_types::web::ImageData>,
}

/// SSE event wrapper for serialization into `data:` fields.
#[derive(serde::Serialize)]
#[serde(untagged)]
enum SsePayload {
    Text {
        content: String,
    },
    Tool {
        tool_name: String,
        status: &'static str,
    },
    Done {
        session_id: String,
        response: String,
    },
    Error {
        message: String,
    },
}

/// Create an SSE stream that emits a single error event then closes.
async fn sse_error_stream(
    message: String,
) -> Sse<
    axum::response::sse::KeepAliveStream<
        ReceiverStream<Result<SseEvent, std::convert::Infallible>>,
    >,
> {
    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(1);
    let payload = SsePayload::Error { message };
    if let Ok(json) = serde_json::to_string(&payload) {
        let _ = tx
            .send(Ok(SseEvent::default().event("error").data(json)))
            .await;
    }
    drop(tx);
    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

async fn handle_turn_stream(
    State(state): State<HttpState>,
    Json(req): Json<TurnStreamRequest>,
) -> impl IntoResponse {
    let session_id = req
        .session_id
        .unwrap_or_else(|| cortex_types::SessionId::new().to_string());
    let input = req.input;
    let inline_images = images_to_inline(&req.images);

    if input.trim().is_empty() {
        return sse_error_stream("input must not be empty".into()).await;
    }
    if let Err(msg) = validate_session_id(&session_id) {
        return sse_error_stream(msg).await;
    }

    let (tx, rx) = tokio::sync::mpsc::channel::<Result<SseEvent, std::convert::Infallible>>(64);

    let daemon = Arc::clone(&state.daemon);
    tokio::spawn(async move {
        // Serialize foreground turns (GWT: one task at a time).
        let Ok(permit) = daemon.turn_semaphore.acquire().await else {
            return;
        };
        permit.forget();

        let tx_text = tx.clone();
        let tx_tool = tx.clone();
        let tx_trace = tx.clone();
        let tx_final = tx;
        let sid_for_done = session_id.clone();

        let (timeout_secs, trace_config) = {
            let cfg = daemon.config();
            (cfg.turn.tool_timeout_secs, cfg.turn.trace.clone())
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            tokio::task::spawn_blocking(move || {
                let tracer = SseTurnTracer {
                    config: trace_config,
                    tx: tx_trace,
                };
                let turn_input = crate::turn_executor::TurnInput {
                    text: &input,
                    attachments: &[],
                    inline_images: &inline_images,
                };
                let result = daemon.execute_turn_streaming(
                    &session_id,
                    &turn_input,
                    "sse",
                    move |text| {
                        let payload = SsePayload::Text {
                            content: text.to_string(),
                        };
                        if let Ok(json) = serde_json::to_string(&payload) {
                            let event = SseEvent::default().event("text").data(json);
                            let _ = tx_text.try_send(Ok(event));
                        }
                    },
                    move |progress| {
                        let status_str = match progress.status {
                            cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                            cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                            cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
                        };
                        let payload = SsePayload::Tool {
                            tool_name: progress.tool_name.clone(),
                            status: status_str,
                        };
                        if let Ok(json) = serde_json::to_string(&payload) {
                            let event = SseEvent::default().event("tool").data(json);
                            let _ = tx_tool.try_send(Ok(event));
                        }
                    },
                    &tracer,
                );
                // Release semaphore right after execution, before post-turn.
                daemon.turn_semaphore.add_permits(1);
                result
            }),
        )
        .await;
        let result = result.map_or_else(
            |_| Err("turn execution timed out".into()),
            |join_result| join_result.unwrap_or_else(|e| Err(e.to_string())),
        );

        let final_event = match result {
            Ok(response) => {
                let payload = SsePayload::Done {
                    session_id: sid_for_done,
                    response,
                };
                let json = serde_json::to_string(&payload).unwrap_or_default();
                SseEvent::default().event("done").data(json)
            }
            Err(msg) => {
                let payload = SsePayload::Error { message: msg };
                let json = serde_json::to_string(&payload).unwrap_or_default();
                SseEvent::default().event("error").data(json)
            }
        };
        let _ = tx_final.send(Ok(final_event)).await;
    });

    Sse::new(ReceiverStream::new(rx)).keep_alive(KeepAlive::default())
}

// ── REST API Handlers ────────────────────────────────────────

#[derive(serde::Deserialize)]
struct SessionsListQuery {
    limit: Option<usize>,
    offset: Option<usize>,
}

async fn handle_sessions_list(
    State(state): State<HttpState>,
    Query(query): Query<SessionsListQuery>,
) -> impl IntoResponse {
    let all = state.daemon.session_manager().list_sessions();
    let offset = query.offset.unwrap_or(0);
    let limit = query.limit.unwrap_or(100);
    let page: Vec<_> = all.into_iter().skip(offset).take(limit).collect();
    (
        StatusCode::OK,
        Json(serde_json::to_value(page).unwrap_or_default()),
    )
}

async fn handle_session_get_http(
    State(state): State<HttpState>,
    PathParam(id): PathParam<String>,
) -> axum::response::Response {
    let sessions = state.daemon.session_manager().list_sessions();
    let found = sessions
        .iter()
        .find(|s| s.id.to_string() == id || s.name.as_deref() == Some(&id));
    found.map_or_else(
        || {
            (
                StatusCode::NOT_FOUND,
                Json(serde_json::json!({"error": "session not found"})),
            )
                .into_response()
        },
        |s| {
            (
                StatusCode::OK,
                Json(serde_json::to_value(s).unwrap_or_default()),
            )
                .into_response()
        },
    )
}

async fn handle_session_create(
    State(state): State<HttpState>,
    body: Option<Json<serde_json::Value>>,
) -> impl IntoResponse {
    // Accept optional user-supplied session_id
    let user_sid = body.and_then(|Json(v)| v.get("session_id")?.as_str().map(String::from));

    let session_count = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    if session_count >= 10_000 {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({"error": "maximum session count reached"})),
        );
    }

    if let Some(ref sid) = user_sid {
        if sid.is_empty() {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "session_id must not be empty" })),
            );
        }
        if sid.len() > 256
            || !sid
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
        {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid session_id" })),
            );
        }
        // Check both in-memory map and persisted store for duplicates
        let in_memory = state
            .daemon
            .sessions()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .contains_key(sid);
        let on_disk = state
            .daemon
            .session_manager()
            .list_sessions()
            .iter()
            .any(|s| s.id.to_string() == *sid || s.name.as_deref() == Some(sid.as_str()));
        if in_memory || on_disk {
            return (
                StatusCode::CONFLICT,
                Json(serde_json::json!({ "error": "session already exists", "session_id": sid })),
            );
        }
        let (created_id, meta) = state.daemon.session_manager().create_session_with_id(sid);
        // Return the user's original ID if it was stored as name (non-UUID input),
        // otherwise return the UUID that was used directly.
        let returned_id = meta
            .name
            .as_deref()
            .unwrap_or(&created_id.to_string())
            .to_string();
        return (
            StatusCode::CREATED,
            Json(serde_json::json!({ "session_id": returned_id })),
        );
    }

    let (session_id, _meta) = state.daemon.session_manager().create_session();
    (
        StatusCode::CREATED,
        Json(serde_json::json!({ "session_id": session_id.to_string() })),
    )
}

#[derive(serde::Deserialize)]
struct TurnRequest {
    session_id: String,
    input: String,
    #[serde(default)]
    images: Vec<cortex_types::web::ImageData>,
}

/// Validate turn input: reject empty input and malformed session IDs.
fn validate_turn_input(session_id: &str, input: &str) -> Result<(), String> {
    if input.trim().is_empty() {
        return Err("input must not be empty".into());
    }
    validate_session_id(session_id)
}

/// Session ID: max 256 chars, alphanumeric + hyphen + underscore + dot.
fn validate_session_id(session_id: &str) -> Result<(), String> {
    if session_id.is_empty() {
        return Err("session_id must not be empty".into());
    }
    if session_id.len() > 256 {
        return Err("session_id exceeds 256 characters".into());
    }
    if !session_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        return Err(
            "session_id must contain only alphanumeric, hyphen, underscore, or dot characters"
                .into(),
        );
    }
    Ok(())
}

/// Reject OPTIONS preflight requests from non-localhost origins with 403.
/// This prevents tower-http `CorsLayer` from sending CORS headers for
/// disallowed origins on preflight requests.
async fn reject_non_localhost_preflight(req: Request, next: Next) -> axum::response::Response {
    if req.method() == axum::http::Method::OPTIONS
        && let Some(origin) = req
            .headers()
            .get(axum::http::header::ORIGIN)
            .and_then(|v| v.to_str().ok())
    {
        let is_localhost = origin.starts_with("http://localhost")
            || origin.starts_with("http://127.0.0.1")
            || origin.starts_with("https://localhost")
            || origin.starts_with("https://127.0.0.1");
        if !is_localhost {
            return (StatusCode::FORBIDDEN, "CORS: origin not allowed").into_response();
        }
    }
    next.run(req).await
}

/// Security headers middleware: add standard hardening headers to all responses.
async fn security_headers(req: Request, next: Next) -> axum::response::Response {
    let mut resp = next.run(req).await;
    let headers = resp.headers_mut();
    headers.insert(
        axum::http::header::HeaderName::from_static("x-content-type-options"),
        axum::http::header::HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("x-frame-options"),
        axum::http::header::HeaderValue::from_static("DENY"),
    );
    headers.insert(
        axum::http::header::HeaderName::from_static("referrer-policy"),
        axum::http::header::HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    resp
}

/// Auth check: when `auth.enabled`, require a valid Bearer JWT.
async fn auth_check(
    auth_config: cortex_types::config::AuthConfig,
    req: Request,
    next: Next,
) -> axum::response::Response {
    if !auth_config.enabled || auth_config.secret.is_empty() {
        return next.run(req).await;
    }

    let token = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    match token {
        Some(t) => {
            let mut validation = jsonwebtoken::Validation::default();
            validation.set_required_spec_claims(&["sub", "exp", "iat"]);
            match jsonwebtoken::decode::<serde_json::Value>(
                t,
                &jsonwebtoken::DecodingKey::from_secret(auth_config.secret.as_bytes()),
                &validation,
            ) {
                Ok(_) => next.run(req).await,
                Err(_) => (
                    StatusCode::UNAUTHORIZED,
                    Json(serde_json::json!({"error": "invalid or expired token"})),
                )
                    .into_response(),
            }
        }
        None => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({"error": "missing Authorization header"})),
        )
            .into_response(),
    }
}

/// Convert web API `ImageData` to `(mime_type, base64_data)` pairs for the turn executor.
fn images_to_inline(images: &[cortex_types::web::ImageData]) -> Vec<(String, String)> {
    images
        .iter()
        .map(|img| (img.media_type.clone(), img.data.clone()))
        .collect()
}

async fn handle_turn(
    State(state): State<HttpState>,
    Json(req): Json<TurnRequest>,
) -> axum::response::Response {
    let daemon = Arc::clone(&state.daemon);
    let session_id = req.session_id;
    let input = req.input;
    let inline_images = images_to_inline(&req.images);

    if let Err(msg) = validate_turn_input(&session_id, &input) {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response();
    }

    // Intercept known slash commands; unknown ones fall through to LLM
    if input.starts_with('/') {
        let d = Arc::clone(&daemon);
        let cmd_input = input.clone();
        let cmd_sid = session_id.clone();
        let is_not_found = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let is_nf = is_not_found.clone();
        let cmd_result = tokio::task::spawn_blocking(move || {
            let result = d.dispatch_command(&cmd_input);
            // "Unknown command:" prefix signals NotFound — fall through to LLM
            if result.starts_with("Unknown command:") {
                is_nf.store(true, std::sync::atomic::Ordering::Relaxed);
            }
            result
        })
        .await
        .unwrap_or_else(|e| e.to_string());

        if !is_not_found.load(std::sync::atomic::Ordering::Relaxed) {
            return (
                StatusCode::OK,
                Json(serde_json::json!({ "session_id": cmd_sid, "response": cmd_result })),
            )
                .into_response();
        }
        // NotFound: fall through to LLM below
    }

    // Rate limit check BEFORE semaphore — reject fast without queueing.
    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited =
        daemon.rate_limiter.check(&session_id)
    {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::HeaderName::from_static("retry-after"),
                axum::http::header::HeaderValue::from_static("5"),
            )],
            Json(serde_json::json!({ "error": "rate limit exceeded" })),
        )
            .into_response();
    }

    // Serialize foreground turns via semaphore (GWT: one task at a time).
    // Acquire before spawn_blocking to queue concurrent requests instead of
    // crashing from nested block_in_place / runtime conflicts.
    {
        let Ok(permit) = daemon.turn_semaphore.acquire().await else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "service shutting down" })),
            )
                .into_response();
        };
        // Forget the permit — we manually release after the turn completes.
        // This avoids lifetime issues with spawn_blocking ('static bound).
        permit.forget();
    }
    let sid = session_id.clone();
    let timeout_secs = {
        let cfg = daemon.config();
        cfg.turn.tool_timeout_secs
    };
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::task::spawn_blocking(move || {
            let result = daemon.execute_turn(&sid, &input, "http", &inline_images);
            daemon.turn_semaphore.add_permits(1);
            result
        }),
    )
    .await;
    let result = result.map_or_else(
        |_| Err("turn execution timed out".into()),
        |join_result| join_result.unwrap_or_else(|e| Err(e.to_string())),
    );

    match result {
        Ok(response) => (
            StatusCode::OK,
            Json(serde_json::json!({ "session_id": session_id, "response": response })),
        )
            .into_response(),
        Err(msg) if msg.contains("rate limit") => (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::HeaderName::from_static("retry-after"),
                axum::http::header::HeaderValue::from_static("5"),
            )],
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
        Err(msg) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": msg })),
        )
            .into_response(),
    }
}

async fn handle_memory_list(State(state): State<HttpState>) -> impl IntoResponse {
    let memories = state.daemon.memory_store().list_all().unwrap_or_default();
    (
        StatusCode::OK,
        Json(serde_json::to_value(memories).unwrap_or_default()),
    )
}

async fn handle_memory_save_http(
    State(state): State<HttpState>,
    Json(body): Json<serde_json::Value>,
) -> axum::response::Response {
    let content = body.get("content").and_then(|v| v.as_str()).unwrap_or("");
    if content.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({"error": "missing content"})),
        )
            .into_response();
    }
    let memory_type: cortex_types::MemoryType = body
        .get("memory_type")
        .or_else(|| body.get("type"))
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(cortex_types::MemoryType::User);
    let kind: cortex_types::MemoryKind = body
        .get("kind")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or(cortex_types::MemoryKind::Episodic);
    let description = body
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let entry = cortex_types::MemoryEntry::new(content, description, memory_type, kind);
    let id = entry.id.clone();
    match state.daemon.memory_store().save(&entry) {
        Ok(()) => {
            state
                .daemon
                .heartbeat_state()
                .pending_embeddings
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            (
                StatusCode::CREATED,
                Json(serde_json::json!({"id": id, "status": "saved"})),
            )
                .into_response()
        }
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("{e}")})),
        )
            .into_response(),
    }
}

#[derive(serde::Deserialize)]
struct AlertsQuery {
    session_id: Option<String>,
}

async fn handle_meta_alerts(
    State(state): State<HttpState>,
    Query(query): Query<AlertsQuery>,
) -> impl IntoResponse {
    let sessions = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);

    let alerts: Vec<serde_json::Value> = query.session_id.as_ref().map_or_else(Vec::new, |sid| {
        sessions
            .get(sid)
            .map(|session| {
                session
                    .monitor
                    .check()
                    .into_iter()
                    .map(|a| {
                        serde_json::json!({ "kind": format!("{:?}", a.kind), "message": a.message })
                    })
                    .collect()
            })
            .unwrap_or_default()
    });

    (StatusCode::OK, Json(serde_json::json!(alerts)))
}

async fn handle_health(State(state): State<HttpState>) -> impl IntoResponse {
    let uptime = chrono::Utc::now()
        .signed_duration_since(state.daemon.start_time())
        .num_seconds();
    let session_count = state
        .daemon
        .sessions()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .len();
    (
        StatusCode::OK,
        Json(serde_json::json!({
            "status": "ok",
            "uptime_secs": uptime,
            "session_count": session_count,
        })),
    )
}

async fn handle_metrics_structured(State(state): State<HttpState>) -> impl IntoResponse {
    let live = state.daemon.metrics().snapshot();
    (
        StatusCode::OK,
        Json(serde_json::to_value(&live).unwrap_or_default()),
    )
}

async fn handle_audit_summary(State(state): State<HttpState>) -> impl IntoResponse {
    let events = state
        .daemon
        .journal()
        .recent_events(500)
        .unwrap_or_default();
    let summary = cortex_turn::observability::AuditAggregator::summarize(&events);
    (
        StatusCode::OK,
        Json(serde_json::to_value(summary).unwrap_or_default()),
    )
}

async fn handle_audit_health(State(state): State<HttpState>) -> impl IntoResponse {
    let events = state
        .daemon
        .journal()
        .recent_events(500)
        .unwrap_or_default();
    let summary = cortex_turn::observability::AuditAggregator::summarize(&events);

    let health_score = if summary.turn_count == 0 {
        1.0
    } else {
        let alert_ratio = f64::from(u32::try_from(summary.meta_alert_count).unwrap_or(u32::MAX))
            / f64::from(u32::try_from(summary.turn_count).unwrap_or(u32::MAX));
        (1.0 - alert_ratio)
            .clamp(0.0, 1.0)
            .mul_add(0.5, summary.avg_confidence * 0.5)
    };

    (
        StatusCode::OK,
        Json(serde_json::json!({
            "health_score": health_score,
            "total_events": summary.total_events,
            "turn_count": summary.turn_count,
            "tool_call_count": summary.tool_call_count,
            "avg_confidence": summary.avg_confidence,
            "meta_alert_count": summary.meta_alert_count,
        })),
    )
}

async fn handle_audit_decision_path(
    State(state): State<HttpState>,
    PathParam(id): PathParam<String>,
) -> axum::response::Response {
    let events = state
        .daemon
        .journal()
        .recent_events(1000)
        .unwrap_or_default();
    let path = cortex_turn::observability::AuditAggregator::extract_decision_path(&events, &id);
    if path.steps.is_empty() && path.outcome.is_empty() {
        return (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({"error": "decision path not found"})),
        )
            .into_response();
    }
    (
        StatusCode::OK,
        Json(serde_json::to_value(path).unwrap_or_default()),
    )
        .into_response()
}

// ── WebSocket Handler ────────────────────────────────────────

async fn handle_ws_upgrade(
    ws: WebSocketUpgrade,
    State(state): State<HttpState>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_ws_connection(socket, state))
}

async fn handle_ws_connection(socket: WebSocket, state: HttpState) {
    let (mut ws_sender, mut ws_receiver) = socket.split();
    let daemon = Arc::clone(&state.daemon);
    let handler = RpcHandler::new(Arc::clone(&daemon));

    while let Some(Ok(msg)) = ws_receiver.next().await {
        let Message::Text(text) = msg else { continue };
        let trimmed = text.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Parse JSON-RPC request
        let Ok(req) = rpc::parse_request(trimmed) else {
            let err = serde_json::json!({
                "event": "error",
                "data": {"message": "invalid JSON-RPC request"}
            });
            let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
            continue;
        };

        if req.method == "session/prompt" {
            handle_ws_streaming_prompt(&daemon, &mut ws_sender, &req).await;
        } else {
            // Synchronous RPC methods
            let resp = handler.handle(&req);
            if let Ok(json) = serde_json::to_string(&resp) {
                let _ = ws_sender.send(Message::Text(json.into())).await;
            }
        }
    }
}

/// Handle `session/prompt` over WebSocket with streaming events.
///
/// Emits the same NDJSON event format (`text`, `tool`, `trace`, `done`,
/// `error`) as the socket/stdio transports, each as a separate WebSocket
/// text message.
async fn handle_ws_streaming_prompt(
    daemon: &Arc<DaemonState>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    req: &rpc::RpcRequest,
) {
    let prompt = req
        .params
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if prompt.trim().is_empty() {
        ws_send_error(ws_sender, "missing prompt").await;
        return;
    }

    let session_id = req
        .params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| cortex_types::SessionId::new().to_string(), String::from);

    if let Err(msg) = validate_session_id(&session_id) {
        ws_send_error(ws_sender, &msg).await;
        return;
    }

    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited =
        daemon.rate_limiter.check(&session_id)
    {
        ws_send_error(ws_sender, "rate limit exceeded").await;
        return;
    }

    let Ok(Ok(permit)) = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        daemon.turn_semaphore.acquire(),
    )
    .await
    else {
        ws_send_error(
            ws_sender,
            "another turn is in progress -- timed out after 30s",
        )
        .await;
        return;
    };
    permit.forget();

    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    let result = execute_ws_turn(daemon, &session_id, prompt, tx, &mut rx, ws_sender).await;

    let done_event = match result {
        Ok(response) => serde_json::json!({
            "event": "done",
            "data": {"session_id": session_id, "response": response}
        }),
        Err(msg) => serde_json::json!({
            "event": "error",
            "data": {"message": msg}
        }),
    };
    let _ = ws_sender
        .send(Message::Text(done_event.to_string().into()))
        .await;
}

/// Execute a streaming turn and pipe events through a channel to a WebSocket.
async fn execute_ws_turn(
    daemon: &Arc<DaemonState>,
    session_id: &str,
    prompt: &str,
    tx: tokio::sync::mpsc::Sender<String>,
    rx: &mut tokio::sync::mpsc::Receiver<String>,
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
) -> Result<String, String> {
    let state = Arc::clone(daemon);
    let sid = session_id.to_string();
    let prompt_text = prompt.to_string();
    let tx_text = tx.clone();
    let tx_tool = tx.clone();
    let tx_trace = tx.clone();

    let (timeout_secs, trace_config) = {
        let cfg = state.config();
        (cfg.turn.tool_timeout_secs, cfg.turn.trace.clone())
    };

    let join = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::task::spawn_blocking(move || {
            let tracer = ChannelTurnTracer {
                config: trace_config,
                tx: tx_trace,
            };
            let turn_input = crate::turn_executor::TurnInput {
                text: &prompt_text,
                attachments: &[],
                inline_images: &[],
            };
            let result = state.execute_turn_streaming(
                &sid,
                &turn_input,
                "ws",
                move |text| {
                    let evt = serde_json::json!({"event":"text","data":{"content":text}});
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = tx_text.try_send(json);
                    }
                },
                move |progress| {
                    let status_str = match progress.status {
                        cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                        cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                        cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
                    };
                    let evt = serde_json::json!({"event":"tool","data":{"tool_name":progress.tool_name,"status":status_str}});
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = tx_tool.try_send(json);
                    }
                },
                &tracer,
            );
            state.turn_semaphore.add_permits(1);
            result
        }),
    );

    drop(tx);

    tokio::pin!(join);
    let mut join_done = false;
    let mut final_result: Option<Result<String, String>> = None;

    loop {
        if join_done && final_result.is_some() {
            while let Ok(line) = rx.try_recv() {
                let _ = ws_sender.send(Message::Text(line.into())).await;
            }
            break;
        }
        tokio::select! {
            biased;
            Some(line) = rx.recv() => {
                let _ = ws_sender.send(Message::Text(line.into())).await;
            }
            result = &mut join, if !join_done => {
                join_done = true;
                final_result = Some(match result {
                    Ok(Ok(Ok(response))) => Ok(response),
                    Ok(Ok(Err(e))) => Err(e),
                    Ok(Err(join_err)) => Err(join_err.to_string()),
                    Err(_) => Err("turn execution timed out".into()),
                });
            }
            else => break,
        }
    }

    final_result.unwrap_or_else(|| Err("unexpected end".into()))
}

async fn ws_send_error(
    ws_sender: &mut futures_util::stream::SplitSink<WebSocket, Message>,
    message: &str,
) {
    let err = serde_json::json!({"event":"error","data":{"message":message}});
    let _ = ws_sender.send(Message::Text(err.to_string().into())).await;
}

// ── Line Protocol Handler ─────────────────────────────────────

async fn handle_line_protocol<S>(stream: S, handler: &RpcHandler, state: &Arc<DaemonState>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    let (reader, mut writer) = tokio::io::split(stream);
    // 64 KB buffer handles large prompts (e.g. multi-KB Chinese text).
    let buf_reader = BufReader::with_capacity(64 * 1024, reader);
    let mut lines = buf_reader.lines();

    while let Ok(Some(line)) = lines.next_line().await {
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        // handler.handle() uses block_in_place internally, which requires
        // running on a tokio worker thread (not spawn_blocking's thread pool).
        // Tool execution itself runs in scoped OS threads, so this won't block.

        // Try batch (JSON array) first
        if let Ok(batch) = serde_json::from_str::<Vec<rpc::RpcRequest>>(&line) {
            let responses: Vec<rpc::RpcResponse> =
                batch.iter().map(|r| handler.handle(r)).collect();
            if let Ok(json) = serde_json::to_string(&responses) {
                let _ = writer.write_all(json.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.flush().await;
            }
            continue;
        }

        // Intercept session/prompt for streaming event delivery.
        if let Ok(req) = rpc::parse_request(&line)
            && req.method == "session/prompt"
        {
            handle_streaming_prompt(&req, &mut writer, state).await;
            continue;
        }

        let response = match rpc::parse_request(&line) {
            Ok(req) => handler.handle(&req),
            Err(err_resp) => *err_resp,
        };

        // JSON-RPC 2.0: notifications (null id) must not receive a response.
        if response.id.as_ref().is_some_and(serde_json::Value::is_null) && response.error.is_none()
        {
            continue;
        }

        if let Ok(json) = serde_json::to_string(&response) {
            let _ = writer.write_all(json.as_bytes()).await;
            let _ = writer.write_all(b"\n").await;
            let _ = writer.flush().await;
        }
    }
}

/// Write an NDJSON error event and flush.
async fn write_error_event<W: tokio::io::AsyncWrite + Unpin>(writer: &mut W, message: &str) {
    let evt = serde_json::json!({"event":"error","data":{"message": message}});
    let _ = writer.write_all(evt.to_string().as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.flush().await;
}

/// Handle `session/prompt` with streaming events (shared by socket and stdio).
///
/// Emits NDJSON event lines (`text`, `tool`, `trace`) as the turn
/// executes, finishing with a `done` or `error` event.
async fn handle_streaming_prompt<W>(req: &rpc::RpcRequest, writer: &mut W, state: &Arc<DaemonState>)
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let prompt = req
        .params
        .get("prompt")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");

    if prompt.trim().is_empty() {
        write_error_event(writer, "missing prompt parameter").await;
        return;
    }

    // Resolve session id (use provided, or generate a new one).
    let session_id = req
        .params
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .map_or_else(|| cortex_types::SessionId::new().to_string(), String::from);

    if let Err(msg) = validate_session_id(&session_id) {
        write_error_event(writer, &msg).await;
        return;
    }

    // Rate limit check before queueing on the semaphore.
    if let crate::rate_limiter::RateLimitResult::SessionLimited
    | crate::rate_limiter::RateLimitResult::GlobalLimited = state.rate_limiter.check(&session_id)
    {
        write_error_event(writer, "rate limit exceeded").await;
        return;
    }

    // Serialize foreground turns (GWT: one task at a time).
    let Ok(Ok(permit)) = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        state.turn_semaphore.acquire(),
    )
    .await
    else {
        write_error_event(writer, "another turn is in progress -- timed out after 30s").await;
        return;
    };
    permit.forget();

    let final_result = execute_streaming_turn(state, &session_id, prompt, writer).await;

    // Send the final done or error event.
    let done_event = match final_result {
        Ok(response) => serde_json::json!({
            "event": "done",
            "data": {"session_id": session_id, "response": response}
        }),
        Err(msg) => serde_json::json!({
            "event": "error",
            "data": {"message": msg}
        }),
    };
    let _ = writer.write_all(done_event.to_string().as_bytes()).await;
    let _ = writer.write_all(b"\n").await;
    let _ = writer.flush().await;
}

/// Spawn the turn in a blocking thread and stream events to the writer.
///
/// Returns the final turn response or an error message.
async fn execute_streaming_turn<W>(
    state: &Arc<DaemonState>,
    session_id: &str,
    prompt: &str,
    writer: &mut W,
) -> Result<String, String>
where
    W: tokio::io::AsyncWrite + Unpin,
{
    let (tx, mut rx) = tokio::sync::mpsc::channel::<String>(64);

    let daemon = Arc::clone(state);
    let sid = session_id.to_string();
    let prompt_text = prompt.to_string();
    let tx_text = tx.clone();
    let tx_tool = tx.clone();
    let tx_trace = tx.clone();

    let (timeout_secs, trace_config) = {
        let cfg = daemon.config();
        (cfg.turn.tool_timeout_secs, cfg.turn.trace.clone())
    };

    let join = tokio::time::timeout(
        std::time::Duration::from_secs(timeout_secs),
        tokio::task::spawn_blocking(move || {
            let tracer = ChannelTurnTracer {
                config: trace_config,
                tx: tx_trace,
            };
            let turn_input = crate::turn_executor::TurnInput {
                text: &prompt_text,
                attachments: &[],
                inline_images: &[],
            };
            let result = daemon.execute_turn_streaming(
                &sid,
                &turn_input,
                "sock",
                move |text| {
                    let evt = serde_json::json!({
                        "event": "text",
                        "data": {"content": text}
                    });
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = tx_text.try_send(json);
                    }
                },
                move |progress| {
                    let status_str = match progress.status {
                        cortex_turn::orchestrator::ToolProgressStatus::Started => "started",
                        cortex_turn::orchestrator::ToolProgressStatus::Completed => "completed",
                        cortex_turn::orchestrator::ToolProgressStatus::Error => "error",
                    };
                    let evt = serde_json::json!({
                        "event": "tool",
                        "data": {
                            "tool_name": progress.tool_name,
                            "status": status_str,
                        }
                    });
                    if let Ok(json) = serde_json::to_string(&evt) {
                        let _ = tx_tool.try_send(json);
                    }
                },
                &tracer,
            );
            daemon.turn_semaphore.add_permits(1);
            result
        }),
    );

    // Drop the original sender so the channel closes when spawn_blocking finishes.
    drop(tx);

    // Stream events from channel to the writer concurrently with the join handle.
    tokio::pin!(join);
    let mut join_done = false;
    let mut final_result: Option<Result<String, String>> = None;

    loop {
        if join_done && final_result.is_some() {
            // Drain any remaining events.
            while let Ok(line) = rx.try_recv() {
                let _ = writer.write_all(line.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
            }
            let _ = writer.flush().await;
            break;
        }

        tokio::select! {
            biased;
            Some(line) = rx.recv() => {
                let _ = writer.write_all(line.as_bytes()).await;
                let _ = writer.write_all(b"\n").await;
                let _ = writer.flush().await;
            }
            result = &mut join, if !join_done => {
                join_done = true;
                final_result = Some(match result {
                    Ok(Ok(Ok(response))) => Ok(response),
                    Ok(Ok(Err(e))) => Err(e),
                    Ok(Err(join_err)) => Err(join_err.to_string()),
                    Err(_) => Err("turn execution timed out".into()),
                });
            }
            else => break,
        }
    }

    final_result.unwrap_or_else(|| Err("unexpected end".into()))
}

// ── Config Persistence ────────────────────────────────────────

/// Write the actual bound address back to config.toml `[daemon].addr`.
///
/// Called once after first bind when the OS assigned a random port.
/// Subsequent starts will use the persisted fixed address.
/// Serve HTTP with optional TLS.
async fn serve_http(
    listener: tokio::net::TcpListener,
    router: Router<()>,
    tls_config: &cortex_types::config::TlsConfig,
    home_for_tls: Option<PathBuf>,
) {
    if !tls_config.enabled {
        let _ = axum::serve(listener, router).await;
        return;
    }
    let (Some(cert_rel), Some(key_rel)) = (&tls_config.cert_path, &tls_config.key_path) else {
        tracing::error!("TLS enabled but cert_path/key_path not set");
        return;
    };
    let base = home_for_tls.unwrap_or_default();
    let (cert, key) = (base.join(cert_rel), base.join(key_rel));
    match crate::tls::build_server_config(&cert, &key) {
        Ok(tls_cfg) => {
            let acceptor = tokio_rustls::TlsAcceptor::from(tls_cfg);
            tracing::info!("TLS enabled for HTTP transport");
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    continue;
                };
                let acceptor = acceptor.clone();
                let router = router.clone();
                tokio::spawn(async move {
                    if let Ok(tls_stream) = acceptor.accept(stream).await {
                        let io = hyper_util::rt::TokioIo::new(tls_stream);
                        let service = hyper_util::service::TowerToHyperService::new(router);
                        let _ = hyper_util::server::conn::auto::Builder::new(
                            hyper_util::rt::TokioExecutor::new(),
                        )
                        .serve_connection(io, service)
                        .await;
                    }
                });
            }
        }
        Err(e) => {
            tracing::error!("TLS config failed: {e}, falling back to plain HTTP");
            let _ = axum::serve(listener, router).await;
        }
    }
}

fn bind_http(addr: std::net::SocketAddr) -> tokio::net::TcpListener {
    // SO_REUSEADDR: allow immediate rebind after daemon restart
    let socket = socket2::Socket::new(
        socket2::Domain::IPV4,
        socket2::Type::STREAM,
        Some(socket2::Protocol::TCP),
    )
    .unwrap_or_else(|e| {
        tracing::error!("Failed to create socket: {e}");
        std::process::exit(1);
    });
    socket.set_reuse_address(true).ok();
    socket.set_nonblocking(true).ok();
    socket.bind(&addr.into()).unwrap_or_else(|e| {
        tracing::error!("Failed to bind {addr}: {e}");
        std::process::exit(1);
    });
    socket.listen(128).unwrap_or_else(|e| {
        tracing::error!("Failed to listen: {e}");
        std::process::exit(1);
    });
    tokio::net::TcpListener::from_std(socket.into()).unwrap_or_else(|e| {
        tracing::error!("Failed to convert listener: {e}");
        std::process::exit(1);
    })
}

/// Persist port to config.toml using line-level replacement to preserve
/// comments and field ordering.
fn persist_port_to_config(config_path: &Path, actual_addr: &str) {
    let Ok(content) = std::fs::read_to_string(config_path) else {
        return;
    };
    let addr_line = format!("addr = \"{actual_addr}\"");

    // Try to replace existing addr line under [daemon]
    let mut in_daemon = false;
    let mut replaced = false;
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        if line.trim().starts_with("[daemon]") {
            in_daemon = true;
        } else if line.trim().starts_with('[') && !line.trim().starts_with("[daemon") {
            in_daemon = false;
        }
        if in_daemon && line.trim().starts_with("addr") {
            lines.push(addr_line.clone());
            replaced = true;
        } else {
            lines.push(line.to_string());
        }
    }

    if !replaced {
        // Append [daemon] section if missing
        lines.push(String::new());
        lines.push("[daemon]".to_string());
        lines.push(addr_line);
    }

    let _ = std::fs::write(config_path, lines.join("\n"));
    tracing::info!(addr = actual_addr, "Port persisted to config.toml");
}

// ── Shutdown Signal ───────────────────────────────────────────

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())
            .expect("failed to install SIGHUP handler");
        loop {
            tokio::select! {
                _ = &mut ctrl_c => { tracing::info!("Received SIGINT"); break; }
                _ = sigterm.recv() => { tracing::info!("Received SIGTERM"); break; }
                _ = sighup.recv() => {
                    tracing::info!("Received SIGHUP — ignored (config reload via file watcher)");
                }
            }
        }
    }

    #[cfg(not(unix))]
    {
        let ctrl_c = tokio::signal::ctrl_c();
        ctrl_c.await.ok();
        tracing::info!("Received Ctrl+C");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_config_default_values() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = DaemonConfig::for_home(tmp.path());
        assert_eq!(cfg.http_addr, "127.0.0.1:0");
        assert_eq!(cfg.socket_path, tmp.path().join("data/cortex.sock"));
        assert!(!cfg.enable_stdio);
    }

    #[test]
    fn daemon_status_json_structure() {
        let status = serde_json::json!({
            "uptime_secs": 42,
            "session_count": 0,
            "transports": ["http", "socket"],
            "version": "2.1.0",
        });
        assert_eq!(status["uptime_secs"], 42);
        assert!(status["transports"].as_array().unwrap().len() == 2);
    }

    #[test]
    fn validate_turn_input_rejects_empty() {
        assert!(validate_turn_input("sess-1", "").is_err());
        assert!(validate_turn_input("sess-1", "   ").is_err());
        assert!(validate_turn_input("sess-1", "\t\n").is_err());
        assert!(validate_turn_input("sess-1", "hello").is_ok());
    }

    #[test]
    fn validate_session_id_rejects_long() {
        let long_id = "a".repeat(257);
        assert!(validate_session_id(&long_id).is_err());
        assert!(validate_session_id(&"a".repeat(256)).is_ok());
    }

    #[test]
    fn validate_session_id_rejects_special_chars() {
        assert!(validate_session_id("../../etc/passwd").is_err());
        assert!(validate_session_id("'; DROP TABLE--").is_err());
        assert!(validate_session_id("session with spaces").is_err());
        assert!(validate_session_id("valid-session_01.test").is_ok());
        assert!(validate_session_id("abc123").is_ok());
        // Empty session_id is rejected (must be explicit).
        assert!(validate_session_id("").is_err());
    }
}
