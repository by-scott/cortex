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
