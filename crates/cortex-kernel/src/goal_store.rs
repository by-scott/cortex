use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cortex_types::GoalStack;

use crate::util::atomic_write;

pub struct GoalStore {
    path: PathBuf,
}

impl GoalStore {
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("goals.json"),
        }
    }

    /// Load the goal stack. Returns default if file missing or corrupt.
    #[must_use]
    pub fn load(&self) -> GoalStack {
        fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    /// # Errors
    /// Returns `io::Error` if the file cannot be written.
    pub fn save(&self, stack: &GoalStack) -> io::Result<()> {
        let json = serde_json::to_string_pretty(stack)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        atomic_write(&self.path, json.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::Goal;

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let store = GoalStore::open(dir.path());
        let stack = GoalStack {
            strategic: Some(Goal::new("build cortex")),
            ..GoalStack::default()
        };
        store.save(&stack).unwrap();
        let loaded = store.load();
        assert!(loaded.strategic.is_some());
    }

    #[test]
    fn load_missing_returns_default() {
        let dir = tempfile::tempdir().unwrap();
        let store = GoalStore::open(dir.path());
        let stack = store.load();
        assert!(stack.strategic.is_none());
    }
}
