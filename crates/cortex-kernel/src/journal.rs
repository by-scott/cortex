use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::Mutex,
};

use chrono::{DateTime, Utc};
use cortex_types::{CorrelationId, Event, Payload, TurnId};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

/// Serialized payload size threshold (in bytes) above which the payload is
/// written to an external blob file instead of being stored inline in `SQLite`.
pub const EXTERNALIZE_THRESHOLD: usize = 4096;

/// SQL schema for the core journal events table and its indexes.
const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS journal_events (
    offset INTEGER PRIMARY KEY AUTOINCREMENT,
    event_id TEXT NOT NULL,
    turn_id TEXT NOT NULL,
    correlation_id TEXT NOT NULL,
    timestamp TEXT NOT NULL,
    event_type TEXT NOT NULL,
    payload BLOB NOT NULL,
    execution_version TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS idx_correlation_id ON journal_events(correlation_id);
CREATE INDEX IF NOT EXISTS idx_event_type ON journal_events(event_type);";

/// SQL schema for the skill utilities persistence table.
const SKILL_SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS skill_utilities (
    name TEXT PRIMARY KEY,
    score REAL NOT NULL DEFAULT 0.5,
    updated_at TEXT NOT NULL DEFAULT ''
);";

/// Append-only event store backed by `SQLite`.
///
/// Stores [`Event`] records in a `WAL`-mode `SQLite` database and optionally
/// externalises large payloads to individual blob files on disk.
pub struct Journal {
    conn: Mutex<Connection>,
    blob_dir: Option<PathBuf>,
}

impl Journal {
    /// Open a journal database at the given file path.
    ///
    /// Creates the schema tables if they do not already exist and enables
    /// `WAL` mode for concurrent-read performance.
    ///
    /// # Errors
    ///
    /// Returns `JournalError::Sqlite` if the database cannot be opened or the
    /// schema cannot be created.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        let blob_dir = path.as_ref().parent().map(|p| p.join("blobs"));
        if let Some(ref d) = blob_dir {
            let _ = std::fs::create_dir_all(d);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        // Migration: add execution_version column for databases created before
        // versioning was introduced.
        let _ = conn.execute_batch(
            "ALTER TABLE journal_events ADD COLUMN execution_version TEXT NOT NULL DEFAULT ''",
        );
        conn.execute_batch(SKILL_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            blob_dir,
        })
    }

    /// Open an in-memory journal (useful for testing).
    ///
    /// # Errors
    ///
    /// Returns `JournalError::Sqlite` if the in-memory database cannot be
    /// created.
    #[must_use = "returns the newly created in-memory journal"]
    pub fn open_in_memory() -> Result<Self, JournalError> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        conn.execute_batch(SKILL_SCHEMA)?;
        Ok(Self {
            conn: Mutex::new(conn),
            blob_dir: None,
        })
    }

    /// Append a single event to the journal.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if serialization or the database insert fails.
    pub fn append(&self, event: &Event) -> Result<u64, JournalError> {
        let offset = {
            let conn = self.lock_conn()?;
            Self::append_inner(&conn, event, self.blob_dir.as_deref())?
        };
        Ok(offset)
    }

    /// Append multiple events in a single transaction.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if any event fails to serialize or insert.  All
    /// events are rolled back on failure.
    pub fn append_batch(&self, events: &[Event]) -> Result<Vec<u64>, JournalError> {
        if events.is_empty() {
            return Ok(Vec::new());
        }
        let mut conn = self.lock_conn()?;
        Self::append_batch_inner(&mut conn, events, self.blob_dir.as_deref())
    }

    fn append_batch_inner(
        conn: &mut rusqlite::Connection,
        events: &[Event],
        blob_dir: Option<&std::path::Path>,
    ) -> Result<Vec<u64>, JournalError> {
        let tx = conn.transaction()?;
        let mut offsets = Vec::with_capacity(events.len());
        for event in events {
            let offset = Self::append_inner(&tx, event, blob_dir)?;
            offsets.push(offset);
        }
        tx.commit()?;
        Ok(offsets)
    }

    /// Query all events sharing a given correlation ID.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query or deserialization fails.
    pub fn query_by_correlation(
        &self,
        correlation_id: &CorrelationId,
    ) -> Result<Vec<StoredEvent>, JournalError> {
        let mut events = {
            let conn = self.lock_conn()?;
            query_journal_events(
                &conn,
                "SELECT offset, event_id, turn_id, correlation_id, timestamp, \
                        event_type, payload, execution_version \
                 FROM journal_events WHERE correlation_id = ?1 ORDER BY offset ASC",
                &[&correlation_id.to_string() as &dyn rusqlite::types::ToSql],
            )?
        };
        resolve_externalized(&mut events, self.blob_dir.as_deref());
        Ok(events)
    }

    /// Return the most recent `n` events, ordered oldest-first.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query or deserialization fails.
    pub fn recent_events(&self, n: usize) -> Result<Vec<StoredEvent>, JournalError> {
        let n_i64 = i64::try_from(n).unwrap_or(i64::MAX);
        let mut events = {
            let conn = self.lock_conn()?;
            query_journal_events(
                &conn,
                "SELECT offset, event_id, turn_id, correlation_id, timestamp, \
                        event_type, payload, execution_version \
                 FROM journal_events ORDER BY offset DESC LIMIT ?1",
                &[&n_i64 as &dyn rusqlite::types::ToSql],
            )?
        };
        events.reverse();
        resolve_externalized(&mut events, self.blob_dir.as_deref());
        Ok(events)
    }

    /// Return events whose offsets fall within `[start_offset, end_offset)`.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query or deserialization fails.
    pub fn events_in_range(
        &self,
        start_offset: u64,
        end_offset: u64,
    ) -> Result<Vec<StoredEvent>, JournalError> {
        let mut events = {
            let conn = self.lock_conn()?;
            let start_i64 = i64::try_from(start_offset).unwrap_or(i64::MAX);
            let end_i64 = i64::try_from(end_offset).unwrap_or(i64::MAX);
            query_journal_events(
                &conn,
                "SELECT offset, event_id, turn_id, correlation_id, timestamp, \
                        event_type, payload, execution_version \
                 FROM journal_events WHERE offset >= ?1 AND offset < ?2 ORDER BY offset ASC",
                &[
                    &start_i64 as &dyn rusqlite::types::ToSql,
                    &end_i64 as &dyn rusqlite::types::ToSql,
                ],
            )?
        };
        resolve_externalized(&mut events, self.blob_dir.as_deref());
        Ok(events)
    }

    /// Return the total number of events in the journal.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query fails.
    pub fn event_count(&self) -> Result<u64, JournalError> {
        let count: i64 = {
            let conn = self.lock_conn()?;
            conn.query_row("SELECT COUNT(*) FROM journal_events", [], |row| row.get(0))?
        };
        Ok(u64::try_from(count).unwrap_or(0))
    }

    /// Load all persisted skill utility scores.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query fails.
    pub fn load_skill_utilities(&self) -> Result<HashMap<String, f64>, JournalError> {
        let conn = self.lock_conn()?;
        load_skill_utilities_inner(&conn)
    }

    /// Persist a skill utility score.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the insert fails.
    pub fn save_skill_utility(&self, name: &str, score: f64) -> Result<(), JournalError> {
        self.lock_conn()?.execute(
            "INSERT OR REPLACE INTO skill_utilities (name, score, updated_at) \
             VALUES (?1, ?2, ?3)",
            params![name, score, Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    /// Create a checkpoint by recording the current maximum offset.
    ///
    /// Appends a [`Payload::SnapshotCreated`] event whose `offset` field is the
    /// current tail of the journal.  This offset can later be passed to
    /// [`Journal::restore_from_checkpoint`] to replay only post-checkpoint
    /// events.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the max-offset query or append fails.
    pub fn create_checkpoint(&self) -> Result<u64, JournalError> {
        let max_offset: i64 = self
            .lock_conn()?
            .query_row(
                "SELECT COALESCE(MAX(offset), 0) FROM journal_events",
                [],
                |row| row.get(0),
            )
            .map_err(JournalError::from)?;
        let offset = u64::try_from(max_offset).unwrap_or(0);
        let event = Event::new(
            TurnId::new(),
            CorrelationId::new(),
            Payload::SnapshotCreated { offset },
        );
        self.append(&event)?;
        Ok(offset)
    }

    /// Return all events written after (and including) the given offset.
    ///
    /// Intended for use after [`Journal::create_checkpoint`]: pass the offset
    /// returned by the checkpoint to replay only the events that occurred
    /// since.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the query or deserialization fails.
    pub fn restore_from_checkpoint(
        &self,
        checkpoint_offset: u64,
    ) -> Result<Vec<StoredEvent>, JournalError> {
        let mut events = {
            let conn = self.lock_conn()?;
            let offset_i64 = i64::try_from(checkpoint_offset).unwrap_or(i64::MAX);
            query_journal_events(
                &conn,
                "SELECT offset, event_id, turn_id, correlation_id, timestamp, \
                        event_type, payload, execution_version \
                 FROM journal_events WHERE offset >= ?1 ORDER BY offset ASC",
                &[&offset_i64 as &dyn rusqlite::types::ToSql],
            )?
        };
        resolve_externalized(&mut events, self.blob_dir.as_deref());
        Ok(events)
    }

    /// Delete blob files not referenced by any journal event.
    ///
    /// Scans all events with type `ExternalizedPayload`, deserializes their
    /// payloads to collect referenced hashes, then removes any blob file whose
    /// name is not in that set.  Returns the number of deleted files.
    ///
    /// # Errors
    ///
    /// Returns `JournalError` if the database cannot be queried.
    pub fn gc_unreferenced_blobs(&self) -> Result<usize, JournalError> {
        let Some(ref blob_dir) = self.blob_dir else {
            return Ok(0);
        };

        // Collect every hash referenced by an ExternalizedPayload event.
        let referenced_hashes = {
            let conn = self.lock_conn()?;
            collect_blob_hashes(&conn)?
        };

        // Walk blob directory, removing any file not in the referenced set.
        let entries = std::fs::read_dir(blob_dir)
            .map_err(|e| JournalError::Serialization(format!("cannot read blobs dir: {e}")))?;
        let mut deleted = 0usize;
        for entry in entries.flatten() {
            let name = entry.file_name();
            let hash = name.to_string_lossy();
            if !referenced_hashes.contains(hash.as_ref()) {
                let _ = std::fs::remove_file(entry.path());
                deleted += 1;
            }
        }
        Ok(deleted)
    }

    // ---- private helpers ----

    fn lock_conn(&self) -> Result<std::sync::MutexGuard<'_, Connection>, JournalError> {
        self.conn
            .lock()
            .map_err(|e| JournalError::Serialization(format!("mutex poisoned: {e}")))
    }

    fn append_inner(
        conn: &Connection,
        event: &Event,
        blob_dir: Option<&Path>,
    ) -> Result<u64, JournalError> {
        let payload_bytes = rmp_serde::to_vec(&event.payload)
            .map_err(|e| JournalError::Serialization(e.to_string()))?;
        let event_type = event_type_name(&event.payload);

        let store_bytes = if payload_bytes.len() > EXTERNALIZE_THRESHOLD {
            if let Some(dir) = blob_dir {
                let hash = hex::encode(Sha256::digest(&payload_bytes));
                let blob_path = dir.join(&hash);
                std::fs::write(&blob_path, &payload_bytes).map_err(|e| {
                    JournalError::Serialization(format!("blob write {}: {e}", blob_path.display()))
                })?;
                let placeholder = Payload::ExternalizedPayload {
                    hash,
                    size: payload_bytes.len(),
                    original_type: event_type.to_owned(),
                };
                rmp_serde::to_vec(&placeholder)
                    .map_err(|e| JournalError::Serialization(e.to_string()))?
            } else {
                payload_bytes
            }
        } else {
            payload_bytes
        };

        conn.execute(
            "INSERT INTO journal_events \
                (event_id, turn_id, correlation_id, timestamp, event_type, payload, execution_version) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(),
                event.turn_id.to_string(),
                event.correlation_id.to_string(),
                event.timestamp.to_rfc3339(),
                event_type,
                store_bytes,
                event.execution_version,
            ],
        )?;
        Ok(u64::try_from(conn.last_insert_rowid()).unwrap_or(0))
    }
}

