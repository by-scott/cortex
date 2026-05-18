use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const CORTEX_HOME_ENV: &str = "CORTEX_HOME";

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
struct ActorBindingsFile {
    #[serde(default)]
    aliases: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    transports: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CortexPaths {
    base_dir: PathBuf,
    instance_id: String,
}

impl CortexPaths {
    #[must_use]
    pub fn new(base_dir: impl Into<PathBuf>, instance_id: impl Into<String>) -> Self {
        Self {
            base_dir: base_dir.into(),
            instance_id: instance_id.into(),
        }
    }

    #[must_use]
    pub fn from_instance_home(instance_home: &Path) -> Self {
        let instance_id = instance_home
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or("default");
        let base_dir = instance_home.parent().unwrap_or(instance_home);
        Self::new(base_dir, instance_id)
    }

    #[must_use]
    pub const fn base_dir(&self) -> &PathBuf {
        &self.base_dir
    }

    #[must_use]
    pub fn instance_id(&self) -> &str {
        &self.instance_id
    }

    #[must_use]
    pub fn instance_home(&self) -> PathBuf {
        self.base_dir.join(&self.instance_id)
    }

    #[must_use]
    pub fn data_dir(&self) -> PathBuf {
        self.instance_home().join("data")
    }

    #[must_use]
    pub fn prompts_dir(&self) -> PathBuf {
        self.instance_home().join("prompts")
    }

    #[must_use]
    pub fn memory_dir(&self) -> PathBuf {
        self.instance_home().join("memory")
    }

    #[must_use]
    pub fn sessions_dir(&self) -> PathBuf {
        self.instance_home().join("sessions")
    }

    #[must_use]
    pub fn skills_dir(&self) -> PathBuf {
        self.instance_home().join("skills")
    }

    #[must_use]
    pub fn config_path(&self) -> PathBuf {
        self.instance_home().join("config.toml")
    }

    #[must_use]
    pub fn config_defaults_path(&self) -> PathBuf {
        self.instance_home().join("config.defaults.toml")
    }

    #[must_use]
    pub fn mcp_path(&self) -> PathBuf {
        self.instance_home().join("mcp.toml")
    }

    #[must_use]
    pub fn actors_path(&self) -> PathBuf {
        self.instance_home().join("actors.toml")
    }

    #[must_use]
    pub fn providers_path(&self) -> PathBuf {
        self.base_dir.join("providers.toml")
    }

    #[must_use]
    pub fn plugins_dir(&self) -> PathBuf {
        self.base_dir.join("plugins")
    }

    #[must_use]
    pub fn channels_dir(&self) -> PathBuf {
        self.instance_home().join("channels")
    }

    #[must_use]
    pub fn channel_dir(&self, platform: &str) -> PathBuf {
        self.channels_dir().join(platform)
    }

    #[must_use]
    pub fn channel_auth_path(&self, platform: &str) -> PathBuf {
        self.channel_files(platform).auth
    }

    #[must_use]
    pub fn channel_policy_path(&self, platform: &str) -> PathBuf {
        self.channel_files(platform).policy
    }

    #[must_use]
    pub fn channel_files(&self, platform: &str) -> ChannelFileSet {
        ChannelFileSet::from_paths(self, platform)
    }

    #[must_use]
    pub fn client_sessions_path(&self) -> PathBuf {
        self.data_dir().join("client_sessions.json")
    }

    #[must_use]
    pub fn actor_sessions_path(&self) -> PathBuf {
        self.data_dir().join("actor_sessions.json")
    }

    #[must_use]
    pub fn cortex_db_path(&self) -> PathBuf {
        self.data_dir().join("cortex.db")
    }

    #[must_use]
    pub fn embedding_store_path(&self) -> PathBuf {
        self.data_dir().join("embedding_store.db")
    }

    #[must_use]
    pub fn memory_graph_path(&self) -> PathBuf {
        self.data_dir().join("memory_graph.db")
    }

    #[must_use]
    pub fn model_info_dir(&self) -> PathBuf {
        self.data_dir()
    }

    #[must_use]
    pub fn socket_path(&self) -> PathBuf {
        self.data_dir().join("cortex.sock")
    }

    #[must_use]
    pub fn blobs_dir(&self) -> PathBuf {
        self.data_dir().join("blobs")
    }

