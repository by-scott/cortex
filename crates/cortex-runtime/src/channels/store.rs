//! Persistent storage for channel authentication and pairing data.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedUser {
    pub user_id: String,
    pub name: String,
    pub paired_at: String,
    #[serde(default)]
    pub subscribe: bool,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdateOffsetState {
    offset: i64,
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
    files: cortex_kernel::ChannelFileSet,
}

impl ChannelStore {
    #[must_use]
    pub fn open(instance_home: &Path, platform: &str) -> Self {
        Self::open_files(cortex_kernel::ChannelFileSet::from_instance_home(
            instance_home,
            platform,
        ))
    }

    /// Open from an already-resolved directory path.
    #[must_use]
    pub fn open_dir(dir: PathBuf) -> Self {
        Self::open_files(cortex_kernel::ChannelFileSet::from_dir(dir))
    }

    /// Open from an already-resolved file set.
    #[must_use]
    pub const fn open_files(files: cortex_kernel::ChannelFileSet) -> Self {
        Self { files }
    }

    /// Return the underlying directory path (for passing across thread boundaries).
    #[must_use]
    pub fn dir(&self) -> &Path {
        &self.files.dir
    }

    #[must_use]
    pub fn policy(&self) -> ChannelPolicy {
        Self::load_json(&self.files.policy).unwrap_or_default()
    }

    pub fn save_policy(&self, policy: &ChannelPolicy) {
        Self::save_json(&self.files.policy, policy);
    }

    #[must_use]
    pub fn paired_users(&self) -> Vec<PairedUser> {
        Self::load_json(&self.files.paired_users).unwrap_or_default()
    }

    pub fn save_paired_users(&self, users: &[PairedUser]) {
        Self::save_json(&self.files.paired_users, users);
    }

    #[must_use]
    pub fn pending_pairs(&self) -> Vec<PendingPair> {
        Self::load_json(&self.files.pending_pairs).unwrap_or_default()
    }

    pub fn save_pending_pairs(&self, pairs: &[PendingPair]) {
        Self::save_json(&self.files.pending_pairs, pairs);
    }

    #[must_use]
    pub fn update_offset(&self) -> i64 {
        Self::load_json::<UpdateOffsetState>(&self.files.update_offset)
            .map_or(0, |state| state.offset)
    }

    pub fn save_update_offset(&self, offset: i64) {
        Self::save_json(&self.files.update_offset, &UpdateOffsetState { offset });
    }

    /// Approve a pending pair request and promote it to a paired user.
    ///
    /// # Errors
    /// Returns an error if the user is already paired or there is no pending
    /// pairing request for `user_id`.
    pub fn approve_pending_pair(&self, user_id: &str) -> Result<PairedUser, ChannelStoreError> {
        let mut paired = self.paired_users();
        if paired.iter().any(|user| user.user_id == user_id) {
            return Err(ChannelStoreError::AlreadyPaired(user_id.to_string()));
        }

        let mut pending = self.pending_pairs();
        let Some(idx) = pending.iter().position(|pair| pair.user_id == user_id) else {
            return Err(ChannelStoreError::PendingUserNotFound(user_id.to_string()));
        };
        let pending_pair = pending.remove(idx);
        self.save_pending_pairs(&pending);

        let paired_user = PairedUser {
            user_id: user_id.to_string(),
            name: pending_pair.user_name,
            paired_at: current_unix_timestamp(),
            subscribe: false,
        };
        paired.push(paired_user.clone());
        self.save_paired_users(&paired);
        Ok(paired_user)
    }

    /// Enable or disable session subscription for a paired user.
    ///
    /// # Errors
    /// Returns an error if the user is not paired.
    pub fn set_pair_subscription(
        &self,
        user_id: &str,
        subscribe: bool,
    ) -> Result<PairedUser, ChannelStoreError> {
        let mut paired = self.paired_users();
        let Some(user) = paired.iter_mut().find(|user| user.user_id == user_id) else {
            return Err(ChannelStoreError::PairedUserNotFound(user_id.to_string()));
        };
        user.subscribe = subscribe;
        let updated = user.clone();
        self.save_paired_users(&paired);
        let persisted = self
            .paired_users()
            .iter()
            .any(|user| user.user_id == user_id && user.subscribe == subscribe);
        if !persisted {
            return Err(ChannelStoreError::PersistFailed(format!(
                "failed to persist subscription for paired user {user_id}"
            )));
        }
        Ok(updated)
    }

    #[must_use]
    pub fn revoke_pair(&self, user_id: &str) -> bool {
        let mut paired = self.paired_users();
        let before = paired.len();
        paired.retain(|user| user.user_id != user_id);
        let removed = paired.len() != before;
        if removed {
            self.save_paired_users(&paired);
        }
        removed
    }

    /// Update the channel access policy mode.
    ///
    /// # Errors
    /// Returns an error if `mode` is not one of `pairing`, `whitelist`, or `open`.
    pub fn update_policy_mode(&self, mode: &str) -> Result<ChannelPolicy, ChannelStoreError> {
        if !matches!(mode, "pairing" | "whitelist" | "open") {
            return Err(ChannelStoreError::InvalidPolicyMode(mode.to_string()));
        }
        let mut policy = self.policy();
        policy.mode = mode.to_string();
        self.save_policy(&policy);
        Ok(policy)
    }

