use std::{cmp::Ordering, collections::HashMap};

use cortex_types::{AttentionChannel, Payload};

const DEFAULT_MAINTENANCE_INTERVAL: usize = 3;
const DEFAULT_ACTOR_BUDGET: usize = 4;
const DEADLINE_PRIORITY_BONUS: i32 = 50;
const INHERITED_PRIORITY_BONUS: i32 = 25;

#[derive(Debug, Clone, PartialEq)]
pub struct TaskOptions {
    pub actor: String,
    pub cost: usize,
    pub risk: f32,
    pub priority: i32,
    pub deadline_after_ticks: Option<u64>,
    pub emergency_debounce_ticks: u64,
    pub inherits_priority_from: Option<AttentionChannel>,
}

impl Default for TaskOptions {
    fn default() -> Self {
        Self {
            actor: "system".to_string(),
            cost: 1,
            risk: 0.0,
            priority: 0,
            deadline_after_ticks: None,
            emergency_debounce_ticks: 1,
            inherits_priority_from: None,
        }
    }
}

impl TaskOptions {
    #[must_use]
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = actor.into();
        self
    }

    #[must_use]
    pub const fn with_cost(mut self, cost: usize) -> Self {
        self.cost = if cost == 0 { 1 } else { cost };
        self
    }

    #[must_use]
    pub const fn with_risk(mut self, risk: f32) -> Self {
        self.risk = risk.clamp(0.0, 1.0);
        self
    }

    #[must_use]
    pub const fn with_priority(mut self, priority: i32) -> Self {
        self.priority = priority;
        self
    }

    #[must_use]
    pub const fn with_deadline_after_ticks(mut self, ticks: u64) -> Self {
        self.deadline_after_ticks = Some(ticks);
        self
    }

    #[must_use]
    pub const fn with_emergency_debounce_ticks(mut self, ticks: u64) -> Self {
        self.emergency_debounce_ticks = ticks;
        self
    }

    #[must_use]
    pub const fn inherit_priority_from(mut self, channel: AttentionChannel) -> Self {
        self.inherits_priority_from = Some(channel);
        self
    }
}

/// A registered task in a channel.
struct ChannelTask {
    name: String,
    options: TaskOptions,
    registered_tick: u64,
    callback: Box<dyn Fn() -> Vec<Payload> + Send>,
}

struct ChannelRun {
    scheduled: Option<Payload>,
    produced: Vec<Payload>,
    executed: usize,
}

/// Three-channel attention scheduler with bounded resource governance.
///
/// The scheduler preserves the original Foreground / Maintenance / Emergency
/// model, but each task now carries enough metadata to explain scheduling
/// decisions: actor budget, maintenance debt, emergency debounce, deadline
/// pressure, risk/cost, and inherited priority.
pub struct ChannelScheduler {
    foreground_tasks: Vec<ChannelTask>,
    maintenance_tasks: Vec<ChannelTask>,
    emergency_tasks: Vec<ChannelTask>,
    foreground_count: usize,
    maintenance_interval: usize,
    actor_budget: usize,
    tick_index: u64,
    maintenance_debt: usize,
    last_emergency_tick: HashMap<String, u64>,
}

impl Default for ChannelScheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl ChannelScheduler {
    #[must_use]
    pub fn new() -> Self {
        Self {
            foreground_tasks: Vec::new(),
            maintenance_tasks: Vec::new(),
            emergency_tasks: Vec::new(),
            foreground_count: 0,
            maintenance_interval: DEFAULT_MAINTENANCE_INTERVAL,
            actor_budget: DEFAULT_ACTOR_BUDGET,
            tick_index: 0,
            maintenance_debt: 0,
            last_emergency_tick: HashMap::new(),
        }
    }

    #[must_use]
    pub fn with_maintenance_interval(mut self, interval: usize) -> Self {
        self.maintenance_interval = interval.max(1);
        self
    }

    #[must_use]
    pub fn with_actor_budget(mut self, budget: usize) -> Self {
        self.actor_budget = budget.max(1);
        self
    }

    #[must_use]
    pub const fn maintenance_debt(&self) -> usize {
        self.maintenance_debt
    }

    #[must_use]
    pub const fn tick_index(&self) -> u64 {
        self.tick_index
    }

    /// Register a named task to a specific channel.
    pub fn register(
        &mut self,
        channel: AttentionChannel,
        name: impl Into<String>,
        callback: impl Fn() -> Vec<Payload> + Send + 'static,
    ) {
        self.register_with_options(channel, name, TaskOptions::default(), callback);
    }

    pub fn register_with_options(
        &mut self,
        channel: AttentionChannel,
        name: impl Into<String>,
        options: TaskOptions,
        callback: impl Fn() -> Vec<Payload> + Send + 'static,
    ) {
        let task = ChannelTask {
            name: name.into(),
            options,
            registered_tick: self.tick_index,
            callback: Box::new(callback),
        };
        match channel {
            AttentionChannel::Foreground => self.foreground_tasks.push(task),
            AttentionChannel::Maintenance => self.maintenance_tasks.push(task),
            AttentionChannel::Emergency => self.emergency_tasks.push(task),
        }
    }