// ---------------------------------------------------------------------------
// StoredEvent
// ---------------------------------------------------------------------------

/// A journal event together with its database-assigned offset.
#[derive(Debug, Clone)]
pub struct StoredEvent {
    pub offset: u64,
    pub event_id: String,
    pub turn_id: String,
    pub correlation_id: String,
    pub timestamp: DateTime<Utc>,
    pub event_type: String,
    pub payload: Payload,
    pub execution_version: String,
}

// ---------------------------------------------------------------------------
// Free helpers
// ---------------------------------------------------------------------------

fn query_journal_events(
    conn: &rusqlite::Connection,
    sql: &str,
    params_slice: &[&dyn rusqlite::types::ToSql],
) -> Result<Vec<StoredEvent>, JournalError> {
    let mut stmt = conn.prepare(sql)?;
    stmt.query_map(params_slice, row_to_stored_event)?
        .collect::<Result<Vec<_>, _>>()
        .map_err(JournalError::from)
}

fn collect_blob_hashes(conn: &rusqlite::Connection) -> Result<HashSet<String>, JournalError> {
    let mut stmt = conn
        .prepare("SELECT payload FROM journal_events WHERE event_type = 'ExternalizedPayload'")?;
    let rows = stmt.query_map([], |row| {
        let bytes: Vec<u8> = row.get(0)?;
        Ok(bytes)
    })?;
    let mut hashes = HashSet::new();
    for row in rows {
        let bytes = row.map_err(JournalError::from)?;
        if let Ok(Payload::ExternalizedPayload { hash, .. }) =
            rmp_serde::from_slice::<Payload>(&bytes)
        {
            hashes.insert(hash);
        }
    }
    Ok(hashes)
}

