//! Cron tool and queue infrastructure for scheduled task execution.
//!
//! The `CronTool` allows the LLM to schedule recurring or one-shot
//! tasks.  The `CronQueue` stores schedule state; the runtime cron scheduler
//! promotes due tasks and executes pending work when the daemon is idle.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use super::{Tool, ToolError, ToolResult};

// ── CronTool ─────────────────────────────────────────────────

/// Tool for the LLM to create, list, and cancel scheduled tasks.
pub struct CronTool {
    queue: Arc<CronQueue>,
}

impl CronTool {
    #[must_use]
    pub const fn new(queue: Arc<CronQueue>) -> Self {
        Self { queue }
    }
}

impl Tool for CronTool {
    fn name(&self) -> &'static str {
        "cron"
    }

    fn description(&self) -> &'static str {
        "Create, list, or cancel scheduled prompts. Use for recurring reports, checks, deferred \
         one-shot work, and maintenance reminders. Cron uses 5-field UTC syntax. Due runs are \
         persisted first, then executed when the runtime can make an autonomous LLM call."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "cancel"],
                    "description": "create, list, or cancel."
                },
                "cron": {
                    "type": "string",
                    "description": "5-field cron expression for create."
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt executed on trigger."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID for cancel."
                },
                "recurring": {
                    "type": "boolean",
                    "default": true,
                    "description": "false means fire once then delete."
                }
            },
            "required": ["action"]
        })
    }

    fn execute(&self, input: serde_json::Value) -> Result<ToolResult, ToolError> {
        let action = input
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::InvalidInput("missing 'action'".into()))?;

        match action {
            "create" => {
                let cron_expr = input
                    .get("cron")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidInput("'create' requires 'cron'".into()))?;
                let prompt = input
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidInput("'create' requires 'prompt'".into()))?;
                let recurring = input
                    .get("recurring")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true);

                let scheduled = self
                    .queue
                    .add(cron_expr, prompt, recurring)
                    .map_err(ToolError::InvalidInput)?;
                let kind = if recurring { "recurring" } else { "one-shot" };
                Ok(ToolResult::success(format!(
                    "Scheduled {kind} task {} with cron=\"{cron_expr}\" next_run=\"{}\".",
                    scheduled.id, scheduled.next_run
                )))
            }
            "list" => {
                use std::fmt::Write;
                let entries = self.queue.list();
                if entries.is_empty() {
                    return Ok(ToolResult::success("No scheduled tasks."));
                }
                let mut out = format!("{} scheduled task(s):", entries.len());
                for e in &entries {
                    let kind = if e.recurring { "recurring" } else { "one-shot" };
                    let state = e.pending_due_at.as_ref().map_or_else(
                        || {
                            e.next_run.as_ref().map_or_else(
                                || "inactive".to_string(),
                                |next| format!("next_run=\"{next}\""),
                            )
                        },
                        |pending| format!("pending_due_at=\"{pending}\""),
                    );
                    let _ = write!(
                        out,
                        "\n  - {} [{kind}] cron=\"{}\" {state} prompt=\"{}\"",
                        e.id, e.cron_expr, e.prompt
                    );
                }
                Ok(ToolResult::success(out))
            }
            "cancel" => {
                let task_id = input
                    .get("task_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ToolError::InvalidInput("'cancel' requires 'task_id'".into()))?;
                if self.queue.cancel(task_id) {
                    Ok(ToolResult::success(format!("Cancelled task {task_id}.")))
                } else {
                    Ok(ToolResult::error(format!("Task {task_id} not found.")))
                }
            }
            other => Err(ToolError::InvalidInput(format!(
                "unknown action: '{other}'. Use create, list, or cancel."
            ))),
        }
    }

    fn capabilities(&self) -> cortex_sdk::ToolCapabilities {
        cortex_sdk::ToolCapabilities::default().with_effect(
            cortex_sdk::ToolEffect::new(cortex_sdk::ToolEffectKind::ScheduleTask)
                .with_target("action"),
        )
    }
}

