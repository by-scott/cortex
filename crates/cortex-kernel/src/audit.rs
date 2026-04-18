use chrono::{DateTime, Utc};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Mutex;

/// SQL schema for the audit log table and indices.
const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA synchronous = NORMAL;
CREATE TABLE IF NOT EXISTS audit_entries (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp TEXT NOT NULL,
    session_id TEXT NOT NULL,
    event_type TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    action TEXT NOT NULL,
    outcome TEXT NOT NULL,
    details TEXT
);
CREATE INDEX IF NOT EXISTS idx_audit_session ON audit_entries(session_id);
CREATE INDEX IF NOT EXISTS idx_audit_type ON audit_entries(event_type);
CREATE INDEX IF NOT EXISTS idx_audit_time ON audit_entries(timestamp);";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// Classification of audit events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AuditEventType {
    /// A tool was executed.
    ToolExecution,
    /// A permission decision was made.
    PermissionDecision,
}

impl AuditEventType {
    /// Return the canonical string representation.
    #[must_use]
    pub const fn as_str(&self) -> &'static str {
        match self {
            Self::ToolExecution => "tool_execution",
            Self::PermissionDecision => "permission_decision",
        }
    }

    /// Parse a string into an `AuditEventType`.
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` if the string does not match any known
    /// variant.
    pub fn parse(s: &str) -> Result<Self, AuditError> {
        match s {
            "tool_execution" => Ok(Self::ToolExecution),
            "permission_decision" => Ok(Self::PermissionDecision),
            other => Err(AuditError::Storage(format!(
                "unknown audit event type: {other}"
            ))),
        }
    }
}

/// A single audit log entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEntry {
    /// Row id (`None` for entries that have not been persisted yet).
    pub id: Option<i64>,
    /// When the event occurred.
    pub timestamp: DateTime<Utc>,
    /// Session that produced the event.
    pub session_id: String,
    /// Kind of event.
    pub event_type: AuditEventType,
    /// Name of the tool involved.
    pub tool_name: String,
    /// Human-readable action description.
    pub action: String,
    /// Outcome of the action.
    pub outcome: String,
    /// Optional free-form details.
    pub details: Option<String>,
}

impl AuditEntry {
    /// Create a tool-execution audit entry.
    #[must_use]
    pub fn tool_execution(
        session_id: impl Into<String>,
        tool_name: impl Into<String>,
        action: impl Into<String>,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            timestamp: Utc::now(),
            session_id: session_id.into(),
            event_type: AuditEventType::ToolExecution,
            tool_name: tool_name.into(),
            action: action.into(),
            outcome: outcome.into(),
            details: None,
        }
    }

    /// Create a permission-decision audit entry.
    #[must_use]
    pub fn permission_decision(
        session_id: impl Into<String>,
        tool_name: impl Into<String>,
        action: impl Into<String>,
        outcome: impl Into<String>,
    ) -> Self {
        Self {
            id: None,
            timestamp: Utc::now(),
            session_id: session_id.into(),
            event_type: AuditEventType::PermissionDecision,
            tool_name: tool_name.into(),
            action: action.into(),
            outcome: outcome.into(),
            details: None,
        }
    }

    /// Attach optional details to an entry.
    #[must_use]
    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// Append-only audit log backed by `SQLite`.
pub struct AuditLog {
    conn: Mutex<Connection>,
}

impl AuditLog {
    /// Open (or create) an audit log backed by a `SQLite` file.
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` if the database cannot be opened or
    /// the schema cannot be initialised.
    pub fn open(path: &Path) -> Result<Self, AuditError> {
        let conn = Connection::open(path).map_err(|e| AuditError::Storage(format!("open: {e}")))?;
        let log = Self {
            conn: Mutex::new(conn),
        };
        log.init_schema()?;
        Ok(log)
    }

    /// Create an in-memory audit log (useful for testing).
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` if the in-memory database cannot be
    /// created.
    pub fn in_memory() -> Result<Self, AuditError> {
        let conn = Connection::open_in_memory()
            .map_err(|e| AuditError::Storage(format!("open in-memory: {e}")))?;
        let log = Self {
            conn: Mutex::new(conn),
        };
        log.init_schema()?;
        Ok(log)
    }

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, AuditError> {
        self.conn
            .lock()
            .map_err(|e| AuditError::Storage(format!("mutex: {e}")))
    }

    fn init_schema(&self) -> Result<(), AuditError> {
        self.lock_conn()?
            .execute_batch(SCHEMA)
            .map_err(|e| AuditError::Storage(format!("init schema: {e}")))?;
        Ok(())
    }

    /// Append an audit entry and return its row id.
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` on database write failure.
    pub fn append(&self, entry: &AuditEntry) -> Result<i64, AuditError> {
        let ts = entry.timestamp.to_rfc3339();
        let id = {
            let conn = self.lock_conn()?;
            conn.execute(
                "INSERT INTO audit_entries \
                 (timestamp, session_id, event_type, tool_name, action, outcome, details) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    ts,
                    entry.session_id,
                    entry.event_type.as_str(),
                    entry.tool_name,
                    entry.action,
                    entry.outcome,
                    entry.details,
                ],
            )
            .map_err(|e| AuditError::Storage(format!("append: {e}")))?;
            conn.last_insert_rowid()
        };
        Ok(id)
    }

    /// Query audit entries by session id.
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` on database query failure.
    pub fn query_by_session(&self, session_id: &str) -> Result<Vec<AuditEntry>, AuditError> {
        self.run_query(
            "SELECT id, timestamp, session_id, event_type, tool_name, action, outcome, details \
             FROM audit_entries WHERE session_id = ?1 ORDER BY timestamp ASC",
            &[session_id],
        )
    }

    /// Query audit entries by event type.
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` on database query failure.
    pub fn query_by_type(
        &self,
        event_type: &AuditEventType,
    ) -> Result<Vec<AuditEntry>, AuditError> {
        self.run_query(
            "SELECT id, timestamp, session_id, event_type, tool_name, action, outcome, details \
             FROM audit_entries WHERE event_type = ?1 ORDER BY timestamp ASC",
            &[event_type.as_str()],
        )
    }

    /// Query audit entries within a time range (inclusive).
    ///
    /// # Errors
    ///
    /// Returns `AuditError::Storage` on database query failure.
    pub fn query_by_range(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
    ) -> Result<Vec<AuditEntry>, AuditError> {
        let start_str = start.to_rfc3339();
        let end_str = end.to_rfc3339();
        self.run_query(
            "SELECT id, timestamp, session_id, event_type, tool_name, action, outcome, details \
             FROM audit_entries \
             WHERE timestamp >= ?1 AND timestamp <= ?2 \
             ORDER BY timestamp ASC",
            &[start_str.as_str(), end_str.as_str()],
        )
    }

    /// Execute a parameterized audit query and collect results.
    fn run_query(&self, sql: &str, params_slice: &[&str]) -> Result<Vec<AuditEntry>, AuditError> {
        let conn = self.lock_conn()?;
        query_audit_entries(&conn, sql, params_slice)
    }
}

