//! Cron tool and queue infrastructure for scheduled task execution.
//!
//! The `CronTool` allows the LLM to schedule recurring or one-shot
//! tasks.  The `CronQueue` is runtime infrastructure checked by the
//! heartbeat engine each tick.

use std::path::{Path, PathBuf};
use std::sync::Arc;

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
        "Schedule recurring or one-shot tasks.\n\n\
         Use to set up periodic actions: memory consolidation reminders, \
         status checks, recurring reports, or deferred one-shot tasks.\n\n\
         Actions:\n\
         - create: schedule a new task with a cron expression and prompt\n\
         - list: show all scheduled tasks\n\
         - cancel: remove a scheduled task by ID\n\n\
         Cron expressions use standard 5-field format: minute hour day-of-month \
         month day-of-week (e.g. \"*/5 * * * *\" = every 5 minutes, \
         \"0 9 * * 1-5\" = weekdays at 9am).\n\n\
         Scheduled tasks execute during idle periods via the heartbeat engine. \
         They persist across daemon restarts."
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "cancel"],
                    "description": "Operation to perform."
                },
                "cron": {
                    "type": "string",
                    "description": "Standard 5-field cron expression. Required for 'create'."
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to execute on each trigger. Required for 'create'."
                },
                "task_id": {
                    "type": "string",
                    "description": "Task ID to cancel. Required for 'cancel'."
                },
                "recurring": {
                    "type": "boolean",
                    "default": true,
                    "description": "true = recurring on schedule, false = fire once then delete."
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

                let id = self.queue.add(cron_expr, prompt, recurring);
                let kind = if recurring { "recurring" } else { "one-shot" };
                Ok(ToolResult::success(format!(
                    "Scheduled {kind} task {id} with cron=\"{cron_expr}\"."
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
                    let _ = write!(
                        out,
                        "\n  - {} [{kind}] cron=\"{}\" prompt=\"{}\"",
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
}

// ── CronQueue (runtime infrastructure) ─────────────────────────

/// Persistent queue of scheduled cron tasks.
///
/// The heartbeat engine checks this each tick for due tasks.
/// Tasks are added via the cron tool and persisted to disk.
pub struct CronQueue {
    path: PathBuf,
}

impl CronQueue {
    /// Open (or create) the cron queue at `data_dir/cron_queue.json`.
    #[must_use]
    pub fn open(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("cron_queue.json"),
        }
    }

    /// Add a new scheduled task. Returns the generated task ID.
    #[must_use]
    pub fn add(&self, cron_expr: &str, prompt: &str, recurring: bool) -> String {
        let id = uuid::Uuid::now_v7().to_string();
        let entry = CronEntry {
            id: id.clone(),
            cron_expr: cron_expr.into(),
            prompt: prompt.into(),
            recurring,
            created_at: chrono::Utc::now().to_rfc3339(),
            last_run: None,
        };
        let mut entries = self.load();
        entries.push(entry);
        self.save(&entries);
        id
    }

    /// List all scheduled tasks.
    #[must_use]
    pub fn list(&self) -> Vec<CronEntry> {
        self.load()
    }

    /// Cancel a task by ID. Returns `true` if the task was found and removed.
    #[must_use]
    pub fn cancel(&self, task_id: &str) -> bool {
        let mut entries = self.load();
        let before = entries.len();
        entries.retain(|e| e.id != task_id);
        if entries.len() < before {
            self.save(&entries);
            true
        } else {
            false
        }
    }

    /// Collect all due prompts, removing one-shot tasks and updating
    /// `last_run` for recurring ones.
    ///
    /// Returns an empty vec if no tasks are due or the queue file
    /// doesn't exist yet.
    #[must_use]
    pub fn collect_due(&self) -> Vec<String> {
        let entries = self.load();
        let now = chrono::Utc::now();
        let mut due = Vec::new();
        let mut remaining = Vec::new();

        for mut entry in entries {
            if is_due(&entry.cron_expr, &now, entry.last_run.as_deref()) {
                due.push(entry.prompt.clone());
                if entry.recurring {
                    entry.last_run = Some(now.to_rfc3339());
                    remaining.push(entry);
                }
                // one-shot: don't add back
            } else {
                remaining.push(entry);
            }
        }

        if !due.is_empty() {
            self.save(&remaining);
        }
        due
    }

    fn load(&self) -> Vec<CronEntry> {
        std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    fn save(&self, entries: &[CronEntry]) {
        if let Ok(json) = serde_json::to_string_pretty(entries) {
            let _ = std::fs::write(&self.path, json);
        }
    }
}

// ── Cron expression matching ───────────────────────────────────

/// Check if the current time matches a 5-field cron expression and
/// was not already run in the current minute.
fn is_due(cron_expr: &str, now: &chrono::DateTime<chrono::Utc>, last_run: Option<&str>) -> bool {
    use chrono::Datelike;
    use chrono::Timelike;

    // Don't fire twice in the same minute.
    if let Some(lr) = last_run
        && let Ok(last) = lr.parse::<chrono::DateTime<chrono::Utc>>()
        && last.date_naive() == now.date_naive()
        && last.hour() == now.hour()
        && last.minute() == now.minute()
    {
        return false;
    }

    let fields: Vec<&str> = cron_expr.split_whitespace().collect();
    if fields.len() != 5 {
        return false;
    }

    let minute = now.minute();
    let hour = now.hour();
    let dom = now.day();
    let month = now.month();
    let dow = now.weekday().num_days_from_sunday(); // 0=Sun

    field_matches(fields[0], minute)
        && field_matches(fields[1], hour)
        && field_matches(fields[2], dom)
        && field_matches(fields[3], month)
        && field_matches(fields[4], dow)
}

/// Check if a single cron field (e.g. `"*/5"`, `"1,3,5"`, `"10-20"`, `"*"`)
/// matches a numeric value.
fn field_matches(field: &str, value: u32) -> bool {
    if field == "*" {
        return true;
    }

    // Comma-separated list: "1,3,5"
    for part in field.split(',') {
        if part_matches(part.trim(), value) {
            return true;
        }
    }
    false
}

/// Match a single part (no commas): `"*"`, `"5"`, `"1-5"`, `"*/10"`, `"1-30/5"`.
fn part_matches(part: &str, value: u32) -> bool {
    if let Some(step_str) = part.strip_prefix("*/") {
        // */N
        if let Ok(step) = step_str.parse::<u32>() {
            return step > 0 && value.is_multiple_of(step);
        }
        return false;
    }

    if part.contains('/') {
        // range/step  e.g. "1-30/5"
        let mut split = part.splitn(2, '/');
        let range_part = split.next().unwrap_or("*");
        let step: u32 = split.next().and_then(|s| s.parse().ok()).unwrap_or(1);
        if step == 0 {
            return false;
        }
        if let Some((lo, hi)) = parse_range(range_part) {
            return value >= lo && value <= hi && (value - lo).is_multiple_of(step);
        }
        return false;
    }

    if let Some((lo, hi)) = parse_range(part) {
        return value >= lo && value <= hi;
    }

    // Plain number
    part.parse::<u32>().is_ok_and(|n| n == value)
}

fn parse_range(s: &str) -> Option<(u32, u32)> {
    let mut split = s.splitn(2, '-');
    let lo = split.next()?.parse().ok()?;
    let hi = split.next()?.parse().ok()?;
    Some((lo, hi))
}