fn load_skill_utilities_inner(
    conn: &rusqlite::Connection,
) -> Result<HashMap<String, f64>, JournalError> {
    let mut stmt = conn.prepare("SELECT name, score FROM skill_utilities")?;
    let rows = stmt.query_map([], |row| {
        let name: String = row.get(0)?;
        let score: f64 = row.get(1)?;
        Ok((name, score))
    })?;
    let mut map = HashMap::new();
    for row in rows {
        let (name, score) = row.map_err(JournalError::from)?;
        map.insert(name, score);
    }
    Ok(map)
}

fn row_to_stored_event(row: &rusqlite::Row<'_>) -> Result<StoredEvent, rusqlite::Error> {
    let payload_bytes: Vec<u8> = row.get(6)?;
    let payload: Payload = rmp_serde::from_slice(&payload_bytes).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Blob, Box::new(e))
    })?;
    let timestamp_str: String = row.get(4)?;
    let timestamp = DateTime::parse_from_rfc3339(&timestamp_str)
        .map(|dt| dt.with_timezone(&Utc))
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(e))
        })?;

    let execution_version: String = row.get(7).unwrap_or_default();

    Ok(StoredEvent {
        offset: u64::try_from(row.get::<_, i64>(0)?).unwrap_or(0),
        event_id: row.get(1)?,
        turn_id: row.get(2)?,
        correlation_id: row.get(3)?,
        timestamp,
        event_type: row.get(5)?,
        payload,
        execution_version,
    })
}