/// Execute a parameterized audit query against a connection.
fn query_audit_entries(
    conn: &rusqlite::Connection,
    sql: &str,
    params_slice: &[&str],
) -> Result<Vec<AuditEntry>, AuditError> {
    let params_refs: Vec<&dyn rusqlite::types::ToSql> = params_slice
        .iter()
        .map(|s| s as &dyn rusqlite::types::ToSql)
        .collect();
    let mut stmt = conn
        .prepare(sql)
        .map_err(|e| AuditError::Storage(format!("query prep: {e}")))?;
    stmt.query_map(params_refs.as_slice(), |row| {
        row_to_entry(row).map_err(|e| rusqlite::Error::ToSqlConversionFailure(e.into()))
    })
    .map_err(|e| AuditError::Storage(format!("query: {e}")))?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| AuditError::Storage(format!("query row: {e}")))
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert a database row into an `AuditEntry`.
///
/// # Errors
///
/// Returns `AuditError::Storage` if any column cannot be read or parsed.
fn row_to_entry(row: &rusqlite::Row<'_>) -> Result<AuditEntry, AuditError> {
    let id: i64 = row
        .get(0)
        .map_err(|e| AuditError::Storage(format!("row id: {e}")))?;
    let ts_str: String = row
        .get(1)
        .map_err(|e| AuditError::Storage(format!("row timestamp: {e}")))?;
    let session_id: String = row
        .get(2)
        .map_err(|e| AuditError::Storage(format!("row session_id: {e}")))?;
    let event_type_str: String = row
        .get(3)
        .map_err(|e| AuditError::Storage(format!("row event_type: {e}")))?;
    let tool_name: String = row
        .get(4)
        .map_err(|e| AuditError::Storage(format!("row tool_name: {e}")))?;
    let action: String = row
        .get(5)
        .map_err(|e| AuditError::Storage(format!("row action: {e}")))?;
    let outcome: String = row
        .get(6)
        .map_err(|e| AuditError::Storage(format!("row outcome: {e}")))?;
    let details: Option<String> = row
        .get(7)
        .map_err(|e| AuditError::Storage(format!("row details: {e}")))?;

    let timestamp = DateTime::parse_from_rfc3339(&ts_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| AuditError::Storage(format!("bad timestamp: {e}")))?;

    let event_type = AuditEventType::parse(&event_type_str)?;

    Ok(AuditEntry {
        id: Some(id),
        timestamp,
        session_id,
        event_type,
        tool_name,
        action,
        outcome,
        details,
    })
}

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors produced by [`AuditLog`] operations.
#[derive(Debug)]
pub enum AuditError {
    /// A storage-level failure (database I/O, schema, parse, etc.).
    Storage(String),
}

