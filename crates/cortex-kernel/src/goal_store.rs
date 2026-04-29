use chrono::{DateTime, Utc};
use cortex_types::{Goal, GoalLevel, GoalSource, GoalStatus};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

const BASE_SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS goals (
    id TEXT PRIMARY KEY,
    owner_actor TEXT NOT NULL DEFAULT 'local:default',
    parent_goal_id TEXT,
    linked_task_id TEXT,
    level TEXT NOT NULL,
    description TEXT NOT NULL,
    success_criteria TEXT NOT NULL DEFAULT '',
    source TEXT NOT NULL,
    status TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 5,
    evidence_refs TEXT NOT NULL DEFAULT '[]',
    memory_refs TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deadline TEXT,
    completed_at TEXT
);";

const INDEX_SCHEMA: &str = "
CREATE INDEX IF NOT EXISTS idx_goals_owner ON goals(owner_actor);
CREATE INDEX IF NOT EXISTS idx_goals_status ON goals(status);
CREATE INDEX IF NOT EXISTS idx_goals_level ON goals(level);
CREATE INDEX IF NOT EXISTS idx_goals_parent ON goals(parent_goal_id);
CREATE INDEX IF NOT EXISTS idx_goals_task ON goals(linked_task_id);";

pub struct GoalStore {
    conn: Mutex<Connection>,
}

impl GoalStore {
    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the database cannot be opened or initialized.
    pub fn open(path: &Path) -> Result<Self, GoalStoreError> {
        let conn = Connection::open(path)
            .map_err(|err| GoalStoreError::Storage(format!("open: {err}")))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the in-memory database cannot be initialized.
    pub fn in_memory() -> Result<Self, GoalStoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|err| GoalStoreError::Storage(format!("open in-memory: {err}")))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, GoalStoreError> {
        self.conn
            .lock()
            .map_err(|err| GoalStoreError::Storage(format!("mutex: {err}")))
    }

