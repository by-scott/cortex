use cortex_kernel::CortexPaths;
use std::path::{Path, PathBuf};

/// Metadata about a discovered Cortex instance.
#[derive(Debug, Clone)]
pub struct InstanceInfo {
    /// Instance identifier (e.g. "default", "work").
    pub id: String,
    /// Absolute path to the instance's home directory.
    pub home_path: PathBuf,
    /// Whether a `config.toml` exists in the instance home.
    pub config_exists: bool,
}

/// Discovers and queries installed Cortex instances.
///
/// Instances are subdirectories of `{base}/`:
/// ```text
/// ~/.cortex/
///   providers.toml      ← global shared
///   default/             ← default instance
///     config.toml
///     prompts/ skills/ data/ memory/ sessions/
///   work/                ← named instance
///     config.toml
///     ...
/// ```
///
/// A directory is recognized as an instance if it contains
/// `config.toml` or any of the standard subdirectories.
pub struct InstanceManager {
    base: PathBuf,
}

/// Error from instance manager operations.
#[derive(Debug)]
pub struct InstanceNotFound(pub String);

impl std::fmt::Display for InstanceNotFound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "instance not found: {}", self.0)
    }
}

impl std::error::Error for InstanceNotFound {}

impl InstanceManager {
    /// Create an instance manager rooted at the given base directory.
    #[must_use]
    pub fn new(base: &Path) -> Self {
        Self {
            base: base.to_path_buf(),
        }
    }

    /// List all discovered instances.
    #[must_use]
    pub fn list(&self) -> Vec<InstanceInfo> {
        let Ok(entries) = std::fs::read_dir(&self.base) else {
            return vec![];
        };
        let mut result = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir()
                && is_instance_dir(&path)
                && let Some(name) = path.file_name().and_then(|n| n.to_str())
            {
                result.push(InstanceInfo {
                    id: name.to_string(),
                    home_path: path,
                    config_exists: instance_has_config(&entry.path()),
                });
            }
        }
        result.sort_by(|a, b| a.id.cmp(&b.id));
        result
    }

    /// Get information about a specific instance by ID.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceNotFound`] if the instance does not exist.
    pub fn get_info(&self, id: &str) -> Result<InstanceInfo, InstanceNotFound> {
        let path = self.base.join(id);
        if path.is_dir() {
            Ok(InstanceInfo {
                id: id.to_string(),
                config_exists: instance_has_config(&path),
                home_path: path,
            })
        } else {
            Err(InstanceNotFound(id.to_string()))
        }
    }

    /// Ensure an instance directory exists.
    ///
    /// # Errors
    ///
    /// Returns [`InstanceNotFound`] if the directory cannot be created.
    pub fn ensure_instance(&self, id: &str) -> Result<InstanceInfo, InstanceNotFound> {
        let path = self.base.join(id);
        if !path.exists() {
            std::fs::create_dir_all(&path)
                .map_err(|e| InstanceNotFound(format!("create '{id}': {e}")))?;
        }
        Ok(InstanceInfo {
            id: id.to_string(),
            config_exists: instance_has_config(&path),
            home_path: path,
        })
    }
}

fn is_instance_dir(path: &Path) -> bool {
    let paths = CortexPaths::from_instance_home(path);
    let markers = [
        paths.config_path(),
        paths.data_dir(),
        paths.prompts_dir(),
        paths.memory_dir(),
    ];
    markers.into_iter().any(|marker| marker.exists())
}

fn instance_has_config(path: &Path) -> bool {
    CortexPaths::from_instance_home(path)
        .config_files()
        .config
        .exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn list_empty_base() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = InstanceManager::new(tmp.path());
        assert!(mgr.list().is_empty());
    }

    #[test]
    fn list_discovers_instances() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("default").join("data")).unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        fs::write(tmp.path().join("work").join("config.toml"), "").unwrap();

        let mgr = InstanceManager::new(tmp.path());
        let instances = mgr.list();
        assert_eq!(instances.len(), 2);
        assert_eq!(instances[0].id, "default");
        assert_eq!(instances[1].id, "work");
    }

    #[test]
    fn list_ignores_non_instance_dirs() {
        let tmp = tempfile::tempdir().unwrap();
        // providers.toml is a file, not a dir
        fs::write(tmp.path().join("providers.toml"), "").unwrap();
        // random dir without markers
        fs::create_dir_all(tmp.path().join("random")).unwrap();
        // real instance
        fs::create_dir_all(tmp.path().join("default").join("prompts")).unwrap();

        let mgr = InstanceManager::new(tmp.path());
        let instances = mgr.list();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].id, "default");
    }

    #[test]
    fn get_info_existing() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("work")).unwrap();
        let mgr = InstanceManager::new(tmp.path());
        let info = mgr.get_info("work").unwrap();
        assert_eq!(info.id, "work");
    }

    #[test]
    fn get_info_nonexistent() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = InstanceManager::new(tmp.path());
        assert!(mgr.get_info("nope").is_err());
    }

    #[test]
    fn ensure_creates_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let mgr = InstanceManager::new(tmp.path());
        let info = mgr.ensure_instance("new-one").unwrap();
        assert!(info.home_path.is_dir());
        assert_eq!(info.id, "new-one");
    }
}