impl std::fmt::Display for AuditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Storage(e) => write!(f, "audit error: {e}"),
        }
    }
}

impl std::error::Error for AuditError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_log() -> AuditLog {
        AuditLog::in_memory().unwrap()
    }

    #[test]
    fn append_and_query_by_session() {
        let log = test_log();
        let entry = AuditEntry::tool_execution("sess-1", "bash", "ls -la", "success");
        log.append(&entry).unwrap();

        let entries = log.query_by_session("sess-1").unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].tool_name, "bash");
        assert_eq!(entries[0].outcome, "success");
    }

    #[test]
    fn query_by_session_empty() {
        let log = test_log();
        let entries = log.query_by_session("nonexistent").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn query_by_type() {
        let log = test_log();
        log.append(&AuditEntry::tool_execution("s1", "bash", "cmd", "ok"))
            .unwrap();
        log.append(&AuditEntry::permission_decision(
            "s1", "bash", "execute", "approved",
        ))
        .unwrap();
        log.append(&AuditEntry::tool_execution("s1", "read", "file.rs", "ok"))
            .unwrap();

        let tools = log.query_by_type(&AuditEventType::ToolExecution).unwrap();
        assert_eq!(tools.len(), 2);

        let perms = log
            .query_by_type(&AuditEventType::PermissionDecision)
            .unwrap();
        assert_eq!(perms.len(), 1);
    }

    #[test]
    fn query_by_range() {
        let log = test_log();
        let now = Utc::now();
        log.append(&AuditEntry::tool_execution("s1", "bash", "cmd", "ok"))
            .unwrap();

        let start = now - chrono::Duration::seconds(10);
        let end = now + chrono::Duration::seconds(10);
        let entries = log.query_by_range(start, end).unwrap();
        assert_eq!(entries.len(), 1);

        let future_start = now + chrono::Duration::hours(1);
        let future_end = now + chrono::Duration::hours(2);
        let empty = log.query_by_range(future_start, future_end).unwrap();
        assert!(empty.is_empty());
    }

    #[test]
    fn entry_with_details() {
        let log = test_log();
        let entry = AuditEntry::tool_execution("s1", "write", "output.txt", "ok")
            .with_details("wrote 1024 bytes");
        log.append(&entry).unwrap();

        let entries = log.query_by_session("s1").unwrap();
        assert_eq!(entries[0].details.as_deref(), Some("wrote 1024 bytes"));
    }

    #[test]
    fn append_returns_row_id() {
        let log = test_log();
        let id1 = log
            .append(&AuditEntry::tool_execution("s1", "a", "b", "c"))
            .unwrap();
        let id2 = log
            .append(&AuditEntry::tool_execution("s1", "d", "e", "f"))
            .unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
    }

    #[test]
    fn multiple_sessions() {
        let log = test_log();
        log.append(&AuditEntry::tool_execution("s1", "bash", "cmd1", "ok"))
            .unwrap();
        log.append(&AuditEntry::tool_execution("s2", "bash", "cmd2", "ok"))
            .unwrap();
        log.append(&AuditEntry::tool_execution("s1", "read", "f.rs", "ok"))
            .unwrap();

        assert_eq!(log.query_by_session("s1").unwrap().len(), 2);
        assert_eq!(log.query_by_session("s2").unwrap().len(), 1);
    }
}
