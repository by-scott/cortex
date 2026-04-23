use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use cortex_types::{Message, SessionId, SessionMetadata};

use crate::util::atomic_write;

pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    /// # Errors
    /// Returns `io::Error` if the directory cannot be created.
    pub fn open(sessions_dir: &Path) -> io::Result<Self> {
        fs::create_dir_all(sessions_dir)?;
        Ok(Self {
            dir: sessions_dir.to_path_buf(),
        })
    }

    /// # Errors
    /// Returns `io::Error` if the metadata cannot be written.
    pub fn save(&self, metadata: &SessionMetadata) -> io::Result<()> {
        let json = serde_json::to_string_pretty(metadata)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        atomic_write(&self.session_path(&metadata.id), json.as_bytes())
    }

    #[must_use]
    pub fn load(&self, session_id: &SessionId) -> Option<SessionMetadata> {
        let raw = fs::read_to_string(self.session_path(session_id)).ok()?;
        serde_json::from_str(&raw).ok()
    }

    #[must_use]
    pub fn load_for_actor(&self, session_id: &SessionId, actor: &str) -> Option<SessionMetadata> {
        let session = self.load(session_id)?;
        session_visible_to_actor(&session, actor).then_some(session)
    }

    #[must_use]
    pub fn list(&self) -> Vec<SessionMetadata> {
        let Ok(entries) = fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        let mut sessions: Vec<SessionMetadata> = entries
            .filter_map(Result::ok)
            .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
            .filter_map(|e| {
                let raw = fs::read_to_string(e.path()).ok()?;
                serde_json::from_str(&raw).ok()
            })
            .collect();
        sessions.sort_by_key(|s| std::cmp::Reverse(s.created_at));
        sessions
    }

    #[must_use]
    pub fn list_for_actor(&self, actor: &str) -> Vec<SessionMetadata> {
        self.list()
            .into_iter()
            .filter(|session| session_visible_to_actor(session, actor))
            .collect()
    }

    #[must_use]
    pub fn find_latest(&self) -> Option<SessionMetadata> {
        self.list().into_iter().next()
    }

    /// # Errors
    /// Returns `io::Error` if the history cannot be written.
    pub fn save_history(&self, session_id: &SessionId, history: &[Message]) -> io::Result<()> {
        let data = rmp_serde::to_vec(history)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        atomic_write(&self.history_path(session_id), &data)
    }

    #[must_use]
    pub fn load_history(&self, session_id: &SessionId) -> Vec<Message> {
        let Ok(data) = fs::read(self.history_path(session_id)) else {
            return Vec::new();
        };
        rmp_serde::from_slice(&data).unwrap_or_default()
    }

    #[must_use]
    pub fn load_history_for_actor(&self, session_id: &SessionId, actor: &str) -> Vec<Message> {
        if self.load_for_actor(session_id, actor).is_none() {
            return Vec::new();
        }
        self.load_history(session_id)
    }

    /// Get the per-session data directory, creating it if needed.
    ///
    /// # Errors
    /// Returns `io::Error` if the directory cannot be created.
    pub fn session_data_dir(&self, session_id: &SessionId) -> io::Result<PathBuf> {
        let dir = self.dir.join(session_id.to_string());
        fs::create_dir_all(&dir)?;
        Ok(dir)
    }

    fn session_path(&self, session_id: &SessionId) -> PathBuf {
        self.dir.join(format!("{session_id}.json"))
    }

    fn history_path(&self, session_id: &SessionId) -> PathBuf {
        self.dir.join(format!("{session_id}.history"))
    }
}

fn session_visible_to_actor(session: &SessionMetadata, actor: &str) -> bool {
    actor == "local:default" || session.owner_actor == actor
}