    /// Execute one scheduling cycle.
    ///
    /// Priority is emergency, then maintenance when due, then foreground. Each
    /// channel still applies actor budgets, emergency debounce, deadline
    /// pressure, risk/cost, and inherited-priority ordering.
    pub fn tick(&mut self) -> Vec<Payload> {
        self.tick_index = self.tick_index.saturating_add(1);
        let mut actor_spend = HashMap::new();
        let mut events = Vec::new();

        let emergency = run_channel_tasks(ChannelRunInput {
            channel: AttentionChannel::Emergency,
            tasks: &self.emergency_tasks,
            tick_index: self.tick_index,
            maintenance_debt: self.maintenance_debt,
            actor_budget: self.actor_budget,
            actor_spend: &mut actor_spend,
            last_emergency_tick: &mut self.last_emergency_tick,
        });
        let emergency_executed = emergency.executed > 0;
        push_channel_run(&mut events, emergency);

        let maintenance_due = self.foreground_count.saturating_add(1) >= self.maintenance_interval
            && !self.maintenance_tasks.is_empty();
        if maintenance_due && emergency_executed {
            self.maintenance_debt = self
                .maintenance_debt
                .saturating_add(self.maintenance_tasks.len());
        } else if maintenance_due {
            let maintenance = run_channel_tasks(ChannelRunInput {
                channel: AttentionChannel::Maintenance,
                tasks: &self.maintenance_tasks,
                tick_index: self.tick_index,
                maintenance_debt: self.maintenance_debt,
                actor_budget: self.actor_budget,
                actor_spend: &mut actor_spend,
                last_emergency_tick: &mut self.last_emergency_tick,
            });
            if maintenance.executed > 0 {
                self.foreground_count = 0;
                self.maintenance_debt = 0;
            }
            push_channel_run(&mut events, maintenance);
        } else if !emergency_executed {
            let foreground = run_channel_tasks(ChannelRunInput {
                channel: AttentionChannel::Foreground,
                tasks: &self.foreground_tasks,
                tick_index: self.tick_index,
                maintenance_debt: self.maintenance_debt,
                actor_budget: self.actor_budget,
                actor_spend: &mut actor_spend,
                last_emergency_tick: &mut self.last_emergency_tick,
            });
            push_channel_run(&mut events, foreground);
        }

        self.foreground_count = self.foreground_count.saturating_add(1);
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

struct ChannelRunInput<'a> {
    channel: AttentionChannel,
    tasks: &'a [ChannelTask],
    tick_index: u64,
    maintenance_debt: usize,
    actor_budget: usize,
    actor_spend: &'a mut HashMap<String, usize>,
    last_emergency_tick: &'a mut HashMap<String, u64>,
}

fn run_channel_tasks(input: ChannelRunInput<'_>) -> ChannelRun {
    let ChannelRunInput {
        channel,
        tasks,
        tick_index,
        maintenance_debt,
        actor_budget,
        actor_spend,
        last_emergency_tick,
    } = input;

    if tasks.is_empty() {
        return ChannelRun {
            scheduled: None,
            produced: Vec::new(),
            executed: 0,
        };
    }

    let mut ordered: Vec<&ChannelTask> = tasks.iter().collect();
    ordered.sort_by(|left, right| compare_tasks(left, right, channel, tick_index));

    let mut selected = Vec::new();
    let mut produced = Vec::new();
    let mut budget_blocked = 0usize;
    let mut debounced = 0usize;
    let mut min_budget_remaining = input.actor_budget;
    let mut total_cost = 0usize;
    let mut max_risk = 0.0_f32;
    let mut max_priority = i32::MIN;

    for task in ordered {
        if is_emergency_debounced(task, channel, tick_index, last_emergency_tick) {
            debounced = debounced.saturating_add(1);
            continue;
        }

        let spent = actor_spend.entry(task.options.actor.clone()).or_insert(0);
        if spent.saturating_add(task.options.cost) > actor_budget {
            budget_blocked = budget_blocked.saturating_add(1);
            continue;
        }

        *spent = spent.saturating_add(task.options.cost);
        let remaining = actor_budget.saturating_sub(*spent);
        min_budget_remaining = min_budget_remaining.min(remaining);
        total_cost = total_cost.saturating_add(task.options.cost);
        max_risk = max_risk.max(task.options.risk);
        max_priority = max_priority.max(effective_priority(task, channel, tick_index));
        selected.push(task);

        if channel == AttentionChannel::Emergency {
            last_emergency_tick.insert(task.name.clone(), tick_index);
        }

        if channel == AttentionChannel::Maintenance {
            produced.push(Payload::MaintenanceExecuted {
                task_name: task.name.clone(),
            });
        }
        produced.extend((task.callback)());
    }

    let summary = ChannelScheduleSummary {
        channel,
        selected: &selected,
        budget_blocked,
        debounced,
        maintenance_debt,
        budget_remaining: min_budget_remaining,
        priority: max_priority,
        risk: max_risk,
        cost: total_cost,
    };
    let scheduled = channel_scheduled_payload(&summary);

    ChannelRun {
        scheduled,
        produced,
        executed: selected.len(),
    }
}

struct ChannelScheduleSummary<'a> {
    channel: AttentionChannel,
    selected: &'a [&'a ChannelTask],
    budget_blocked: usize,
    debounced: usize,
    maintenance_debt: usize,
    budget_remaining: usize,
    priority: i32,
    risk: f32,
    cost: usize,
}

