use std::path::{Path, PathBuf};

use cortex_kernel::{
    EmbeddingClient, EmbeddingStore, GoalStore, Journal, MemoryStore, ModelInfoStore,
    PromptManager, SessionStore, ensure_base_dirs, ensure_home_dirs, load_config, load_providers,
};
use cortex_turn::llm::{LlmClient, create_llm_client};
use cortex_turn::tools::{ToolRegistry, register_core_tools_basic};
use cortex_types::config::{CortexConfig, ProviderRegistry, ResolvedEndpoint};

/// A fully initialized Cortex cognitive runtime instance.
///
/// Holds ownership of every subsystem required to execute Turns.
/// Constructed via the async [`CortexRuntime::new`] method.
///
/// `Debug` is implemented manually because `dyn LlmClient` is not `Debug`.
pub struct CortexRuntime {
    home: PathBuf,
    data_dir: PathBuf,
    config: CortexConfig,
    providers: ProviderRegistry,
    journal: Journal,
    memory_store: MemoryStore,
    goal_store: GoalStore,
    session_store: SessionStore,
    llm: Box<dyn LlmClient>,
    tools: ToolRegistry,
    prompt_manager: PromptManager,
    embedding_client: Option<EmbeddingClient>,
    embedding_store: Option<EmbeddingStore>,
    max_output_tokens: usize,
    /// Plugin-contributed skill directories, collected during plugin loading.
    pub plugin_skill_dirs: Vec<PathBuf>,
    /// Keep plugin shared libraries alive for the runtime's lifetime.
    /// Dropping these would invalidate plugin tool vtables.
    pub plugin_libraries: Vec<libloading::Library>,
}

impl std::fmt::Debug for CortexRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CortexRuntime")
            .field("home", &self.home)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

/// Error type returned when runtime initialization fails.
#[derive(Debug)]
pub struct RuntimeError(pub String);

impl std::fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RuntimeError {}

