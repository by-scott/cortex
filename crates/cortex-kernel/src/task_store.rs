use chrono::{DateTime, Utc};
use cortex_types::{Payload, SharedTask, SharedTaskStatus, TaskAssignment};
use rusqlite::{Connection, params};
use std::path::Path;
use std::sync::Mutex;

/// SQL schema for the task store tables.
const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS shared_tasks (
    id TEXT PRIMARY KEY,
    owner_actor TEXT NOT NULL DEFAULT 'local:default',
    parent_task_id TEXT,
    description TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'Pending',
    assigned_instance TEXT,
    priority INTEGER NOT NULL DEFAULT 5,
    result TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    deadline TEXT
);
CREATE INDEX IF NOT EXISTS idx_tasks_status ON shared_tasks(status);
CREATE INDEX IF NOT EXISTS idx_tasks_owner ON shared_tasks(owner_actor);
CREATE INDEX IF NOT EXISTS idx_tasks_parent ON shared_tasks(parent_task_id);
CREATE TABLE IF NOT EXISTS task_assignments (
    task_id TEXT NOT NULL,
    target_instance TEXT NOT NULL,
    assigned_at TEXT NOT NULL,
    deadline TEXT,
    PRIMARY KEY (task_id, target_instance)
);";

/// Persistent store for shared tasks backed by `SQLite`.
pub struct TaskStore {
    conn: Mutex<Connection>,
}

impl TaskStore {
    /// Open or create a task store at the given path.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the database cannot be opened or
    /// the schema cannot be initialised.
    pub fn open(path: &Path) -> Result<Self, TaskStoreError> {
        let conn =
            Connection::open(path).map_err(|e| TaskStoreError::Storage(format!("open: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    /// Create an in-memory task store (useful for testing).
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the in-memory database cannot be
    /// created.
    pub fn in_memory() -> Result<Self, TaskStoreError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| TaskStoreError::Storage(format!("open in-memory: {e}")))?;
        let store = Self {
            conn: Mutex::new(conn),
        };
        store.init_schema()?;
        Ok(store)
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, TaskStoreError> {
        self.conn
            .lock()
            .map_err(|e| TaskStoreError::Storage(format!("mutex: {e}")))
    }

    fn init_schema(&self) -> Result<(), TaskStoreError> {
        let conn = self.lock_conn()?;
        conn.execute_batch(SCHEMA)
            .map_err(|e| TaskStoreError::Storage(format!("init schema: {e}")))?;
        let _ = conn.execute(
            "ALTER TABLE shared_tasks ADD COLUMN owner_actor TEXT NOT NULL DEFAULT 'local:default'",
            [],
        );
        drop(conn);
        Ok(())
    }

    /// Save (insert or replace) a shared task.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the database write fails.
    pub fn save(&self, task: &SharedTask) -> Result<(), TaskStoreError> {
        self.lock_conn()?
            .execute(
                "INSERT OR REPLACE INTO shared_tasks \
                 (id, owner_actor, parent_task_id, description, status, assigned_instance, \
                  priority, result, created_at, updated_at, deadline) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    task.id,
                    task.owner_actor,
                    task.parent_task_id,
                    task.description,
                    format!("{}", task.status),
                    task.assigned_instance,
                    task.priority,
                    task.result,
                    task.created_at.to_rfc3339(),
                    task.updated_at.to_rfc3339(),
                    task.deadline.map(|d| d.to_rfc3339()),
                ],
            )
            .map_err(|e| TaskStoreError::Storage(format!("save: {e}")))?;
        Ok(())
    }

    /// Load a task by ID.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the task cannot be found or the
    /// query fails.
    pub fn load(&self, id: &str) -> Result<SharedTask, TaskStoreError> {
        self.lock_conn()?
            .query_row(
                "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
                 priority, result, created_at, updated_at, deadline \
                 FROM shared_tasks WHERE id = ?1",
                params![id],
                |row| {
                    row_to_task(row).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
                },
            )
            .map_err(|e| TaskStoreError::Storage(format!("load: {e}")))
    }

    /// Load a task only if it is visible to the actor.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the task cannot be found or is not visible.
    pub fn load_for_actor(&self, id: &str, actor: &str) -> Result<SharedTask, TaskStoreError> {
        let task = self.load(id)?;
        if actor == "local:default" || task.owner_actor == actor {
            Ok(task)
        } else {
            Err(TaskStoreError::Storage(format!(
                "load: task {id} not found"
            )))
        }
    }

    /// Delete a task by ID.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the database delete fails.
    pub fn delete(&self, id: &str) -> Result<bool, TaskStoreError> {
        let rows = self
            .lock_conn()?
            .execute("DELETE FROM shared_tasks WHERE id = ?1", params![id])
            .map_err(|e| TaskStoreError::Storage(format!("delete: {e}")))?;
        Ok(rows > 0)
    }

    /// Delete a task only if it is visible to the actor.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if loading or deleting fails.
    pub fn delete_for_actor(&self, id: &str, actor: &str) -> Result<bool, TaskStoreError> {
        let _ = self.load_for_actor(id, actor)?;
        self.delete(id)
    }

    /// List all tasks with a specific status, ordered by priority descending.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn list_by_status(
        &self,
        status: SharedTaskStatus,
    ) -> Result<Vec<SharedTask>, TaskStoreError> {
        let status_str = format!("{status}");
        let conn = self.lock_conn()?;
        query_tasks(
            &conn,
            "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
             priority, result, created_at, updated_at, deadline \
             FROM shared_tasks WHERE status = ?1 ORDER BY priority DESC",
            &[status_str.as_str()],
            "list_by_status",
        )
    }

    /// List tasks visible to an actor for a specific status.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn list_by_status_for_actor(
        &self,
        status: SharedTaskStatus,
        actor: &str,
    ) -> Result<Vec<SharedTask>, TaskStoreError> {
        if actor == "local:default" {
            return self.list_by_status(status);
        }
        let status_str = format!("{status}");
        let conn = self.lock_conn()?;
        query_tasks(
            &conn,
            "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
             priority, result, created_at, updated_at, deadline \
             FROM shared_tasks WHERE status = ?1 AND owner_actor = ?2 ORDER BY priority DESC",
            &[status_str.as_str(), actor],
            "list_by_status_for_actor",
        )
    }