fn channel_scheduled_payload(summary: &ChannelScheduleSummary<'_>) -> Option<Payload> {
    if summary.selected.is_empty() && summary.budget_blocked == 0 && summary.debounced == 0 {
        return None;
    }

    let task_count = summary.selected.len();
    let actor = selected_actor_label(summary.selected);
    let priority = if task_count == 0 { 0 } else { summary.priority };

    Some(Payload::ChannelScheduled {
        channel: summary.channel.to_string(),
        task_count,
        actor,
        explanation: schedule_explanation(summary),
        maintenance_debt: summary.maintenance_debt,
        emergency_debounced: summary.debounced,
        budget_remaining: summary.budget_remaining,
        priority,
        risk: summary.risk,
        cost: summary.cost,
    })
}

fn push_channel_run(events: &mut Vec<Payload>, run: ChannelRun) {
    if let Some(event) = run.scheduled {
        events.push(event);
    }
    events.extend(run.produced);
}

fn compare_tasks(
    left: &ChannelTask,
    right: &ChannelTask,
    channel: AttentionChannel,
    tick_index: u64,
) -> Ordering {
    effective_priority(right, channel, tick_index)
        .cmp(&effective_priority(left, channel, tick_index))
        .then_with(|| deadline_due(right, tick_index).cmp(&deadline_due(left, tick_index)))
        .then_with(|| left.options.risk.total_cmp(&right.options.risk))
        .then_with(|| left.name.cmp(&right.name))
}

fn effective_priority(task: &ChannelTask, channel: AttentionChannel, tick_index: u64) -> i32 {
    let mut priority = task.options.priority;
    if deadline_due(task, tick_index) {
        priority = priority.saturating_add(DEADLINE_PRIORITY_BONUS);
    }
    if task
        .options
        .inherits_priority_from
        .is_some_and(|source| source != channel)
    {
        priority = priority.saturating_add(INHERITED_PRIORITY_BONUS);
    }
    priority
}

fn deadline_due(task: &ChannelTask, tick_index: u64) -> bool {
    task.options
        .deadline_after_ticks
        .is_some_and(|deadline| tick_index.saturating_sub(task.registered_tick) >= deadline)
}

fn is_emergency_debounced(
    task: &ChannelTask,
    channel: AttentionChannel,
    tick_index: u64,
    last_emergency_tick: &HashMap<String, u64>,
) -> bool {
    if channel != AttentionChannel::Emergency {
        return false;
    }
    last_emergency_tick
        .get(&task.name)
        .is_some_and(|last_tick| {
            tick_index.saturating_sub(*last_tick) <= task.options.emergency_debounce_ticks
        })
}

fn selected_actor_label(selected: &[&ChannelTask]) -> String {
    let Some(first) = selected.first() else {
        return "none".to_string();
    };
    let actor = &first.options.actor;
    if selected
        .iter()
        .all(|task| task.options.actor.as_str() == actor.as_str())
    {
        actor.clone()
    } else {
        "mixed".to_string()
    }
}

fn schedule_explanation(summary: &ChannelScheduleSummary<'_>) -> String {
    let mut parts = vec![format!(
        "{} selected {} task(s)",
        summary.channel,
        summary.selected.len()
    )];
    if summary.maintenance_debt > 0 {
        parts.push(format!("maintenance debt {}", summary.maintenance_debt));
    }
    if summary.budget_blocked > 0 {
        parts.push(format!(
            "{} task(s) skipped by actor budget",
            summary.budget_blocked
        ));
    }
    if summary.debounced > 0 {
        parts.push(format!("{} emergency task(s) debounced", summary.debounced));
    }
    if summary
        .selected
        .iter()
        .any(|task| task.options.inherits_priority_from.is_some())
    {
        parts.push("priority inheritance applied".to_string());
    }
    if summary
        .selected
        .iter()
        .any(|task| deadline_due(task, u64::MAX))
    {
        parts.push("deadline pressure applied".to_string());
    }
    parts.join("; ")
}
