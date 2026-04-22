use chrono::Utc;
use cortex_kernel::MemoryStore;
use cortex_types::MemoryStatus;

use super::decay;

const CAPTURED_COUNT_THRESHOLD: usize = 5;

/// Check if memory extraction should run.
#[must_use]
pub const fn should_extract(turns_since_extract: usize, extract_min_turns: usize) -> bool {
    turns_since_extract >= extract_min_turns
}

/// Check if consolidation should run (time-based or count-based).
#[must_use]
pub fn should_consolidate(
    hours_since_last: f64,
    consolidate_interval_hours: u64,
    captured_count: usize,
) -> bool {
    hours_since_last >= f64::from(u32::try_from(consolidate_interval_hours).unwrap_or(u32::MAX))
        || captured_count >= CAPTURED_COUNT_THRESHOLD
}

/// Deprecate memories that have decayed below the freshness threshold.
///
/// # Errors
/// Returns `io::Error` if the store cannot be read or written.
pub fn deprecate_expired(store: &MemoryStore, decay_rate: f64) -> Result<usize, std::io::Error> {
    let mut entries = store.list_all()?;
    let now = Utc::now();
    let mut count = 0;
    for entry in &mut entries {
        if entry.status == MemoryStatus::Deprecated {
            continue;
        }
        let secs = now.signed_duration_since(entry.updated_at).num_seconds();
        let hours = f64::from(i32::try_from(secs).unwrap_or(i32::MAX)) / 3600.0;
        if decay::should_deprecate(hours, entry.access_count, decay_rate) {
            entry.status = MemoryStatus::Deprecated;
            store.save(entry)?;
            count += 1;
        }
    }
    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_below_threshold() {
        assert!(!should_extract(2, 5));
    }

    #[test]
    fn extract_at_threshold() {
        assert!(should_extract(5, 5));
    }

    #[test]
    fn extract_above_threshold() {
        assert!(should_extract(10, 5));
    }

    #[test]
    fn consolidate_by_time() {
        assert!(should_consolidate(25.0, 24, 0));
    }

    #[test]
    fn consolidate_by_count() {
        assert!(should_consolidate(1.0, 24, 5));
    }

    #[test]
    fn no_consolidate() {
        assert!(!should_consolidate(1.0, 24, 2));
    }
}
