use serde::{Deserialize, Serialize};

pub(super) const DEFAULT_CAPABILITY_CACHE_TTL_HOURS: u64 = 168;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct CachedModelInfo {
    #[serde(default)]
    pub(super) context_window: usize,
    #[serde(default)]
    pub(super) max_output_tokens: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(super) vision_max_output_tokens: Option<usize>,
    pub(super) fetched_at: chrono::DateTime<chrono::Utc>,
}

impl CachedModelInfo {
    pub(super) fn new() -> Self {
        Self {
            context_window: 0,
            max_output_tokens: 0,
            vision_max_output_tokens: None,
            fetched_at: chrono::Utc::now(),
        }
    }

    pub(super) fn is_expired_with_ttl(&self, ttl_hours: u64) -> bool {
        chrono::Utc::now()
            .signed_duration_since(self.fetched_at)
            .num_hours()
            >= i64::try_from(ttl_hours).unwrap_or(i64::MAX)
    }
}