// ── CronEntry ──────────────────────────────────────────────────

/// A single scheduled task persisted in the cron queue.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronEntry {
    pub id: String,
    pub cron_expr: String,
    pub prompt: String,
    pub recurring: bool,
    pub created_at: String,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub pending_due_at: Option<String>,
}

/// Result returned when a task is scheduled.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronScheduled {
    pub id: String,
    pub next_run: String,
}

/// A due task ready to be executed by the runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronInvocation {
    pub task_id: String,
    pub due_at: String,
    pub prompt: String,
}

// ── CronQueue (runtime infrastructure) ─────────────────────────

/// Persistent queue of scheduled cron tasks.
///
/// The queue captures due tasks as pending work. The runtime cron scheduler can
/// then execute pending work later without losing a scheduled run while the
/// daemon is busy.
pub struct CronQueue {
    path: PathBuf,
    lock: Mutex<()>,
}

impl CronQueue {
    /// Open (or create) the cron queue at `data_dir/cron_queue.json`.
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("cron_queue.json"),
            lock: Mutex::new(()),
        }
    }

    /// Add a new scheduled task. Returns the generated task ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the cron expression is invalid or cannot produce a
    /// next run within the supported scheduling horizon.
    pub fn add(
        &self,
        cron_expr: &str,
        prompt: &str,
        recurring: bool,
    ) -> Result<CronScheduled, String> {
        validate_cron_expr(cron_expr)?;
        let now = chrono::Utc::now();
        let next_run = next_run_after(cron_expr, &now)
            .ok_or_else(|| "cron expression has no next run within five years".to_string())?;
        let id = uuid::Uuid::now_v7().to_string();
        let entry = CronEntry {
            id: id.clone(),
            cron_expr: cron_expr.into(),
            prompt: prompt.into(),
            recurring,
            created_at: now.to_rfc3339(),
            last_run: None,
            next_run: Some(next_run.to_rfc3339()),
            pending_due_at: None,
        };
        let _guard = self.guard();
        let mut entries = self.load_unlocked();
        entries.push(entry);
        self.save_unlocked(&entries);
        Ok(CronScheduled {
            id,
            next_run: next_run.to_rfc3339(),
        })
    }

    /// List all scheduled tasks.
    #[must_use]
    pub fn list(&self) -> Vec<CronEntry> {
        let _guard = self.guard();
        self.load_unlocked()
    }

    /// Cancel a task by ID. Returns `true` if the task was found and removed.
    #[must_use]
    pub fn cancel(&self, task_id: &str) -> bool {
        let _guard = self.guard();
        let mut entries = self.load_unlocked();
        let before = entries.len();
        entries.retain(|e| e.id != task_id);
        if entries.len() < before {
            self.save_unlocked(&entries);
            true
        } else {
            false
        }
    }

    /// Promote due schedules into durable pending invocations.
    #[must_use]
    pub fn promote_due(&self) -> usize {
        let _guard = self.guard();
        let mut entries = self.load_unlocked();
        let now = chrono::Utc::now();
        let mut promoted = 0usize;
        let mut changed = false;

        for entry in &mut entries {
            if entry.pending_due_at.is_some() {
                continue;
            }

            let next_run = parse_time(entry.next_run.as_deref().unwrap_or(""))
                .or_else(|| next_run_after(&entry.cron_expr, &now));
            let Some(next_run) = next_run else {
                continue;
            };

            if entry.next_run.as_deref() != Some(next_run.to_rfc3339().as_str()) {
                entry.next_run = Some(next_run.to_rfc3339());
                changed = true;
            }

            if next_run <= now {
                entry.pending_due_at = Some(next_run.to_rfc3339());
                entry.next_run = if entry.recurring {
                    next_run_after(&entry.cron_expr, &now).map(|dt| dt.to_rfc3339())
                } else {
                    None
                };
                promoted += 1;
                changed = true;
            }
        }

        if changed {
            self.save_unlocked(&entries);
        }
        promoted
    }

    /// Return all pending invocations without marking them complete.
    #[must_use]
    pub fn pending(&self) -> Vec<CronInvocation> {
        let _guard = self.guard();
        let mut pending: Vec<_> = self
            .load_unlocked()
            .into_iter()
            .filter_map(|entry| {
                let due_at = entry.pending_due_at?;
                Some(CronInvocation {
                    task_id: entry.id,
                    due_at,
                    prompt: entry.prompt,
                })
            })
            .collect();
        pending.sort_by(|a, b| a.due_at.cmp(&b.due_at).then(a.task_id.cmp(&b.task_id)));
        pending
    }

    /// Return the next known pending or scheduled wake time.
    #[must_use]
    pub fn next_wake_at(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        let _guard = self.guard();
        self.load_unlocked()
            .into_iter()
            .filter_map(|entry| {
                entry
                    .pending_due_at
                    .as_deref()
                    .or(entry.next_run.as_deref())
                    .and_then(parse_time)
            })
            .min()
    }

    /// Mark a pending invocation complete. Recurring tasks remain scheduled;
    /// one-shot tasks are removed.
    #[must_use]
    pub fn complete(&self, task_id: &str, due_at: &str) -> bool {
        let _guard = self.guard();
        let entries = self.load_unlocked();
        let now = chrono::Utc::now();
        let mut changed = false;
        let mut remaining = Vec::with_capacity(entries.len());

        for mut entry in entries {
            let matches_pending =
                entry.id == task_id && entry.pending_due_at.as_deref() == Some(due_at);
            if matches_pending {
                changed = true;
                if entry.recurring {
                    entry.pending_due_at = None;
                    entry.last_run = Some(now.to_rfc3339());
                    let next_is_stale = entry
                        .next_run
                        .as_deref()
                        .and_then(parse_time)
                        .is_none_or(|next| next <= now);
                    if next_is_stale {
                        entry.next_run =
                            next_run_after(&entry.cron_expr, &now).map(|dt| dt.to_rfc3339());
                    }
                    remaining.push(entry);
                }
            } else {
                remaining.push(entry);
            }
        }

        if changed {
            self.save_unlocked(&remaining);
        }
        changed
    }

    fn guard(&self) -> MutexGuard<'_, ()> {
        self.lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn load_unlocked(&self) -> Vec<CronEntry> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save_unlocked(&self, entries: &[CronEntry]) {
        if let Ok(json) = serde_json::to_string_pretty(entries) {
            let _ = cortex_kernel::atomic_write(&self.path, json.as_bytes());
        }
    }
}

