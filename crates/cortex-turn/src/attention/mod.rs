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