/// Resolve any [`Payload::ExternalizedPayload`] placeholders back to their
/// original payloads by reading the corresponding blob files.
fn resolve_externalized(events: &mut [StoredEvent], blob_dir: Option<&Path>) {
    let Some(dir) = blob_dir else { return };
    for event in events {
        if let Payload::ExternalizedPayload { ref hash, .. } = event.payload {
            let blob_path = dir.join(hash);
            if let Ok(bytes) = std::fs::read(&blob_path)
                && let Ok(original) = rmp_serde::from_slice::<Payload>(&bytes)
            {
                event.payload = original;
            }
        }
    }
}

/// Map a [`Payload`] variant to a stable string tag stored in the `event_type`
/// column.
#[must_use]
const fn event_type_name(payload: &Payload) -> &'static str {
    match payload {
        Payload::TurnStarted => "TurnStarted",
        Payload::TurnCompleted => "TurnCompleted",
        Payload::TurnInterrupted => "TurnInterrupted",
        Payload::SessionStarted { .. } => "SessionStarted",
        Payload::SessionEnded { .. } => "SessionEnded",
        Payload::UserMessage { .. } => "UserMessage",
        Payload::AssistantMessage { .. } => "AssistantMessage",
        Payload::ToolInvocationIntent { .. } => "ToolInvocationIntent",
        Payload::ToolInvocationResult { .. } => "ToolInvocationResult",
        Payload::PermissionRequested { .. } => "PermissionRequested",
        Payload::PermissionGranted { .. } => "PermissionGranted",
        Payload::PermissionDenied { .. } => "PermissionDenied",
        Payload::ContextPressureObserved { .. } => "ContextPressureObserved",
        Payload::ContextCompacted { .. } => "ContextCompacted",
        Payload::ContextCompactBoundary { .. } => "ContextCompactBoundary",
        Payload::ImpasseDetected { .. } => "ImpasseDetected",
        Payload::ConflictDetected { .. } => "ConflictDetected",
        Payload::MetaControlApplied { .. } => "MetaControlApplied",
        Payload::FrameCheckResult { .. } => "FrameCheckResult",
        Payload::GoalSet { .. } => "GoalSet",
        Payload::GoalShifted { .. } => "GoalShifted",
        Payload::GoalCompleted { .. } => "GoalCompleted",
        Payload::MemoryCaptured { .. } => "MemoryCaptured",
        Payload::MemoryMaterialized { .. } => "MemoryMaterialized",
        Payload::MemoryStabilized { .. } => "MemoryStabilized",
        Payload::LlmCallCompleted { .. } => "LlmCallCompleted",
        Payload::WorkingMemoryItemActivated { .. } => "WorkingMemoryItemActivated",
        Payload::WorkingMemoryItemRehearsed { .. } => "WorkingMemoryItemRehearsed",
        Payload::WorkingMemoryItemEvicted { .. } => "WorkingMemoryItemEvicted",
        Payload::WorkingMemoryCapacityExceeded { .. } => "WorkingMemoryCapacityExceeded",
        Payload::ChannelScheduled { .. } => "ChannelScheduled",
        Payload::MaintenanceExecuted { .. } => "MaintenanceExecuted",
        Payload::EmergencyTriggered { .. } => "EmergencyTriggered",
        Payload::GuardrailTriggered { .. } => "GuardrailTriggered",
        Payload::ExternalInputObserved { .. } => "ExternalInputObserved",
        Payload::ConfidenceAssessed { .. } => "ConfidenceAssessed",
        Payload::ConfidenceLow { .. } => "ConfidenceLow",
        Payload::PressureResponseApplied { .. } => "PressureResponseApplied",
        Payload::AcpClientSpawned { .. } => "AcpClientSpawned",
        Payload::AcpClientResponse { .. } => "AcpClientResponse",
        Payload::AgentWorkerSpawned { .. } => "AgentWorkerSpawned",
        Payload::AgentWorkerCompleted { .. } => "AgentWorkerCompleted",
        Payload::DelegationCompleted { .. } => "DelegationCompleted",
        Payload::PromptUpdated { .. } => "PromptUpdated",
        Payload::ReasoningStarted { .. } => "ReasoningStarted",
        Payload::ReasoningStepCompleted { .. } => "ReasoningStepCompleted",
        Payload::ReasoningBranchEvaluated { .. } => "ReasoningBranchEvaluated",
        Payload::ReasoningChainCompleted { .. } => "ReasoningChainCompleted",
        Payload::TaskDecomposed { .. } => "TaskDecomposed",
        Payload::TaskAggregated { .. } => "TaskAggregated",
        Payload::TaskClaimed { .. } => "TaskClaimed",
        Payload::WorkflowSpecLoaded { .. } => "WorkflowSpecLoaded",
        Payload::CausalAnalysisCompleted { .. } => "CausalAnalysisCompleted",
        Payload::EmbeddingModelSwitched { .. } => "EmbeddingModelSwitched",
        Payload::EmbeddingDegraded { .. } => "EmbeddingDegraded",
        Payload::SkillInvoked { .. } => "SkillInvoked",
        Payload::SkillCompleted { .. } => "SkillCompleted",
        Payload::PluginLoaded { .. } => "PluginLoaded",
        Payload::AuditQueryExecuted { .. } => "AuditQueryExecuted",
        Payload::HealthAutoRecoveryTriggered { .. } => "HealthAutoRecoveryTriggered",
        Payload::AlertFired { .. } => "AlertFired",
        Payload::SecuritySanitized { .. } => "SecuritySanitized",
        Payload::ConfigValidated { .. } => "ConfigValidated",
        Payload::PluginDiscovered { .. } => "PluginDiscovered",
        Payload::MemorySplit { .. } => "MemorySplit",
        Payload::MemoryGraphHealthAssessed { .. } => "MemoryGraphHealthAssessed",
        Payload::MemoryRelationReorganized { .. } => "MemoryRelationReorganized",
        Payload::ReasoningReflection { .. } => "ReasoningReflection",
        Payload::CausalRetrospect { .. } => "CausalRetrospect",
        Payload::SnapshotCreated { .. } => "SnapshotCreated",
        Payload::ProjectionCheckpoint { .. } => "ProjectionCheckpoint",
        Payload::SelfModification { .. } => "SelfModification",
        Payload::SideEffectRecorded { .. } => "SideEffectRecorded",
        Payload::ExternalizedPayload { .. } => "ExternalizedPayload",
        Payload::QualityCheckSuggested { .. } => "QualityCheckSuggested",
        Payload::ExplorationTriggered { .. } => "ExplorationTriggered",
        Payload::MaintenanceCycleCompleted { .. } => "MaintenanceCycleCompleted",
    }
}

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur during journal operations.
#[derive(Debug)]
pub enum JournalError {
    /// An underlying `SQLite` error.
    Sqlite(rusqlite::Error),
    /// A serialization or I/O error described by the contained message.
    Serialization(String),
}