// ── Cron expression matching ───────────────────────────────────

/// Check whether the current UTC minute matches a 5-field cron expression.
fn cron_matches(cron_expr: &str, now: &chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::Datelike;
    use chrono::Timelike;

    let fields: Vec<&str> = cron_expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let minute = now.minute();
    let hour = now.hour();
    let dom = now.day();
    let month = now.month();
    let dow = now.weekday().num_days_from_sunday();

    field_matches(fields[0], CronField::MINUTE, minute)
        && field_matches(fields[1], CronField::HOUR, hour)
        && field_matches(fields[2], CronField::DAY_OF_MONTH, dom)
        && field_matches(fields[3], CronField::MONTH, month)
        && field_matches(fields[4], CronField::DAY_OF_WEEK, dow)
}

fn validate_cron_expr(cron_expr: &str) -> Result<(), String> {
    let fields: Vec<&str> = cron_expr.split_whitespace().collect();
    if fields.len() != 5 {
        return Err("cron expression must have exactly 5 fields".into());
    }
    field_values(fields[0], CronField::MINUTE)?;
    field_values(fields[1], CronField::HOUR)?;
    field_values(fields[2], CronField::DAY_OF_MONTH)?;
    field_values(fields[3], CronField::MONTH)?;
    field_values(fields[4], CronField::DAY_OF_WEEK)?;
    Ok(())
}

