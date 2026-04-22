use cortex_types::{AttentionChannel, Payload};

const DEFAULT_MAINTENANCE_INTERVAL: usize = 3;

/// A registered task in a channel.
struct ChannelTask {
    name: String,
    callback: Box<dyn Fn() -> Vec<Payload> + Send>,
}

/// Three-channel attention scheduler (Posner triple-network model).
///
/// Manages foreground, maintenance, and emergency channels with
/// priority scheduling and anti-starvation guarantees.
pub struct ChannelScheduler {
    foreground_tasks: Vec<ChannelTask>,
    maintenance_tasks: Vec<ChannelTask>,
    emergency_tasks: Vec<ChannelTask>,
    foreground_count: usize,
    maintenance_interval: usize,
}

impl Default for ChannelScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelScheduler {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            foreground_tasks: Vec::new(),
            maintenance_tasks: Vec::new(),
            emergency_tasks: Vec::new(),
            foreground_count: 0,
            maintenance_interval: DEFAULT_MAINTENANCE_INTERVAL,
        }
    }

    #[must_use]
    pub fn with_maintenance_interval(mut self, interval: usize) -> Self {
        self.maintenance_interval = interval.max(1);
        self
    }

    /// Register a named task to a specific channel.
    pub fn register(
        &mut self,
        channel: AttentionChannel,
        name: impl Into<String>,
        callback: impl Fn() -> Vec<Payload> + Send + 'static,
    ) {
        let task = ChannelTask {
            name: name.into(),
            callback: Box::new(callback),
        };
        match channel {
            AttentionChannel::Foreground => self.foreground_tasks.push(task),
            AttentionChannel::Maintenance => self.maintenance_tasks.push(task),
            AttentionChannel::Emergency => self.emergency_tasks.push(task),
        }
    }

    /// Execute one scheduling cycle. Priority: emergency then maintenance (if due) then foreground.
    ///
    /// Returns all events produced by executed tasks.
    pub fn tick(&mut self) -> Vec<Payload> {
        let mut events = Vec::new();

        // 1. Always execute Emergency channel
        if !self.emergency_tasks.is_empty() {
            events.push(Payload::ChannelScheduled {
                channel: AttentionChannel::Emergency.to_string(),
                task_count: self.emergency_tasks.len(),
            });
            for task in &self.emergency_tasks {
                let task_events = (task.callback)();
                if !task_events.is_empty() {
                    events.extend(task_events);
                }
            }
        }

        // 2. Execute Maintenance if anti-starvation threshold reached
        if self.foreground_count >= self.maintenance_interval && !self.maintenance_tasks.is_empty()
        {
            events.push(Payload::ChannelScheduled {
                channel: AttentionChannel::Maintenance.to_string(),
                task_count: self.maintenance_tasks.len(),
            });
            for task in &self.maintenance_tasks {
                let task_events = (task.callback)();
                events.push(Payload::MaintenanceExecuted {
                    task_name: task.name.clone(),
                });
                events.extend(task_events);
            }
            self.foreground_count = 0;
        }

        // 3. Increment foreground counter (represents one foreground iteration)
        self.foreground_count += 1;

        events
    }

    /// Number of registered tasks per channel.
    #[must_use]
    pub const fn task_counts(&self) -> (usize, usize, usize) {
        (
            self.foreground_tasks.len(),
            self.maintenance_tasks.len(),
            self.emergency_tasks.len(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn empty_scheduler_produces_no_events() {
        let mut sched = ChannelScheduler::new();
        let events = sched.tick();
        assert!(events.is_empty());
    }

    #[test]
    fn emergency_always_executes() {
        let mut sched = ChannelScheduler::new();
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        sched.register(AttentionChannel::Emergency, "pressure_check", move || {
            c.fetch_add(1, Ordering::SeqCst);
            vec![]
        });

        sched.tick();
        sched.tick();
        sched.tick();
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn maintenance_executes_after_interval() {
        let mut sched = ChannelScheduler::new().with_maintenance_interval(3);
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        sched.register(AttentionChannel::Maintenance, "meta_check", move || {
            c.fetch_add(1, Ordering::SeqCst);
            vec![]
        });

        // Ticks 1, 2: foreground_count = 0, 1 (initialized, then incremented)
        // Maintenance only fires when foreground_count >= 3
        sched.tick(); // fc=0 -> no maintenance -> fc=1
        sched.tick(); // fc=1 -> no maintenance -> fc=2
        sched.tick(); // fc=2 -> no maintenance -> fc=3
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        sched.tick(); // fc=3 -> maintenance fires! -> fc reset to 0 -> fc=1
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn anti_starvation_resets_counter() {
        let mut sched = ChannelScheduler::new().with_maintenance_interval(2);
        let counter = Arc::new(AtomicUsize::new(0));
        let c = counter.clone();
        sched.register(AttentionChannel::Maintenance, "check", move || {
            c.fetch_add(1, Ordering::SeqCst);
            vec![]
        });

        // interval=2: maintenance fires when fc>=2
        sched.tick(); // fc=0->1
        sched.tick(); // fc=1->2
        sched.tick(); // fc=2 -> fires, reset -> fc=1
        assert_eq!(counter.load(Ordering::SeqCst), 1);

        sched.tick(); // fc=1->2
        sched.tick(); // fc=2 -> fires, reset -> fc=1
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn emergency_events_returned() {
        let mut sched = ChannelScheduler::new();
        sched.register(AttentionChannel::Emergency, "guard", || {
            vec![Payload::EmergencyTriggered {
                task_name: "guard".into(),
                details: "injection detected".into(),
            }]
        });

        let events = sched.tick();
        assert!(events.iter().any(|e| matches!(
            e,
            Payload::EmergencyTriggered { task_name, .. } if task_name == "guard"
        )));
    }

    #[test]
    fn maintenance_events_include_executed_marker() {
        let mut sched = ChannelScheduler::new().with_maintenance_interval(1);
        sched.register(
            AttentionChannel::Maintenance,
            "meta_check",
            std::vec::Vec::new,
        );

        // First tick: fc=0 < 1, no maintenance
        sched.tick();
        // Second tick: fc=1 >= 1, maintenance fires
        let events = sched.tick();
        assert!(events.iter().any(|e| matches!(
            e,
            Payload::MaintenanceExecuted { task_name } if task_name == "meta_check"
        )));
    }

    #[test]
    fn task_counts_correct() {
        let mut sched = ChannelScheduler::new();
        sched.register(AttentionChannel::Foreground, "fg1", std::vec::Vec::new);
        sched.register(AttentionChannel::Foreground, "fg2", std::vec::Vec::new);
        sched.register(AttentionChannel::Maintenance, "mt1", std::vec::Vec::new);
        sched.register(AttentionChannel::Emergency, "em1", std::vec::Vec::new);

        assert_eq!(sched.task_counts(), (2, 1, 1));
    }
}