    /// Get all sub-tasks of a parent task.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn sub_tasks(&self, parent_id: &str) -> Result<Vec<SharedTask>, TaskStoreError> {
        let conn = self.lock_conn()?;
        query_tasks(
            &conn,
            "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
             priority, result, created_at, updated_at, deadline \
             FROM shared_tasks WHERE parent_task_id = ?1",
            &[parent_id],
            "sub_tasks",
        )
    }

    /// Find all claimable tasks (Pending with no assigned instance), ordered by
    /// priority descending then `created_at` ascending.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn find_claimable(&self) -> Result<Vec<SharedTask>, TaskStoreError> {
        let conn = self.lock_conn()?;
        query_tasks(
            &conn,
            "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
             priority, result, created_at, updated_at, deadline \
             FROM shared_tasks \
             WHERE status = 'Pending' AND assigned_instance IS NULL \
             ORDER BY priority DESC, created_at ASC",
            &[],
            "find_claimable",
        )
    }

    /// Find claimable tasks visible to an actor.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn find_claimable_for_actor(&self, actor: &str) -> Result<Vec<SharedTask>, TaskStoreError> {
        if actor == "local:default" {
            return self.find_claimable();
        }
        let conn = self.lock_conn()?;
        query_tasks(
            &conn,
            "SELECT id, owner_actor, parent_task_id, description, status, assigned_instance, \
             priority, result, created_at, updated_at, deadline \
             FROM shared_tasks \
             WHERE status = 'Pending' AND assigned_instance IS NULL AND owner_actor = ?1 \
             ORDER BY priority DESC, created_at ASC",
            &[actor],
            "find_claimable_for_actor",
        )
    }

    /// Atomically claim a Pending task for an instance.
    ///
    /// Returns a `TaskAssignment` and a `TaskClaimed` event payload on success.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the task is not Pending or the
    /// transaction fails.
    pub fn claim(
        &self,
        task_id: &str,
        instance_id: &str,
    ) -> Result<(TaskAssignment, Payload), TaskStoreError> {
        {
            let conn = self.lock_conn()?;
            claim_task_inner(&conn, task_id, instance_id)?;
        }

        let assignment = TaskAssignment::new(task_id.to_string(), instance_id.to_string());
        let event = Payload::TaskClaimed {
            task_id: task_id.to_string(),
            instance_id: instance_id.to_string(),
        };

        Ok((assignment, event))
    }

    /// Claim a task only if it is visible to the actor.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the task is not visible or cannot be claimed.
    pub fn claim_for_actor(
        &self,
        task_id: &str,
        instance_id: &str,
        actor: &str,
    ) -> Result<(TaskAssignment, Payload), TaskStoreError> {
        let _ = self.load_for_actor(task_id, actor)?;
        self.claim(task_id, instance_id)
    }

    /// Save a task assignment record.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the database write fails.
    pub fn save_assignment(&self, assignment: &TaskAssignment) -> Result<(), TaskStoreError> {
        self.lock_conn()?
            .execute(
                "INSERT OR REPLACE INTO task_assignments \
                 (task_id, target_instance, assigned_at, deadline) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![
                    assignment.task_id,
                    assignment.target_instance,
                    assignment.assigned_at.to_rfc3339(),
                    assignment.deadline.map(|d| d.to_rfc3339()),
                ],
            )
            .map_err(|e| TaskStoreError::Storage(format!("save_assignment: {e}")))?;
        Ok(())
    }

    /// Get all assignments for a task.
    ///
    /// # Errors
    ///
    /// Returns `TaskStoreError::Storage` if the query fails.
    pub fn assignments_for(&self, task_id: &str) -> Result<Vec<TaskAssignment>, TaskStoreError> {
        let conn = self.lock_conn()?;
        query_assignments(&conn, task_id)
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Execute the transactional claim logic against the connection.
fn claim_task_inner(
    conn: &rusqlite::Connection,
    task_id: &str,
    instance_id: &str,
) -> Result<(), TaskStoreError> {
    let tx = conn
        .unchecked_transaction()
        .map_err(|e| TaskStoreError::Storage(format!("claim tx begin: {e}")))?;

    let status_str: String = tx
        .query_row(
            "SELECT status FROM shared_tasks WHERE id = ?1",
            params![task_id],
            |row| row.get(0),
        )
        .map_err(|e| TaskStoreError::Storage(format!("claim select: {e}")))?;

    if status_str != "Pending" {
        return Err(TaskStoreError::Storage(format!(
            "cannot claim task {task_id}: status is {status_str}, expected Pending"
        )));
    }

    let now = Utc::now();
    tx.execute(
        "UPDATE shared_tasks \
         SET status = 'Assigned', assigned_instance = ?1, updated_at = ?2 \
         WHERE id = ?3",
        params![instance_id, now.to_rfc3339(), task_id],
    )
    .map_err(|e| TaskStoreError::Storage(format!("claim update: {e}")))?;

    tx.execute(
        "INSERT OR REPLACE INTO task_assignments \
         (task_id, target_instance, assigned_at, deadline) \
         VALUES (?1, ?2, ?3, NULL)",
        params![task_id, instance_id, now.to_rfc3339()],
    )
    .map_err(|e| TaskStoreError::Storage(format!("claim assignment: {e}")))?;

    tx.commit()
        .map_err(|e| TaskStoreError::Storage(format!("claim commit: {e}")))
}

/// Query helper: run a parameterized task query and collect results.
fn query_tasks(
    conn: &rusqlite::Connection,
    sql: &str,
    params_slice: &[&str],
    label: &str,
) -> Result<Vec<SharedTask>, TaskStoreError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_slice
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| TaskStoreError::Storage(format!("{label} prepare: {e}")))?;
    stmt.query_map(params_refs.as_slice(), |row| {
        row_to_task(row).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
    })
    .map_err(|e| TaskStoreError::Storage(format!("{label} query: {e}")))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| TaskStoreError::Storage(format!("{label} row: {e}")))
}