    #[must_use]
    pub fn config_files(&self) -> ConfigFileSet {
        ConfigFileSet::from_paths(self)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelFileSet {
    pub dir: PathBuf,
    pub auth: PathBuf,
    pub policy: PathBuf,
    pub paired_users: PathBuf,
    pub pending_pairs: PathBuf,
    pub update_offset: PathBuf,
}

impl ChannelFileSet {
    #[must_use]
    pub fn from_instance_home(instance_home: &Path, platform: &str) -> Self {
        Self::from_paths(&CortexPaths::from_instance_home(instance_home), platform)
    }

    #[must_use]
    pub fn from_paths(paths: &CortexPaths, platform: &str) -> Self {
        Self::from_dir(paths.channel_dir(platform))
    }

    #[must_use]
    pub fn from_dir(dir: PathBuf) -> Self {
        Self {
            auth: dir.join("auth.json"),
            policy: dir.join("policy.json"),
            paired_users: dir.join("paired_users.json"),
            pending_pairs: dir.join("pending_pairs.json"),
            update_offset: dir.join("update_offset.json"),
            dir,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFileSet {
    pub config: PathBuf,
    pub providers: PathBuf,
    pub mcp: PathBuf,
    pub actors: PathBuf,
}

impl ConfigFileSet {
    #[must_use]
    pub fn from_paths(paths: &CortexPaths) -> Self {
        Self {
            config: paths.config_path(),
            providers: paths.providers_path(),
            mcp: paths.mcp_path(),
            actors: paths.actors_path(),
        }
    }
}

pub struct ActorBindingsStore {
    config_path: PathBuf,
}

impl ActorBindingsStore {
    #[must_use]
    pub fn from_paths(paths: &CortexPaths) -> Self {
        Self {
            config_path: paths.config_files().actors,
        }
    }

    #[must_use]
    pub fn actor_aliases(&self) -> std::collections::BTreeMap<String, String> {
        load_actor_bindings_file(&self.config_path).aliases
    }

    pub fn set_actor_alias(&self, from: &str, to: &str) {
        let mut bindings = load_actor_bindings_file(&self.config_path);
        bindings.aliases.insert(from.to_string(), to.to_string());
        save_actor_bindings_file(&self.config_path, &bindings);
    }

    #[must_use]
    pub fn remove_actor_alias(&self, from: &str) -> bool {
        let mut bindings = load_actor_bindings_file(&self.config_path);
        let removed = bindings.aliases.remove(from).is_some();
        if removed {
            save_actor_bindings_file(&self.config_path, &bindings);
        }
        removed
    }

    #[must_use]
    pub fn transport_actors(&self) -> std::collections::BTreeMap<String, String> {
        load_actor_bindings_file(&self.config_path).transports
    }

    pub fn set_transport_actor(&self, transport: &str, actor: &str) {
        let mut bindings = load_actor_bindings_file(&self.config_path);
        bindings
            .transports
            .insert(transport.to_string(), actor.to_string());
        save_actor_bindings_file(&self.config_path, &bindings);
    }

    #[must_use]
    pub fn remove_transport_actor(&self, transport: &str) -> bool {
        let mut bindings = load_actor_bindings_file(&self.config_path);
        let removed = bindings.transports.remove(transport).is_some();
        if removed {
            save_actor_bindings_file(&self.config_path, &bindings);
        }
        removed
    }
}

pub struct RuntimeStateStore {
    client_sessions: PathBuf,
    actor_sessions: PathBuf,
}

impl RuntimeStateStore {
    #[must_use]
    pub fn from_paths(paths: &CortexPaths) -> Self {
        Self {
            client_sessions: paths.client_sessions_path(),
            actor_sessions: paths.actor_sessions_path(),
        }
    }

    #[must_use]
    pub fn client_sessions(&self) -> HashMap<String, String> {
        load_hash_map(&self.client_sessions)
    }

    pub fn save_client_sessions(&self, sessions: &HashMap<String, String>) {
        save_hash_map(&self.client_sessions, sessions);
    }

    #[must_use]
    pub fn actor_sessions(&self) -> HashMap<String, String> {
        load_hash_map(&self.actor_sessions)
    }

    pub fn save_actor_sessions(&self, sessions: &HashMap<String, String>) {
        save_hash_map(&self.actor_sessions, sessions);
    }
}

fn load_actor_bindings_file(path: &Path) -> ActorBindingsFile {
    fs::read_to_string(path)
        .ok()
        .and_then(|content| toml::from_str::<ActorBindingsFile>(&content).ok())
        .unwrap_or_default()
}

fn save_actor_bindings_file(path: &Path, bindings: &ActorBindingsFile) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(content) = toml::to_string_pretty(bindings) {
        let _ = crate::atomic_write_text(path, content);
    }
}

fn load_hash_map(path: &Path) -> HashMap<String, String> {
    fs::read_to_string(path)
        .ok()
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default()
}

fn save_hash_map(path: &Path, map: &HashMap<String, String>) {
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(json) = serde_json::to_string_pretty(map) {
        let _ = crate::atomic_write_text(path, json);
    }
}

/// Resolve the Cortex home directory.
/// Priority: CLI arg > `CORTEX_HOME` env > `$HOME/.cortex`
#[must_use]
pub fn resolve_home(cli_arg: Option<&str>) -> PathBuf {
    if let Some(arg) = cli_arg {
        return PathBuf::from(arg);
    }
    if let Ok(env) = std::env::var(CORTEX_HOME_ENV) {
        return PathBuf::from(env);
    }
    dirs_fallback()
}

fn dirs_fallback() -> PathBuf {
    std::env::var("HOME").map_or_else(
        |_| PathBuf::from(".cortex"),
        |h| PathBuf::from(h).join(".cortex"),
    )
}

/// Create the standard directory structure under an instance home.
///
/// # Errors
/// Returns `io::Error` if directories cannot be created.
pub fn ensure_home_dirs(home: &Path) -> io::Result<()> {
    for sub in [
        "prompts",
        "prompts/system",
        "prompts/.backup",
        "skills",
        "data",
        "memory",
        "sessions",
    ] {
        fs::create_dir_all(home.join(sub))?;
    }
    Ok(())
}

/// Ensure the base directory exists.
///
/// # Errors
/// Returns `io::Error` if the directory cannot be created.
pub fn ensure_base_dirs(base: &Path) -> io::Result<()> {
    fs::create_dir_all(base)
}