impl From<rusqlite::Error> for JournalError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl std::fmt::Display for JournalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "journal sqlite error: {e}"),
            Self::Serialization(e) => write!(f, "journal serialization error: {e}"),
        }
    }
}

impl std::error::Error for JournalError {}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use cortex_types::{CorrelationId, Payload, TurnId};

    fn make_event(turn_id: TurnId, corr_id: CorrelationId, payload: Payload) -> Event {
        Event::new(turn_id, corr_id, payload)
    }

    #[test]
    fn open_and_append() {
        let journal = Journal::open_in_memory().unwrap();
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();
        let event = make_event(turn_id, corr_id, Payload::TurnStarted);
        let offset = journal.append(&event).unwrap();
        assert_eq!(offset, 1);
        assert_eq!(journal.event_count().unwrap(), 1);
    }

    #[test]
    fn append_and_query_by_correlation() {
        let journal = Journal::open_in_memory().unwrap();
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();

        journal
            .append(&make_event(turn_id, corr_id, Payload::TurnStarted))
            .unwrap();
        journal
            .append(&make_event(
                turn_id,
                corr_id,
                Payload::UserMessage {
                    content: "hello".into(),
                },
            ))
            .unwrap();
        // Different correlation
        let other_corr = CorrelationId::new();
        journal
            .append(&make_event(turn_id, other_corr, Payload::TurnCompleted))
            .unwrap();

        let events = journal.query_by_correlation(&corr_id).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0].payload, Payload::TurnStarted));
        assert!(matches!(events[1].payload, Payload::UserMessage { .. }));
    }

    #[test]
    fn recent_events() {
        let journal = Journal::open_in_memory().unwrap();
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();

        for _ in 0..10 {
            journal
                .append(&make_event(turn_id, corr_id, Payload::TurnStarted))
                .unwrap();
        }

        let recent = journal.recent_events(5).unwrap();
        assert_eq!(recent.len(), 5);
        assert!(recent[0].offset < recent[4].offset);
    }

    #[test]
    fn payload_roundtrip_through_sqlite() {
        let journal = Journal::open_in_memory().unwrap();
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();

        let payload = Payload::ToolInvocationResult {
            tool_name: "read".into(),
            output: "file contents".into(),
            is_error: false,
        };
        journal
            .append(&make_event(turn_id, corr_id, payload.clone()))
            .unwrap();

        let events = journal.recent_events(1).unwrap();
        assert_eq!(events[0].payload, payload);
    }

    #[test]
    fn append_batch_writes_all() {
        let journal = Journal::open_in_memory().unwrap();
        let tid = TurnId::new();
        let cid = CorrelationId::new();
        let events: Vec<Event> = (0..5)
            .map(|i| {
                make_event(
                    tid,
                    cid,
                    Payload::UserMessage {
                        content: format!("msg-{i}"),
                    },
                )
            })
            .collect();
        let offsets = journal.append_batch(&events).unwrap();
        assert_eq!(offsets.len(), 5);
        let all = journal.recent_events(10).unwrap();
        assert_eq!(all.len(), 5);
    }

    #[test]
    fn append_batch_empty() {
        let journal = Journal::open_in_memory().unwrap();
        let offsets = journal.append_batch(&[]).unwrap();
        assert!(offsets.is_empty());
    }

    #[test]
    fn checkpoint_uses_max_offset() {
        let journal = Journal::open_in_memory().unwrap();
        let tid = TurnId::new();
        let cid = CorrelationId::new();

        // Empty journal checkpoint should be 0.
        let cp0 = journal.create_checkpoint().unwrap();
        assert_eq!(cp0, 0);

        // Append some events, then checkpoint again.
        for _ in 0..3 {
            journal
                .append(&make_event(tid, cid, Payload::TurnStarted))
                .unwrap();
        }
        // Now we have offsets 1 (checkpoint-snap), 2, 3, 4.
        let cp1 = journal.create_checkpoint().unwrap();
        assert_eq!(cp1, 4);
    }

    #[test]
    fn events_in_range() {
        let journal = Journal::open_in_memory().unwrap();
        let tid = TurnId::new();
        let cid = CorrelationId::new();
        for _ in 0..5 {
            journal
                .append(&make_event(tid, cid, Payload::TurnStarted))
                .unwrap();
        }
        let range = journal.events_in_range(2, 4).unwrap();
        assert_eq!(range.len(), 2);
        assert_eq!(range[0].offset, 2);
        assert_eq!(range[1].offset, 3);
    }

    #[test]
    fn skill_utility_roundtrip() {
        let journal = Journal::open_in_memory().unwrap();
        journal.save_skill_utility("read", 0.9).unwrap();
        journal.save_skill_utility("write", 0.4).unwrap();
        let utils = journal.load_skill_utilities().unwrap();
        assert!((utils["read"] - 0.9).abs() < f64::EPSILON);
        assert!((utils["write"] - 0.4).abs() < f64::EPSILON);
    }

    #[test]
    fn restore_from_checkpoint() {
        let journal = Journal::open_in_memory().unwrap();
        let tid = TurnId::new();
        let cid = CorrelationId::new();

        for _ in 0..3 {
            journal
                .append(&make_event(tid, cid, Payload::TurnStarted))
                .unwrap();
        }
        let cp = journal.create_checkpoint().unwrap();
        // Add more events after checkpoint.
        journal
            .append(&make_event(
                tid,
                cid,
                Payload::UserMessage {
                    content: "post-cp".into(),
                },
            ))
            .unwrap();

        let post = journal.restore_from_checkpoint(cp).unwrap();
        // Should include the checkpoint offset itself and everything after.
        assert!(post.len() >= 2);
    }

    #[test]
    fn event_type_name_covers_all_variants() {
        // Ensure that event_type_name returns a non-empty string for every
        // variant.  If a new variant is added to Payload without updating
        // event_type_name, this test will fail to compile due to the
        // non-exhaustive match.
        let name = event_type_name(&Payload::TurnStarted);
        assert!(!name.is_empty());
    }
}
