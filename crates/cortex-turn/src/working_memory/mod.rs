use chrono::{DateTime, Utc};
use cortex_types::{Payload, WorkingMemoryItem};

const MIN_CAPACITY: usize = 3;
const MAX_CAPACITY: usize = 7;
const REHEARSAL_BOOST: f64 = 0.2;
const EVICTION_THRESHOLD: f64 = 0.1;
const DECAY_RATE: f64 = 0.1;

/// Result of activating a new item in working memory.
pub struct ActivateResult {
    /// The item that was evicted to make room, if any.
    pub evicted: Option<WorkingMemoryItem>,
    /// Events to emit to the journal.
    pub events: Vec<Payload>,
}

/// Manages a capacity-constrained set of active items in working memory.
///
/// Implements the `4+-1` active items constraint from charter S1 (Baddeley/Cowan model).
/// All operations produce [`Payload`] variants for journal audit.
pub struct WorkingMemoryManager {
    items: Vec<WorkingMemoryItem>,
    capacity: usize,
}

impl WorkingMemoryManager {
    #[must_use]
    pub fn new(capacity: usize) -> Self {
        Self {
            items: Vec::new(),
            capacity: capacity.clamp(MIN_CAPACITY, MAX_CAPACITY),
        }
    }

    /// Activate a new item. If at capacity, evicts the lowest-relevance item first.
    ///
    /// # Panics
    /// Panics if floating-point comparison fails (should not happen with clamped values).
    pub fn activate(&mut self, tag: impl Into<String>, relevance: f64) -> ActivateResult {
        let tag = tag.into();
        let relevance = relevance.clamp(0.0, 1.0);
        let mut events = Vec::new();
        let mut evicted = None;

        // If at capacity, evict lowest relevance item
        if self.items.len() >= self.capacity {
            events.push(Payload::WorkingMemoryCapacityExceeded {
                current_count: self.items.len(),
                capacity: self.capacity,
            });

            if let Some(min_idx) = self
                .items
                .iter()
                .enumerate()
                .min_by(|(_, a), (_, b)| {
                    a.relevance
                        .partial_cmp(&b.relevance)
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|(i, _)| i)
            {
                let removed = self.items.remove(min_idx);
                events.push(Payload::WorkingMemoryItemEvicted {
                    tag: removed.tag.clone(),
                    reason: "capacity_overflow".into(),
                });
                evicted = Some(removed);
            }
        }

        // Add the new item
        let item = WorkingMemoryItem::new(&tag, relevance);
        self.items.push(item);

        events.push(Payload::WorkingMemoryItemActivated { tag, relevance });

        ActivateResult { evicted, events }
    }

    /// Rehearse an active item by tag, boosting its relevance and refreshing its timestamp.
    /// Returns the events to emit (empty if no match found).
    pub fn rehearse(&mut self, tag: &str) -> Vec<Payload> {
        for item in &mut self.items {
            if item.tag == tag {
                item.relevance = (item.relevance + REHEARSAL_BOOST).min(1.0);
                item.last_rehearsed = Utc::now();
                return vec![Payload::WorkingMemoryItemRehearsed {
                    tag: tag.to_string(),
                    new_relevance: item.relevance,
                }];
            }
        }
        Vec::new()
    }

    /// Apply time-based decay to all items and evict those below threshold.
    /// Returns events for each evicted item.
    pub fn decay_and_evict(&mut self, now: DateTime<Utc>) -> Vec<Payload> {
        let mut events = Vec::new();

        // Apply exponential decay
        for item in &mut self.items {
            let elapsed = f64::from(
                u32::try_from(
                    now.signed_duration_since(item.last_rehearsed)
                        .num_seconds()
                        .max(0),
                )
                .unwrap_or(u32::MAX),
            );
            item.relevance *= (-DECAY_RATE * elapsed).exp();
        }

        // Evict items below threshold
        let mut i = 0;
        while i < self.items.len() {
            if self.items[i].relevance < EVICTION_THRESHOLD {
                let removed = self.items.remove(i);
                events.push(Payload::WorkingMemoryItemEvicted {
                    tag: removed.tag,
                    reason: "decay_below_threshold".into(),
                });
            } else {
                i += 1;
            }
        }

        events
    }

    /// Number of currently active items.
    #[must_use]
    pub const fn active_count(&self) -> usize {
        self.items.len()
    }

    /// Read-only access to active items.
    #[must_use]
    pub fn items(&self) -> &[WorkingMemoryItem] {
        &self.items
    }

    /// The configured capacity.
    #[must_use]
    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    /// Mutable access to items (for testing and manual timestamp manipulation).
    pub const fn items_mut(&mut self) -> &mut Vec<WorkingMemoryItem> {
        &mut self.items
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn capacity_clamped_to_range() {
        assert_eq!(WorkingMemoryManager::new(1).capacity(), MIN_CAPACITY);
        assert_eq!(WorkingMemoryManager::new(100).capacity(), MAX_CAPACITY);
        assert_eq!(WorkingMemoryManager::new(5).capacity(), 5);
    }

    #[test]
    fn activate_under_capacity() {
        let mut wm = WorkingMemoryManager::new(5);
        let result = wm.activate("read", 0.8);
        assert!(result.evicted.is_none());
        assert_eq!(wm.active_count(), 1);
        assert_eq!(
            result.events.len(),
            1,
            "should have only ItemActivated event"
        );
        assert!(matches!(
            &result.events[0],
            Payload::WorkingMemoryItemActivated { tag, relevance }
            if tag == "read" && (*relevance - 0.8).abs() < f64::EPSILON
        ));
    }

    #[test]
    fn activate_at_capacity_evicts_lowest() {
        let mut wm = WorkingMemoryManager::new(3);
        wm.activate("a", 0.5);
        wm.activate("b", 0.3);
        wm.activate("c", 0.9);

        // At capacity (3), adding "d" should evict "b" (lowest relevance 0.3)
        let result = wm.activate("d", 0.7);
        assert_eq!(wm.active_count(), 3);
        assert!(result.evicted.is_some());
        assert_eq!(result.evicted.unwrap().tag, "b");

        // Should have: CapacityExceeded, ItemEvicted, ItemActivated
        assert_eq!(result.events.len(), 3);
        assert!(matches!(
            &result.events[0],
            Payload::WorkingMemoryCapacityExceeded {
                current_count: 3,
                capacity: 3
            }
        ));
        assert!(matches!(
            &result.events[1],
            Payload::WorkingMemoryItemEvicted { tag, reason }
            if tag == "b" && reason == "capacity_overflow"
        ));
    }

    #[test]
    fn rehearse_matching_tag() {
        let mut wm = WorkingMemoryManager::new(5);
        wm.activate("read", 0.5);

        let events = wm.rehearse("read");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            Payload::WorkingMemoryItemRehearsed { tag, new_relevance }
            if tag == "read" && (*new_relevance - 0.7).abs() < f64::EPSILON
        ));

        assert!((wm.items()[0].relevance - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn rehearse_no_match() {
        let mut wm = WorkingMemoryManager::new(5);
        wm.activate("read", 0.5);

        let events = wm.rehearse("write");
        assert!(events.is_empty());
    }

    #[test]
    fn rehearse_caps_at_one() {
        let mut wm = WorkingMemoryManager::new(5);
        wm.activate("tool", 0.95);

        wm.rehearse("tool");
        assert!((wm.items()[0].relevance - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn decay_and_evict_removes_stale_items() {
        let mut wm = WorkingMemoryManager::new(5);
        wm.activate("old", 0.15);
        wm.activate("fresh", 0.9);

        // Backdate "old" item's last_rehearsed to 30 seconds ago
        // so decay applies significantly to it but not to "fresh"
        let now = Utc::now();
        wm.items_mut()[0].last_rehearsed = now - Duration::seconds(30);

        // "old": 0.15 * exp(-0.1 * 30) = 0.15 * exp(-3) ~ 0.0075 < 0.1 -> evicted
        // "fresh": rehearsed now, 0 seconds elapsed -> 0.9 * exp(0) = 0.9 -> kept
        let events = wm.decay_and_evict(now);

        assert_eq!(wm.active_count(), 1);
        assert_eq!(wm.items()[0].tag, "fresh");
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            Payload::WorkingMemoryItemEvicted { tag, reason }
            if tag == "old" && reason == "decay_below_threshold"
        ));
    }

    #[test]
    fn decay_preserves_high_relevance_items() {
        let mut wm = WorkingMemoryManager::new(5);
        wm.activate("strong", 1.0);

        // After 5 seconds: 1.0 * exp(-0.1 * 5) = 1.0 * exp(-0.5) ~ 0.607
        let future = Utc::now() + Duration::seconds(5);
        let events = wm.decay_and_evict(future);

        assert!(events.is_empty());
        assert_eq!(wm.active_count(), 1);
        assert!(wm.items()[0].relevance > 0.5);
    }
}