/// Query helper: load task assignments for a task.
fn query_assignments(
    conn: &rusqlite::Connection,
    task_id: &str,
) -> Result<Vec<TaskAssignment>, TaskStoreError> {
    let mut stmt = conn
        .prepare(
            "SELECT task_id, target_instance, assigned_at, deadline \
             FROM task_assignments WHERE task_id = ?1",
        )
        .map_err(|e| TaskStoreError::Storage(format!("assignments_for prepare: {e}")))?;

    stmt.query_map(params![task_id], |row| {
        let tid: String = row.get(0)?;
        let target: String = row.get(1)?;
        let assigned_at_str: String = row.get(2)?;
        let deadline_str: Option<String> = row.get(3)?;

        let assigned_at = DateTime::parse_from_rfc3339(&assigned_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .map_err(|e| {
                rusqlite::Error::ToSqlConversionFailure(format!("bad assigned_at: {e}").into())
            })?;
        let deadline = deadline_str
            .map(|s| {
                DateTime::parse_from_rfc3339(&s)
                    .map(|dt| dt.with_timezone(&Utc))
                    .map_err(|e| {
                        rusqlite::Error::ToSqlConversionFailure(format!("bad deadline: {e}").into())
                    })
            })
            .transpose()?;

        Ok(TaskAssignment {
            task_id: tid,
            target_instance: target,
            assigned_at,
            deadline,
        })
    })
    .map_err(|e| TaskStoreError::Storage(format!("assignments_for query: {e}")))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| TaskStoreError::Storage(format!("assignments_for row: {e}")))
}