    /// Add or remove a user from one of the policy lists.
    ///
    /// # Errors
    /// Returns an error if the entry already exists when adding, or is missing
    /// when removing.
    pub fn mutate_policy_list(
        &self,
        list: PolicyList,
        user_id: &str,
        add: bool,
    ) -> Result<ChannelPolicy, ChannelStoreError> {
        let mut policy = self.policy();
        let users = match list {
            PolicyList::Whitelist => &mut policy.whitelist,
            PolicyList::Blacklist => &mut policy.blacklist,
        };

        if add {
            if users.iter().any(|user| user == user_id) {
                return Err(ChannelStoreError::PolicyEntryExists {
                    list,
                    user_id: user_id.to_string(),
                });
            }
            users.push(user_id.to_string());
        } else {
            let before = users.len();
            users.retain(|user| user != user_id);
            if users.len() == before {
                return Err(ChannelStoreError::PolicyEntryMissing {
                    list,
                    user_id: user_id.to_string(),
                });
            }
        }

        self.save_policy(&policy);
        Ok(policy)
    }

    #[must_use]
    pub fn is_paired(&self, user_id: &str) -> bool {
        self.paired_users().iter().any(|u| u.user_id == user_id)
    }

    #[must_use]
    pub fn is_blacklisted(&self, user_id: &str) -> bool {
        self.policy().blacklist.iter().any(|b| b == user_id)
    }

    /// Read a JSON file from the store directory.
    fn load_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
    }

    /// Write a JSON file to the store directory.
    fn save_json<T: Serialize + ?Sized>(path: &Path, data: &T) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(data) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyList {
    Whitelist,
    Blacklist,
}

impl PolicyList {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Whitelist => "whitelist",
            Self::Blacklist => "blacklist",
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum ChannelStoreError {
    AlreadyPaired(String),
    PendingUserNotFound(String),
    PairedUserNotFound(String),
    PersistFailed(String),
    InvalidPolicyMode(String),
    PolicyEntryExists { list: PolicyList, user_id: String },
    PolicyEntryMissing { list: PolicyList, user_id: String },
}

impl std::fmt::Display for PolicyList {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::fmt::Display for ChannelStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyPaired(user_id) => write!(f, "user {user_id} is already paired"),
            Self::PendingUserNotFound(user_id) => write!(f, "pending user not found: {user_id}"),
            Self::PairedUserNotFound(user_id) => write!(f, "paired user not found: {user_id}"),
            Self::PersistFailed(message) => f.write_str(message),
            Self::InvalidPolicyMode(mode) => write!(f, "invalid policy mode: {mode}"),
            Self::PolicyEntryExists { list, user_id } => {
                write!(f, "{user_id} already in {list}")
            }
            Self::PolicyEntryMissing { list, user_id } => {
                write!(f, "{user_id} not found in {list}")
            }
        }
    }
}

impl std::error::Error for ChannelStoreError {}

fn current_unix_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or_else(|_| "unknown".to_string(), |d| format!("{}s", d.as_secs()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approve_pending_pair_moves_user_into_paired() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChannelStore::open_dir(dir.path().to_path_buf());
        store.save_pending_pairs(&[PendingPair {
            user_id: "5188621876".into(),
            user_name: "Scott".into(),
            code: "ABC123".into(),
            created_at: "now".into(),
        }]);

        let paired = store.approve_pending_pair("5188621876").unwrap();
        assert_eq!(paired.name, "Scott");
        assert!(!paired.subscribe);
        assert!(store.pending_pairs().is_empty());
        assert!(store.is_paired("5188621876"));
    }

    #[test]
    fn pair_subscription_updates_one_user() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChannelStore::open_dir(dir.path().to_path_buf());
        store.save_paired_users(&[
            PairedUser {
                user_id: "u1".into(),
                name: "One".into(),
                paired_at: "now".into(),
                subscribe: false,
            },
            PairedUser {
                user_id: "u2".into(),
                name: "Two".into(),
                paired_at: "now".into(),
                subscribe: false,
            },
        ]);

        let updated = store.set_pair_subscription("u1", true).unwrap();
        assert!(updated.subscribe);
        let users = store.paired_users();
        assert!(
            users
                .iter()
                .any(|user| user.user_id == "u1" && user.subscribe)
        );
        assert!(
            users
                .iter()
                .any(|user| user.user_id == "u2" && !user.subscribe)
        );
    }

    #[test]
    fn mutate_policy_list_adds_and_removes_entries() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChannelStore::open_dir(dir.path().to_path_buf());

        let policy = store
            .mutate_policy_list(PolicyList::Whitelist, "5188621876", true)
            .unwrap();
        assert_eq!(policy.whitelist, vec!["5188621876".to_string()]);

        let policy = store
            .mutate_policy_list(PolicyList::Whitelist, "5188621876", false)
            .unwrap();
        assert!(policy.whitelist.is_empty());
    }

    #[test]
    fn update_offset_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let store = ChannelStore::open_dir(dir.path().to_path_buf());

        assert_eq!(store.update_offset(), 0);
        store.save_update_offset(604_510_842);
        assert_eq!(store.update_offset(), 604_510_842);
    }
}