    fn init_schema(&self) -> Result<(), GoalStoreError> {
        let conn = self.lock_conn()?;
        conn.execute_batch(BASE_SCHEMA)
            .map_err(|err| GoalStoreError::Storage(format!("init schema: {err}")))?;
        conn.execute_batch(INDEX_SCHEMA)
            .map_err(|err| GoalStoreError::Storage(format!("init indexes: {err}")))?;
        drop(conn);
        Ok(())
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the database write fails.
    pub fn save(&self, goal: &Goal) -> Result<(), GoalStoreError> {
        let evidence_refs = encode_refs(&goal.evidence_refs)?;
        let memory_refs = encode_refs(&goal.memory_refs)?;
        self.lock_conn()?
            .execute(
                "INSERT OR REPLACE INTO goals \
                 (id, owner_actor, parent_goal_id, linked_task_id, level, description, \
                  success_criteria, source, status, priority, evidence_refs, memory_refs, \
                  created_at, updated_at, deadline, completed_at) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
                params![
                    goal.id,
                    goal.owner_actor,
                    goal.parent_goal_id,
                    goal.linked_task_id,
                    goal.level.to_string(),
                    goal.description,
                    goal.success_criteria,
                    goal.source.to_string(),
                    goal.status.to_string(),
                    goal.priority,
                    evidence_refs,
                    memory_refs,
                    goal.created_at.to_rfc3339(),
                    goal.updated_at.to_rfc3339(),
                    goal.deadline.map(|deadline| deadline.to_rfc3339()),
                    goal.completed_at
                        .map(|completed_at| completed_at.to_rfc3339()),
                ],
            )
            .map_err(|err| GoalStoreError::Storage(format!("save: {err}")))?;
        Ok(())
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the goal cannot be found or decoded.
    pub fn load(&self, id: &str) -> Result<Goal, GoalStoreError> {
        self.lock_conn()?
            .query_row(
                goal_select_sql("WHERE id = ?1").as_str(),
                params![id],
                row_to_goal,
            )
            .map_err(|err| GoalStoreError::Storage(format!("load: {err}")))
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the goal is hidden, missing, or cannot be decoded.
    pub fn load_for_actor(&self, id: &str, actor: &str) -> Result<Goal, GoalStoreError> {
        let goal = self.load(id)?;
        if actor == "local:default" || goal.owner_actor == actor {
            Ok(goal)
        } else {
            Err(GoalStoreError::Storage(format!(
                "load: goal {id} not found"
            )))
        }
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the query fails.
    pub fn list_for_actor(&self, actor: &str) -> Result<Vec<Goal>, GoalStoreError> {
        let conn = self.lock_conn()?;
        if actor == "local:default" {
            return query_goals(
                &conn,
                goal_select_sql("ORDER BY priority DESC, created_at ASC").as_str(),
                &[],
                "list_for_actor_all",
            );
        }
        query_goals(
            &conn,
            goal_select_sql("WHERE owner_actor = ?1 ORDER BY priority DESC, created_at ASC")
                .as_str(),
            &[actor],
            "list_for_actor",
        )
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the query fails.
    pub fn list_open_for_actor(&self, actor: &str) -> Result<Vec<Goal>, GoalStoreError> {
        self.list_for_actor(actor).map(|goals| {
            goals
                .into_iter()
                .filter(|goal| goal.status.is_open())
                .collect()
        })
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the query fails.
    pub fn list_by_status_for_actor(
        &self,
        status: GoalStatus,
        actor: &str,
    ) -> Result<Vec<Goal>, GoalStoreError> {
        let status = status.to_string();
        let conn = self.lock_conn()?;
        if actor == "local:default" {
            return query_goals(
                &conn,
                goal_select_sql("WHERE status = ?1 ORDER BY priority DESC, created_at ASC")
                    .as_str(),
                &[status.as_str()],
                "list_by_status_all",
            );
        }
        query_goals(
            &conn,
            goal_select_sql(
                "WHERE status = ?1 AND owner_actor = ?2 ORDER BY priority DESC, created_at ASC",
            )
            .as_str(),
            &[status.as_str(), actor],
            "list_by_status_for_actor",
        )
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if loading or deleting fails.
    pub fn delete_for_actor(&self, id: &str, actor: &str) -> Result<bool, GoalStoreError> {
        let _ = self.load_for_actor(id, actor)?;
        let rows = self
            .lock_conn()?
            .execute("DELETE FROM goals WHERE id = ?1", params![id])
            .map_err(|err| GoalStoreError::Storage(format!("delete: {err}")))?;
        Ok(rows > 0)
    }

    /// # Errors
    ///
    /// Returns `GoalStoreError::Storage` if the goal is hidden, missing, or the transition is invalid.
    pub fn update_status_for_actor(
        &self,
        id: &str,
        actor: &str,
        status: GoalStatus,
    ) -> Result<Goal, GoalStoreError> {
        let mut goal = self.load_for_actor(id, actor)?;
        if goal.status != status {
            goal.status
                .try_transition(status)
                .map_err(|err| GoalStoreError::Storage(err.to_string()))?;
        }
        goal.status = status;
        goal.updated_at = Utc::now();
        goal.completed_at = if status.is_terminal() {
            Some(goal.updated_at)
        } else {
            None
        };
        self.save(&goal)?;
        Ok(goal)
    }
}

fn goal_select_sql(tail: &str) -> String {
    format!(
        "SELECT id, owner_actor, parent_goal_id, linked_task_id, level, description, \
         success_criteria, source, status, priority, evidence_refs, memory_refs, \
         created_at, updated_at, deadline, completed_at FROM goals {tail}"
    )
}

fn query_goals(
    conn: &Connection,
    sql: &str,
    params_slice: &[&str],
    label: &str,
) -> Result<Vec<Goal>, GoalStoreError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_slice
        .iter()
        .map(|value| value as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|err| GoalStoreError::Storage(format!("{label} prepare: {err}")))?;
    stmt.query_map(params_refs.as_slice(), row_to_goal)
        .map_err(|err| GoalStoreError::Storage(format!("{label} query: {err}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|err| GoalStoreError::Storage(format!("{label} row: {err}")))
}

fn encode_refs(values: &[String]) -> Result<String, GoalStoreError> {
    serde_json::to_string(values).map_err(|err| GoalStoreError::Storage(format!("refs: {err}")))
}

fn decode_refs(value: &str) -> Result<Vec<String>, GoalStoreError> {
    serde_json::from_str(value).map_err(|err| GoalStoreError::Storage(format!("refs: {err}")))
}

fn parse_goal_level(value: &str) -> Result<GoalLevel, GoalStoreError> {
    match value {
        "Strategic" => Ok(GoalLevel::Strategic),
        "Tactical" => Ok(GoalLevel::Tactical),
        "Immediate" => Ok(GoalLevel::Immediate),
        other => Err(GoalStoreError::Storage(format!(
            "unknown goal level: {other}"
        ))),
    }
}

fn parse_goal_status(value: &str) -> Result<GoalStatus, GoalStoreError> {
    match value {
        "Proposed" => Ok(GoalStatus::Proposed),
        "Active" => Ok(GoalStatus::Active),
        "Blocked" => Ok(GoalStatus::Blocked),
        "Completed" => Ok(GoalStatus::Completed),
        "Abandoned" => Ok(GoalStatus::Abandoned),
        other => Err(GoalStoreError::Storage(format!(
            "unknown goal status: {other}"
        ))),
    }
}

fn parse_goal_source(value: &str) -> Result<GoalSource, GoalStoreError> {
    match value {
        "User" => Ok(GoalSource::User),
        "Operator" => Ok(GoalSource::Operator),
        "Runtime" => Ok(GoalSource::Runtime),
        "Memory" => Ok(GoalSource::Memory),
        "Imported" => Ok(GoalSource::Imported),
        other => Err(GoalStoreError::Storage(format!(
            "unknown goal source: {other}"
        ))),
    }
}

fn parse_time(value: &str, label: &str) -> Result<DateTime<Utc>, GoalStoreError> {
    DateTime::parse_from_rfc3339(value)
        .map(|time| time.with_timezone(&Utc))
        .map_err(|err| GoalStoreError::Storage(format!("bad {label}: {err}")))
}

fn parse_optional_time(
    value: Option<String>,
    label: &str,
) -> Result<Option<DateTime<Utc>>, GoalStoreError> {
    value.map_or(Ok(None), |time| parse_time(&time, label).map(Some))
}

fn row_to_goal(row: &rusqlite::Row<'_>) -> rusqlite::Result<Goal> {
    row_to_goal_inner(row).map_err(|err| rusqlite::Error::ToSqlConversionFailure(err.into()))
}

fn row_to_goal_inner(row: &rusqlite::Row<'_>) -> Result<Goal, GoalStoreError> {
    let level: String = row.get(4).map_err(row_err("level"))?;
    let source: String = row.get(7).map_err(row_err("source"))?;
    let status: String = row.get(8).map_err(row_err("status"))?;
    let priority: i32 = row.get(9).map_err(row_err("priority"))?;
    let evidence_refs: String = row.get(10).map_err(row_err("evidence_refs"))?;
    let memory_refs: String = row.get(11).map_err(row_err("memory_refs"))?;
    let created_at: String = row.get(12).map_err(row_err("created_at"))?;
    let updated_at: String = row.get(13).map_err(row_err("updated_at"))?;
    let deadline: Option<String> = row.get(14).map_err(row_err("deadline"))?;
    let completed_at: Option<String> = row.get(15).map_err(row_err("completed_at"))?;
    Ok(Goal {
        id: row.get(0).map_err(row_err("id"))?,
        owner_actor: row.get(1).map_err(row_err("owner_actor"))?,
        parent_goal_id: row.get(2).map_err(row_err("parent_goal_id"))?,
        linked_task_id: row.get(3).map_err(row_err("linked_task_id"))?,
        level: parse_goal_level(&level)?,
        description: row.get(5).map_err(row_err("description"))?,
        success_criteria: row.get(6).map_err(row_err("success_criteria"))?,
        source: parse_goal_source(&source)?,
        status: parse_goal_status(&status)?,
        priority: u8::try_from(priority)
            .map_err(|err| GoalStoreError::Storage(format!("bad priority: {err}")))?,
        evidence_refs: decode_refs(&evidence_refs)?,
        memory_refs: decode_refs(&memory_refs)?,
        created_at: parse_time(&created_at, "created_at")?,
        updated_at: parse_time(&updated_at, "updated_at")?,
        deadline: parse_optional_time(deadline, "deadline")?,
        completed_at: parse_optional_time(completed_at, "completed_at")?,
    })
}

fn row_err(label: &'static str) -> impl FnOnce(rusqlite::Error) -> GoalStoreError {
    move |err| GoalStoreError::Storage(format!("row {label}: {err}"))
}

#[derive(Debug)]
pub enum GoalStoreError {
    Storage(String),
}

impl std::fmt::Display for GoalStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(message) => write!(f, "goal store error: {message}"),
        }
    }
}

impl std::error::Error for GoalStoreError {}