/// Parse a status string into a `SharedTaskStatus`.
///
/// # Errors
///
/// Returns `TaskStoreError::Storage` if the string does not match any known
/// status variant.
fn parse_status(s: &str) -> Result<SharedTaskStatus, TaskStoreError> {
    match s {
        "Pending" => Ok(SharedTaskStatus::Pending),
        "Assigned" => Ok(SharedTaskStatus::Assigned),
        "InProgress" => Ok(SharedTaskStatus::InProgress),
        "Completed" => Ok(SharedTaskStatus::Completed),
        "Failed" => Ok(SharedTaskStatus::Failed),
        "Cancelled" => Ok(SharedTaskStatus::Cancelled),
        other => Err(TaskStoreError::Storage(format!(
            "unknown task status: {other}"
        ))),
    }
}

/// Convert a database row into a `SharedTask`.
///
/// # Errors
///
/// Returns `TaskStoreError::Storage` if any column cannot be read or parsed.
fn row_to_task(row: &rusqlite::Row<'_>) -> Result<SharedTask, TaskStoreError> {
    let id: String = row
        .get(0)
        .map_err(|e| TaskStoreError::Storage(format!("row id: {e}")))?;
    let owner_actor: String = row
        .get(1)
        .map_err(|e| TaskStoreError::Storage(format!("row owner_actor: {e}")))?;
    let parent_task_id: Option<String> = row
        .get(2)
        .map_err(|e| TaskStoreError::Storage(format!("row parent_task_id: {e}")))?;
    let description: String = row
        .get(3)
        .map_err(|e| TaskStoreError::Storage(format!("row description: {e}")))?;
    let status_str: String = row
        .get(4)
        .map_err(|e| TaskStoreError::Storage(format!("row status: {e}")))?;
    let assigned_instance: Option<String> = row
        .get(5)
        .map_err(|e| TaskStoreError::Storage(format!("row assigned_instance: {e}")))?;
    let priority_i32: i32 = row
        .get(6)
        .map_err(|e| TaskStoreError::Storage(format!("row priority: {e}")))?;
    let result: Option<String> = row
        .get(7)
        .map_err(|e| TaskStoreError::Storage(format!("row result: {e}")))?;
    let created_at_str: String = row
        .get(8)
        .map_err(|e| TaskStoreError::Storage(format!("row created_at: {e}")))?;
    let updated_at_str: String = row
        .get(9)
        .map_err(|e| TaskStoreError::Storage(format!("row updated_at: {e}")))?;
    let deadline_str: Option<String> = row
        .get(10)
        .map_err(|e| TaskStoreError::Storage(format!("row deadline: {e}")))?;

    let status = parse_status(&status_str)?;

    let priority = u8::try_from(priority_i32)
        .map_err(|e| TaskStoreError::Storage(format!("bad priority: {e}")))?;

    let created_at = DateTime::parse_from_rfc3339(&created_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| TaskStoreError::Storage(format!("bad created_at: {e}")))?;
    let updated_at = DateTime::parse_from_rfc3339(&updated_at_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| TaskStoreError::Storage(format!("bad updated_at: {e}")))?;
    let deadline = deadline_str
        .map(|s| {
            DateTime::parse_from_rfc3339(&s)
                .map(|dt| dt.with_timezone(&Utc))
                .map_err(|e| TaskStoreError::Storage(format!("bad deadline: {e}")))
        })
        .transpose()?;

    Ok(SharedTask {
        id,
        owner_actor,
        parent_task_id,
        description,
        status,
        assigned_instance,
        priority,
        result,
        created_at,
        updated_at,
        deadline,
    })
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors produced by [`TaskStore`] operations.
#[derive(Debug)]
pub enum TaskStoreError {
    /// A storage-level failure (database I/O, schema, parse, etc.).
    Storage(String),
}

impl std::fmt::Display for TaskStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "task store error: {e}"),
        }
    }
}