fn next_run_after(
    cron_expr: &str,
    after: &chrono::DateTime<chrono::Utc>,
) -> Option<chrono::DateTime<chrono::Utc>> {
    use chrono::Timelike;

    let mut cursor = *after + chrono::Duration::minutes(1);
    cursor = cursor
        .with_second(0)
        .and_then(|dt| dt.with_nanosecond(0))
        .unwrap_or(cursor);

    for _ in 0..=5 * 366 * 24 * 60 {
        if cron_matches(cron_expr, &cursor) {
            return Some(cursor);
        }
        cursor += chrono::Duration::minutes(1);
    }
    None
}

fn parse_time(value: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&chrono::Utc))
}

#[derive(Clone, Copy)]
struct CronField {
    min: u32,
    max: u32,
    sunday_seven: bool,
}

impl CronField {
    const MINUTE: Self = Self::new(0, 59, false);
    const HOUR: Self = Self::new(0, 23, false);
    const DAY_OF_MONTH: Self = Self::new(1, 31, false);
    const MONTH: Self = Self::new(1, 12, false);
    const DAY_OF_WEEK: Self = Self::new(0, 7, true);

    const fn new(min: u32, max: u32, sunday_seven: bool) -> Self {
        Self {
            min,
            max,
            sunday_seven,
        }
    }

    const fn canonical_max(self) -> u32 {
        if self.sunday_seven { 6 } else { self.max }
    }

    fn normalize(self, raw: u32) -> Option<u32> {
        if self.sunday_seven && raw == 7 {
            return Some(0);
        }
        (raw >= self.min && raw <= self.canonical_max()).then_some(raw)
    }
}

fn field_matches(field: &str, spec: CronField, value: u32) -> bool {
    field_values(field, spec).is_ok_and(|values| values.contains(&value))
}

fn field_values(field: &str, spec: CronField) -> Result<BTreeSet<u32>, String> {
    let mut values = BTreeSet::new();
    for part in field.split(',') {
        add_part_values(part.trim(), spec, &mut values)?;
    }
    if values.is_empty() {
        Err(format!("invalid empty cron field '{field}'"))
    } else {
        Ok(values)
    }
}

fn add_part_values(part: &str, spec: CronField, values: &mut BTreeSet<u32>) -> Result<(), String> {
    if part.is_empty() {
        return Err("cron fields cannot contain empty list entries".into());
    }

    let (base, step) = match part.split_once('/') {
        Some((base, step)) => {
            let step = step
                .parse::<u32>()
                .map_err(|_| format!("invalid cron step '{step}'"))?;
            if step == 0 {
                return Err("cron step must be greater than zero".into());
            }
            (base, step)
        }
        None => (part, 1),
    };

    if base == "*" {
        for (idx, raw) in (spec.min..=spec.canonical_max()).enumerate() {
            if idx.is_multiple_of(step as usize)
                && let Some(value) = spec.normalize(raw)
            {
                values.insert(value);
            }
        }
        return Ok(());
    }

    if let Some((lo, hi)) = base.split_once('-') {
        let lo = parse_raw(lo, spec)?;
        let hi = parse_raw(hi, spec)?;
        if lo > hi {
            return Err(format!("invalid cron range '{base}'"));
        }
        for (idx, raw) in (lo..=hi).enumerate() {
            if idx.is_multiple_of(step as usize)
                && let Some(value) = spec.normalize(raw)
            {
                values.insert(value);
            }
        }
        return Ok(());
    }

    if step != 1 {
        return Err(format!("cron step requires '*' or a range in '{part}'"));
    }
    values.insert(spec.normalize(parse_raw(base, spec)?).ok_or_else(|| {
        format!(
            "cron value '{base}' is outside supported range {}-{}",
            spec.min, spec.max
        )
    })?);
    Ok(())
}

fn parse_raw(value: &str, spec: CronField) -> Result<u32, String> {
    let raw = value
        .parse::<u32>()
        .map_err(|_| format!("invalid cron value '{value}'"))?;
    if raw < spec.min || raw > spec.max {
        return Err(format!(
            "cron value '{value}' is outside supported range {}-{}",
            spec.min, spec.max
        ));
    }
    Ok(raw)
}