impl CortexRuntime {
    /// Initialize a complete Cortex runtime from the given home directory argument.
    ///
    /// If `home_arg` is `None`, the default home directory resolution chain applies
    /// (`--home` > `CORTEX_HOME` > `~/.cortex`).
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeError`] if any subsystem fails to initialize.
    /// Initialize from resolved paths.
    ///
    /// - `base`: root Cortex directory (e.g., `~/.cortex/`) — holds `providers.toml`
    /// - `instance_home`: instance directory (e.g., `~/.cortex/default/`) — holds
    ///   `config.toml`, `prompts/`, `skills/`, `data/`, `memory/`, `sessions/`
    pub async fn new(base: &Path, instance_home: &Path) -> Result<Self, RuntimeError> {
        let home = instance_home.to_path_buf();
        ensure_base_dirs(base).map_err(|e| RuntimeError(format!("base dirs: {e}")))?;
        ensure_home_dirs(&home).map_err(|e| RuntimeError(format!("home dirs: {e}")))?;

        let prompt_manager =
            PromptManager::new(&home).map_err(|e| RuntimeError(format!("prompt manager: {e}")))?;
        let (providers, resolved_provider) =
            load_providers(base).map_err(|e| RuntimeError(format!("load providers: {e}")))?;
        let config = load_config(&home, resolved_provider.as_deref(), &providers);

        let db_path = home.join("data").join("cortex.db");
        let journal = Journal::open(&db_path).map_err(|e| RuntimeError(format!("journal: {e}")))?;
        let memory_store = MemoryStore::open(&home.join("memory"))
            .map_err(|e| RuntimeError(format!("memory store: {e}")))?;
        let goal_store = GoalStore::open(&home.join("data"));
        let session_store = SessionStore::open(&home.join("sessions"))
            .map_err(|e| RuntimeError(format!("session store: {e}")))?;

        let primary_endpoint = ResolvedEndpoint::resolve_primary(&config.api, &providers)
            .map_err(|e| RuntimeError(format!("resolve LLM endpoint: {e}")))?;
        let llm = create_llm_client(&primary_endpoint);

        let max_output_tokens = fetch_model_max_output(&home, &primary_endpoint, &config).await;

        let tools = init_tools();

        let (embedding_client, embedding_store) = init_embedding(&config, &providers, &home);

        let data_dir = home.join("data");

        Ok(Self {
            home,
            data_dir,
            config,
            providers,
            journal,
            memory_store,
            goal_store,
            session_store,
            llm,
            tools,
            prompt_manager,
            embedding_client,
            embedding_store,
            max_output_tokens,
            plugin_skill_dirs: Vec::new(),
            plugin_libraries: Vec::new(),
        })
    }

    // ── Accessors ──────────────────────────────────────────────

    #[must_use]
    pub fn home(&self) -> &Path {
        &self.home
    }

    #[must_use]
    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    #[must_use]
    pub const fn config(&self) -> &CortexConfig {
        &self.config
    }

    #[must_use]
    pub const fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    #[must_use]
    pub const fn journal(&self) -> &Journal {
        &self.journal
    }

    #[must_use]
    pub const fn memory_store(&self) -> &MemoryStore {
        &self.memory_store
    }

    #[must_use]
    pub const fn goal_store(&self) -> &GoalStore {
        &self.goal_store
    }

    #[must_use]
    pub const fn session_store(&self) -> &SessionStore {
        &self.session_store
    }

    #[must_use]
    pub fn llm(&self) -> &dyn LlmClient {
        self.llm.as_ref()
    }

    #[must_use]
    pub const fn tools(&self) -> &ToolRegistry {
        &self.tools
    }

    /// Mutable access to the tool registry, e.g. for registering plugins.
    pub const fn tools_mut(&mut self) -> &mut ToolRegistry {
        &mut self.tools
    }

    /// Move all tools from the runtime's registry into the target registry.
    /// Used to transfer plugin-registered tools to the daemon's tool set.
    pub fn drain_plugin_tools(&mut self, target: &mut ToolRegistry) {
        self.tools.drain_into(target);
    }

    #[must_use]
    pub const fn prompt_manager(&self) -> &PromptManager {
        &self.prompt_manager
    }

    #[must_use]
    pub const fn embedding_client(&self) -> Option<&EmbeddingClient> {
        self.embedding_client.as_ref()
    }

    #[must_use]
    pub const fn embedding_store(&self) -> Option<&EmbeddingStore> {
        self.embedding_store.as_ref()
    }

    #[must_use]
    pub const fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }
}

// ── Private helpers ────────────────────────────────────────────

fn init_tools() -> ToolRegistry {
    let mut tools = ToolRegistry::new();
    register_core_tools_basic(&mut tools);
    tools
}

fn init_embedding(
    config: &CortexConfig,
    providers: &ProviderRegistry,
    home: &Path,
) -> (Option<EmbeddingClient>, Option<EmbeddingStore>) {
    let embedding_client = {
        let name = &config.embedding.provider;
        let key = &config.embedding.api_key;
        let model = &config.embedding.model;
        providers
            .get(name)
            .map(|provider_cfg| EmbeddingClient::new(provider_cfg, key, model))
    };
    let embedding_store = EmbeddingStore::open(&home.join("data").join("embedding_store.db")).ok();
    (embedding_client, embedding_store)
}

async fn fetch_model_max_output(
    home: &Path,
    endpoint: &ResolvedEndpoint,
    config: &CortexConfig,
) -> usize {
    let mut store = ModelInfoStore::open(&home.join("data"));
    let max_output = if config.api.max_tokens > 0 {
        config.api.max_tokens
    } else {
        cortex_types::config::DEFAULT_MAX_TOKENS_FALLBACK
    };
    let info = store
        .get_or_fetch(endpoint, config.context.max_tokens, max_output)
        .await;
    info.max_output_tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn runtime_new_creates_home_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path();
        let instance = base.join("default");

        let _result = CortexRuntime::new(base, &instance).await;

        // Instance dirs created by ensure_home_dirs
        assert!(instance.join("data").exists());
        assert!(instance.join("memory").exists());
        assert!(instance.join("prompts").exists());
        assert!(instance.join("skills").exists());
        assert!(instance.join("sessions").exists());
    }
}
