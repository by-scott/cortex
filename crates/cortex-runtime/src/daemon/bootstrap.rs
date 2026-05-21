use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::AtomicUsize;
use std::sync::{Arc, Mutex, RwLock};

use cortex_kernel::{Journal, SessionStore};

use crate::runtime::CortexRuntime;

use super::DaemonState;

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

pub struct RuntimeBindings {
    pub client_sessions: HashMap<String, String>,
    pub actor_sessions: HashMap<String, String>,
    pub actor_aliases: HashMap<String, String>,
    pub transport_actors: HashMap<String, String>,
}

struct RuntimeArtifacts {
    journal: Journal,
    session_store: SessionStore,
    task_store: cortex_kernel::TaskStore,
    goal_store: cortex_kernel::GoalStore,
    memory_store: cortex_kernel::MemoryStore,
    prompt_manager: cortex_kernel::PromptManager,
}

struct RuntimeQueues {
    cron_queue: Arc<cortex_turn::tools::cron::CronQueue>,
    post_turn_tx: tokio::sync::mpsc::UnboundedSender<super::post_turn_queue::PostTurnJob>,
    post_turn_rx: tokio::sync::mpsc::UnboundedReceiver<super::post_turn_queue::PostTurnJob>,
}

impl DaemonState {
    pub(super) fn paths(&self) -> cortex_kernel::CortexPaths {
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

        let queues = Self::init_runtime_queues(&data_dir);
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
            &queues.cron_queue,
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

        let rate_limiter = Self::init_rate_limiter(&config);

        // Register self-introspection tools (audit, prompt_inspect).
        crate::introspect_tools::register_introspect_tools(&mut tools, &home);

        Ok(Self {
            journal,
            session_store,
            task_store: Arc::new(task_store),
            goal_store: Arc::new(goal_store),
            sessions: Mutex::new(HashMap::new()),
            turn_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
            foreground_waiters: AtomicUsize::new(0),
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
            heartbeat_state: Arc::new(crate::heartbeat::HeartbeatState::new()),
            cron_queue: queues.cron_queue,
            post_turn_tx: queues.post_turn_tx,
            post_turn_rx: Mutex::new(Some(queues.post_turn_rx)),
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

    fn storage_paths(data_dir: &Path) -> cortex_kernel::CortexPaths {
        let instance_home = data_dir.parent().unwrap_or(data_dir);
        cortex_kernel::CortexPaths::from_instance_home(instance_home)
    }

    fn init_runtime_queues(data_dir: &Path) -> RuntimeQueues {
        let cron_queue = Arc::new(cortex_turn::tools::cron::CronQueue::open(data_dir));
        let (post_turn_tx, post_turn_rx) = super::post_turn_queue::channel();
        RuntimeQueues {
            cron_queue,
            post_turn_tx,
            post_turn_rx,
        }
    }

    fn init_rate_limiter(
        config: &cortex_types::config::CortexConfig,
    ) -> crate::rate_limiter::RateLimiter {
        crate::rate_limiter::RateLimiter::new(
            config.rate_limit.per_session_rpm,
            config.rate_limit.global_rpm,
        )
    }

    pub(super) fn runtime_state_store(data_dir: &Path) -> cortex_kernel::RuntimeStateStore {
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

    pub(super) fn load_runtime_bindings(data_dir: &Path) -> RuntimeBindings {
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
        let persisted_health = journal.load_skill_health().unwrap_or_default();
        let persisted_proposals = journal.load_skill_proposals().unwrap_or_default();
        let skill_registry = cortex_turn::skills::SkillRegistry::new();
        skill_registry.load_utilities(persisted_utilities);
        skill_registry.load_health(persisted_health);
        skill_registry.load_proposals(persisted_proposals);
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
    /// Core tools (`bash`, `memory_search`, `memory_save`, `agent`) are
    /// registered later by `init_memory_subsystem` once the memory store is
    /// available. Plugin tools are loaded separately via the plugin system.
    fn init_tools(
        config: &cortex_types::config::CortexConfig,
        skill_registry: &Arc<cortex_turn::skills::SkillRegistry>,
    ) -> cortex_turn::tools::ToolRegistry {
        let mut tools = cortex_turn::tools::ToolRegistry::new();
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
            config.acp.clone(),
            Some(paths.config_files().config),
            Arc::clone(cron_queue),
        );

        MemorySubsystem {
            store: memory_store,
            embedding_client,
            embedding_store,
            embedding_health,
        }
    }
}