impl std::error::Error for TaskStoreError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_store() -> TaskStore {
        TaskStore::in_memory().unwrap()
    }

    #[test]
    fn save_and_load() {
        let store = test_store();
        let task = SharedTask::new("Test task".to_owned());
        store.save(&task).unwrap();

        let loaded = store.load(&task.id).unwrap();
        assert_eq!(loaded.id, task.id);
        assert_eq!(loaded.description, "Test task");
        assert_eq!(loaded.status, SharedTaskStatus::Pending);
    }

    #[test]
    fn delete_task() {
        let store = test_store();
        let task = SharedTask::new("Delete me".to_owned());
        store.save(&task).unwrap();
        assert!(store.delete(&task.id).unwrap());
        assert!(store.load(&task.id).is_err());
    }

    #[test]
    fn list_by_status() {
        let store = test_store();
        let t1 = SharedTask::new("A".to_owned());
        let mut t2 = SharedTask::new("B".to_owned());
        t2.status = SharedTaskStatus::Completed;
        store.save(&t1).unwrap();
        store.save(&t2).unwrap();

        let pending = store.list_by_status(SharedTaskStatus::Pending).unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].description, "A");

        let completed = store.list_by_status(SharedTaskStatus::Completed).unwrap();
        assert_eq!(completed.len(), 1);
        assert_eq!(completed[0].description, "B");
    }

    #[test]
    fn actor_scoped_task_apis_filter_and_protect_mutations() {
        let store = test_store();
        let mut own = SharedTask::new("own".to_owned());
        own.owner_actor = "telegram:1".into();
        let mut other = SharedTask::new("other".to_owned());
        other.owner_actor = "telegram:2".into();
        store.save(&own).unwrap();
        store.save(&other).unwrap();

        let visible = store
            .list_by_status_for_actor(SharedTaskStatus::Pending, "telegram:1")
            .unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].id, own.id);
        assert!(store.load_for_actor(&own.id, "telegram:1").is_ok());
        assert!(store.load_for_actor(&other.id, "telegram:1").is_err());
        assert!(
            store
                .claim_for_actor(&other.id, "worker-01", "telegram:1")
                .is_err()
        );
        let claimable = store.find_claimable_for_actor("telegram:1").unwrap();
        assert_eq!(claimable.len(), 1);
        store
            .claim_for_actor(&own.id, "worker-01", "telegram:1")
            .unwrap();
        assert!(store.delete_for_actor(&other.id, "telegram:1").is_err());
        assert!(store.delete_for_actor(&other.id, "local:default").unwrap());
    }

    #[test]
    fn sub_tasks_query() {
        let store = test_store();
        let parent = SharedTask::new("Parent".to_owned());
        let child1 = SharedTask::new("Child 1".to_owned()).with_parent(parent.id.clone());
        let child2 = SharedTask::new("Child 2".to_owned()).with_parent(parent.id.clone());
        store.save(&parent).unwrap();
        store.save(&child1).unwrap();
        store.save(&child2).unwrap();

        let subs = store.sub_tasks(&parent.id).unwrap();
        assert_eq!(subs.len(), 2);
    }

    #[test]
    fn find_claimable() {
        let store = test_store();
        let t1 = SharedTask::new("Claimable".to_owned()).with_priority(10);
        let mut t2 = SharedTask::new("Already assigned".to_owned());
        t2.assigned_instance = Some("worker-01".into());
        let mut t3 = SharedTask::new("Completed".to_owned());
        t3.status = SharedTaskStatus::Completed;
        store.save(&t1).unwrap();
        store.save(&t2).unwrap();
        store.save(&t3).unwrap();

        let claimable = store.find_claimable().unwrap();
        assert_eq!(claimable.len(), 1);
        assert_eq!(claimable[0].description, "Claimable");
    }

    #[test]
    fn claim_pending_task() {
        let store = test_store();
        let task = SharedTask::new("Claim me".to_owned());
        store.save(&task).unwrap();

        let (assignment, event) = store.claim(&task.id, "worker-01").unwrap();
        assert_eq!(assignment.target_instance, "worker-01");
        assert!(matches!(event, Payload::TaskClaimed { .. }));

        let loaded = store.load(&task.id).unwrap();
        assert_eq!(loaded.status, SharedTaskStatus::Assigned);
        assert_eq!(loaded.assigned_instance.as_deref(), Some("worker-01"));
    }

    #[test]
    fn claim_non_pending_fails() {
        let store = test_store();
        let mut task = SharedTask::new("Not pending".to_owned());
        task.status = SharedTaskStatus::Completed;
        store.save(&task).unwrap();

        assert!(store.claim(&task.id, "worker-01").is_err());
    }

    #[test]
    fn claim_race_second_fails() {
        let store = test_store();
        let task = SharedTask::new("Race".to_owned());
        store.save(&task).unwrap();

        store.claim(&task.id, "worker-01").unwrap();
        assert!(store.claim(&task.id, "worker-02").is_err());
    }

    #[test]
    fn save_and_query_assignments() {
        let store = test_store();
        let assignment = TaskAssignment::new("task-1".to_owned(), "worker-01".to_owned());
        store.save_assignment(&assignment).unwrap();

        let loaded = store.assignments_for("task-1").unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].target_instance, "worker-01");
    }

    #[test]
    fn priority_ordering_in_claimable() {
        let store = test_store();
        let t_low = SharedTask::new("Low priority".to_owned()).with_priority(1);
        let t_high = SharedTask::new("High priority".to_owned()).with_priority(10);
        store.save(&t_low).unwrap();
        store.save(&t_high).unwrap();

        let claimable = store.find_claimable().unwrap();
        assert_eq!(claimable.len(), 2);
        assert_eq!(claimable[0].description, "High priority");
    }
}
