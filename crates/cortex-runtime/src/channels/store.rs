//! Persistent storage for channel authentication and pairing data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedUser {
    pub user_id: String,
    pub name: String,
    pub paired_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPair {
    pub user_id: String,
    pub user_name: String,
    pub code: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPolicy {
    /// `"pairing"` (default) | `"whitelist"` | `"open"`.
    pub mode: String,
    /// Users always allowed (checked before pairing). Effective in all modes.
    #[serde(default)]
    pub whitelist: Vec<String>,
    /// Users always denied. Takes priority over whitelist and pairing.
    #[serde(default)]
    pub blacklist: Vec<String>,
    #[serde(default = "default_pair_ttl")]
    pub pair_code_ttl_secs: u64,
    #[serde(default = "default_max_pending")]
    pub max_pending: usize,
}

const fn default_pair_ttl() -> u64 {
    300
}
const fn default_max_pending() -> usize {
    10
}

impl Default for ChannelPolicy {
    fn default() -> Self {
        Self {
            mode: "pairing".into(),
            whitelist: Vec::new(),
            blacklist: Vec::new(),
            pair_code_ttl_secs: 300,
            max_pending: 10,
        }
    }
}

pub struct ChannelStore {
    dir: PathBuf,
}

impl ChannelStore {
    #[must_use]
    pub fn open(instance_home: &Path, platform: &str) -> Self {
        let dir = instance_home.join("channels").join(platform);
        let _ = std::fs::create_dir_all(&dir);
        Self { dir }
    }

    /// Open from an already-resolved directory path.
    #[must_use]
    pub const fn open_dir(dir: PathBuf) -> Self {
        Self { dir }
    }

    /// Return the underlying directory path (for passing across thread boundaries).
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    #[must_use]
    pub fn policy(&self) -> ChannelPolicy {
        self.load_json("policy.json").unwrap_or_default()
    }

    pub fn save_policy(&self, policy: &ChannelPolicy) {
        self.save_json("policy.json", policy);
    }

    #[must_use]
    pub fn paired_users(&self) -> Vec<PairedUser> {
        self.load_json("paired_users.json").unwrap_or_default()
    }

    pub fn save_paired_users(&self, users: &[PairedUser]) {
        self.save_json("paired_users.json", users);
    }

    #[must_use]
    pub fn pending_pairs(&self) -> Vec<PendingPair> {
        self.load_json("pending_pairs.json").unwrap_or_default()
    }

    pub fn save_pending_pairs(&self, pairs: &[PendingPair]) {
        self.save_json("pending_pairs.json", pairs);
    }

    #[must_use]
    pub fn is_paired(&self, user_id: &str) -> bool {
        self.paired_users().iter().any(|u| u.user_id == user_id)
    }

    #[must_use]
    pub fn is_blacklisted(&self, user_id: &str) -> bool {
        self.policy().blacklist.iter().any(|b| b == user_id)
    }

    /// Return the active session ID for a user, if any.
    #[must_use]
    pub fn active_session(&self, user_id: &str) -> Option<String> {
        let sessions: std::collections::HashMap<String, String> =
            self.load_json("user_sessions.json").unwrap_or_default();
        sessions.get(user_id).filter(|s| !s.is_empty()).cloned()
    }

    /// Set (or clear) the active session ID for a user.
    pub fn set_active_session(&self, user_id: &str, session_id: &str) {
        let mut sessions: std::collections::HashMap<String, String> =
            self.load_json("user_sessions.json").unwrap_or_default();
        sessions.insert(user_id.to_string(), session_id.to_string());
        self.save_json("user_sessions.json", &sessions);
    }

    /// Read a JSON file from the store directory.
    fn load_json<T: serde::de::DeserializeOwned>(&self, filename: &str) -> Option<T> {
        let path = self.dir.join(filename);
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Write a JSON file to the store directory.
    fn save_json<T: Serialize + ?Sized>(&self, filename: &str, data: &T) {
        let path = self.dir.join(filename);
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, json);
        }
    }
}
