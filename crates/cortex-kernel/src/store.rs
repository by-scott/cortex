use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use cortex_types::{
    ActorId, AuthContext, ClientId, DeliveryStatus, FastCapture, OutboundDeliveryRecord,
    OwnedScope, PermissionDecision, PermissionRequest, PermissionResolution,
    PermissionResolutionError, PermissionStatus, PermissionTicket, SemanticMemory, SessionId,
    SideEffectId, SideEffectIntent, SideEffectKind, SideEffectRecord, SideEffectStatus, TenantId,
    TokenUsage, TransportCapabilities, UsageRecord, Visibility,
};
use rusqlite::{Connection, OptionalExtension, params};
use serde::Deserialize;

#[derive(Debug)]
pub enum StoreError {
    AccessDenied,
    CountOutOfRange(i64),
    InvalidVisibility(String),
    Io(std::io::Error),
    Json(serde_json::Error),
    Permission(PermissionResolutionError),
    Sqlite(rusqlite::Error),
    ValueOutOfRange(&'static str),
}

#[derive(Debug)]
pub enum DbWriterError {
    Receive,
    Send,
    Store(StoreError),
}

impl From<StoreError> for DbWriterError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        Self::Sqlite(error)
    }
}

impl From<serde_json::Error> for StoreError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionRecord {
    pub session_id: SessionId,
    pub scope: OwnedScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MigrationReport {
    pub imported_sessions: usize,
    pub skipped_files: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreHealth {
    pub journal_mode: String,
    pub synchronous: String,
    pub foreign_keys: bool,
    pub busy_timeout_ms: u64,
    pub wal_autocheckpoint_pages: u64,
}

pub struct SqliteStore {
    connection: Connection,
}

pub struct DbWriter {
    sender: mpsc::Sender<DbMessage>,
    handle: Option<thread::JoinHandle<()>>,
}

type DbJob = Box<dyn FnOnce(&SqliteStore) + Send + 'static>;

enum DbMessage {
    Write(DbJob),
    Shutdown,
}

const MIGRATION_SQL: &str = "
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE TABLE IF NOT EXISTS tenants (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS sessions_owner_idx
    ON sessions(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS clients (
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT NOT NULL,
    capabilities_json TEXT NOT NULL,
    active_session_id TEXT,
    PRIMARY KEY (tenant_id, actor_id, client_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    FOREIGN KEY (active_session_id) REFERENCES sessions(id)
);
CREATE TABLE IF NOT EXISTS fast_captures (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS fast_captures_owner_idx
    ON fast_captures(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS semantic_memories (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS semantic_memories_owner_idx
    ON semantic_memories(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS permission_requests (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS permission_requests_owner_idx
    ON permission_requests(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS permission_resolutions (
    id TEXT PRIMARY KEY,
    request_id TEXT NOT NULL UNIQUE,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    decision TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (request_id) REFERENCES permission_requests(id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE TABLE IF NOT EXISTS permission_tickets (
    request_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    status TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (request_id) REFERENCES permission_requests(id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS permission_tickets_owner_idx
    ON permission_tickets(tenant_id, actor_id, visibility, status);
CREATE TABLE IF NOT EXISTS delivery_records (
    delivery_id TEXT NOT NULL,
    recipient_client_id TEXT NOT NULL,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status TEXT NOT NULL,
    attempts INTEGER NOT NULL,
    json TEXT NOT NULL,
    PRIMARY KEY (delivery_id, recipient_client_id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
CREATE INDEX IF NOT EXISTS delivery_records_owner_idx
    ON delivery_records(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS usage_records (
    turn_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    session_id TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id),
    FOREIGN KEY (session_id) REFERENCES sessions(id)
);
CREATE INDEX IF NOT EXISTS usage_records_owner_idx
    ON usage_records(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS side_effect_intents (
    id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    kind TEXT NOT NULL,
    idempotency_key TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE UNIQUE INDEX IF NOT EXISTS side_effect_intents_idempotency_idx
    ON side_effect_intents(tenant_id, actor_id, idempotency_key);
CREATE INDEX IF NOT EXISTS side_effect_intents_owner_idx
    ON side_effect_intents(tenant_id, actor_id, visibility);
CREATE TABLE IF NOT EXISTS side_effect_records (
    intent_id TEXT PRIMARY KEY,
    tenant_id TEXT NOT NULL,
    actor_id TEXT NOT NULL,
    client_id TEXT,
    visibility TEXT NOT NULL,
    status TEXT NOT NULL,
    json TEXT NOT NULL,
    FOREIGN KEY (intent_id) REFERENCES side_effect_intents(id),
    FOREIGN KEY (tenant_id) REFERENCES tenants(id)
);
CREATE INDEX IF NOT EXISTS side_effect_records_owner_idx
    ON side_effect_records(tenant_id, actor_id, visibility, status);
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (1, 'multi_user_core');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (2, 'memory_core');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (3, 'permission_core');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (4, 'delivery_outbox');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (5, 'usage_ledger');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (6, 'permission_ticket_lifecycle');
INSERT OR IGNORE INTO schema_migrations(version, name)
    VALUES (7, 'side_effect_ledger');
";

impl SqliteStore {
    /// # Errors
    /// Returns an error when the database cannot be opened or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let store = Self {
            connection: Connection::open(path)?,
        };
        store.configure_connection()?;
        store.migrate()?;
        Ok(store)
    }

    fn configure_connection(&self) -> Result<(), StoreError> {
        self.connection.busy_timeout(Duration::from_secs(5))?;
        self.connection.execute_batch(
            "
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA foreign_keys = ON;
            PRAGMA wal_autocheckpoint = 1000;
            ",
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when schema migration fails.
    pub fn migrate(&self) -> Result<(), StoreError> {
        self.connection.execute_batch(MIGRATION_SQL)?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when `SQLite` diagnostic pragmas cannot be queried.
    pub fn health(&self) -> Result<StoreHealth, StoreError> {
        let journal_mode = self.query_string_pragma("journal_mode")?;
        let synchronous = synchronous_label(self.query_i64_pragma("synchronous")?);
        let foreign_keys = self.query_i64_pragma("foreign_keys")? != 0;
        let busy_timeout_ms = i64_to_u64(self.query_i64_pragma("busy_timeout")?)?;
        let wal_autocheckpoint_pages = i64_to_u64(self.query_i64_pragma("wal_autocheckpoint")?)?;
        Ok(StoreHealth {
            journal_mode,
            synchronous,
            foreign_keys,
            busy_timeout_ms,
            wal_autocheckpoint_pages,
        })
    }

    /// # Errors
    /// Returns an error when `SQLite` cannot run a passive `WAL` checkpoint.
    pub fn checkpoint_passive(&self) -> Result<(), StoreError> {
        self.connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE);")?;
        Ok(())
    }

    fn query_string_pragma(&self, name: &'static str) -> Result<String, StoreError> {
        self.connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, String>(0))
            .map_err(StoreError::from)
    }

    fn query_i64_pragma(&self, name: &'static str) -> Result<i64, StoreError> {
        self.connection
            .query_row(&format!("PRAGMA {name}"), [], |row| row.get::<_, i64>(0))
            .map_err(StoreError::from)
    }

    /// # Errors
    /// Returns an error when the tenant cannot be written.
    pub fn upsert_tenant(&self, tenant_id: &TenantId, name: &str) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO tenants(id, name) VALUES (?1, ?2)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name",
            params![tenant_id.as_str(), name],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the client cannot be written.
    pub fn upsert_client(
        &self,
        context: &AuthContext,
        capabilities: &TransportCapabilities,
    ) -> Result<(), StoreError> {
        let capabilities_json = serde_json::to_string(capabilities)?;
        self.connection.execute(
            "INSERT INTO clients(tenant_id, actor_id, client_id, capabilities_json)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(tenant_id, actor_id, client_id)
             DO UPDATE SET capabilities_json = excluded.capabilities_json",
            params![
                context.tenant_id.as_str(),
                context.actor_id.as_str(),
                context.client_id.as_str(),
                capabilities_json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the session cannot be written.
    pub fn upsert_session(
        &self,
        session_id: &SessionId,
        scope: &OwnedScope,
    ) -> Result<(), StoreError> {
        self.connection.execute(
            "INSERT INTO sessions(id, tenant_id, actor_id, client_id, visibility)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility",
            params![
                session_id.as_str(),
                scope.tenant_id.as_str(),
                scope.actor_id.as_str(),
                scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(scope.visibility),
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the session is not visible to the client or the update fails.
    pub fn set_active_session(
        &self,
        context: &AuthContext,
        session_id: &SessionId,
    ) -> Result<(), StoreError> {
        let Some(session) = self.session(session_id)? else {
            return Err(StoreError::AccessDenied);
        };
        if !session.scope.is_visible_to(context) {
            return Err(StoreError::AccessDenied);
        }
        self.connection.execute(
            "UPDATE clients
             SET active_session_id = ?4
             WHERE tenant_id = ?1 AND actor_id = ?2 AND client_id = ?3",
            params![
                context.tenant_id.as_str(),
                context.actor_id.as_str(),
                context.client_id.as_str(),
                session_id.as_str(),
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query fails.
    pub fn active_session(&self, context: &AuthContext) -> Result<Option<SessionId>, StoreError> {
        Ok(self
            .connection
            .query_row(
                "SELECT active_session_id
                 FROM clients
                 WHERE tenant_id = ?1 AND actor_id = ?2 AND client_id = ?3",
                params![
                    context.tenant_id.as_str(),
                    context.actor_id.as_str(),
                    context.client_id.as_str(),
                ],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten()
            .map(SessionId::from_raw))
    }

    /// # Errors
    /// Returns an error when the query fails.
    pub fn visible_sessions(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<SessionRecord>, StoreError> {
        let mut statement = self.connection.prepare(
            "SELECT id, tenant_id, actor_id, client_id, visibility
             FROM sessions
             WHERE tenant_id = ?1
             ORDER BY id",
        )?;
        let rows = statement.query_map(params![context.tenant_id.as_str()], session_from_row)?;
        let mut sessions = Vec::new();
        for row in rows {
            let session = row?;
            if session.scope.is_visible_to(context) {
                sessions.push(session);
            }
        }
        Ok(sessions)
    }

    /// # Errors
    /// Returns an error when the query fails.
    pub fn applied_migrations(&self) -> Result<Vec<i64>, StoreError> {
        let mut statement = self
            .connection
            .prepare("SELECT version FROM schema_migrations ORDER BY version")?;
        let rows = statement.query_map([], |row| row.get::<_, i64>(0))?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }

    /// # Errors
    /// Returns an error when the query fails.
    pub fn client_count(&self, tenant_id: &TenantId) -> Result<usize, StoreError> {
        let count = self.connection.query_row(
            "SELECT COUNT(*) FROM clients WHERE tenant_id = ?1",
            params![tenant_id.as_str()],
            |row| row.get::<_, i64>(0),
        )?;
        usize::try_from(count).map_err(|_| StoreError::CountOutOfRange(count))
    }

    /// # Errors
    /// Returns an error when the capture cannot be serialized or written.
    pub fn save_fast_capture(&self, capture: &FastCapture) -> Result<(), StoreError> {
        let json = serde_json::to_string(capture)?;
        self.connection.execute(
            "INSERT INTO fast_captures(id, tenant_id, actor_id, client_id, visibility, json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                json = excluded.json",
            params![
                capture.id,
                capture.scope.tenant_id.as_str(),
                capture.scope.actor_id.as_str(),
                capture.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(capture.scope.visibility),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the memory cannot be serialized or written.
    pub fn save_semantic_memory(&self, memory: &SemanticMemory) -> Result<(), StoreError> {
        let json = serde_json::to_string(memory)?;
        self.connection.execute(
            "INSERT INTO semantic_memories(id, tenant_id, actor_id, client_id, visibility, json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                json = excluded.json",
            params![
                memory.id,
                memory.scope.tenant_id.as_str(),
                memory.scope.actor_id.as_str(),
                memory.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(memory.scope.visibility),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_fast_captures(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<FastCapture>, StoreError> {
        let rows = self.visible_json_rows("fast_captures", context)?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .filter_visible(context)
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_semantic_memories(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<SemanticMemory>, StoreError> {
        let rows = self.visible_json_rows("semantic_memories", context)?;
        rows.into_iter()
            .map(|json| serde_json::from_str(&json).map_err(StoreError::from))
            .filter_visible(context)
    }

    /// # Errors
    /// Returns an error when the permission request cannot be written.
    pub fn save_permission_request(&self, request: &PermissionRequest) -> Result<(), StoreError> {
        let json = serde_json::to_string(request)?;
        self.connection.execute(
            "INSERT INTO permission_requests(id, tenant_id, actor_id, client_id, visibility, json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                json = excluded.json",
            params![
                request.id.as_str(),
                request.scope.tenant_id.as_str(),
                request.scope.actor_id.as_str(),
                request.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(request.scope.visibility),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the request is missing, owner validation fails, or
    /// the resolution cannot be persisted.
    pub fn resolve_permission(
        &self,
        resolution: &PermissionResolution,
    ) -> Result<PermissionDecision, StoreError> {
        let request_json = self.connection.query_row(
            "SELECT json FROM permission_requests WHERE id = ?1",
            params![resolution.request_id.as_str()],
            |row| row.get::<_, String>(0),
        )?;
        let request = serde_json::from_str::<PermissionRequest>(&request_json)?;
        let decision = request
            .resolve(resolution)
            .map_err(StoreError::Permission)?;
        let json = serde_json::to_string(resolution)?;
        self.connection.execute(
            "INSERT INTO permission_resolutions(
                id, request_id, tenant_id, actor_id, client_id, decision, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(request_id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                decision = excluded.decision,
                json = excluded.json",
            params![
                resolution.request_id.as_str(),
                resolution.request_id.as_str(),
                resolution.scope.tenant_id.as_str(),
                resolution.scope.actor_id.as_str(),
                resolution.scope.client_id.as_ref().map(ClientId::as_str),
                decision_label(decision),
                json,
            ],
        )?;
        Ok(decision)
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_permission_requests(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<PermissionRequest>, StoreError> {
        let rows = self.visible_json_rows("permission_requests", context)?;
        let mut requests = Vec::new();
        for json in rows {
            let request = serde_json::from_str::<PermissionRequest>(&json)?;
            if request.scope.is_visible_to(context) {
                requests.push(request);
            }
        }
        Ok(requests)
    }

    /// # Errors
    /// Returns an error when the permission ticket cannot be written.
    pub fn save_permission_ticket(&self, ticket: &PermissionTicket) -> Result<(), StoreError> {
        self.save_permission_request(&ticket.request)?;
        let json = serde_json::to_string(ticket)?;
        self.connection.execute(
            "INSERT INTO permission_tickets(
                request_id, tenant_id, actor_id, client_id, visibility, status, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(request_id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                status = excluded.status,
                json = excluded.json",
            params![
                ticket.request.id.as_str(),
                ticket.request.scope.tenant_id.as_str(),
                ticket.request.scope.actor_id.as_str(),
                ticket
                    .request
                    .scope
                    .client_id
                    .as_ref()
                    .map(ClientId::as_str),
                visibility_label(ticket.request.scope.visibility),
                permission_status_label(ticket.status),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_permission_tickets(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<PermissionTicket>, StoreError> {
        let rows = self.visible_json_rows("permission_tickets", context)?;
        let mut tickets = Vec::new();
        for json in rows {
            let ticket = serde_json::from_str::<PermissionTicket>(&json)?;
            if ticket.request.scope.is_visible_to(context) {
                tickets.push(ticket);
            }
        }
        Ok(tickets)
    }

    /// # Errors
    /// Returns an error when the delivery record cannot be serialized or written.
    pub fn save_delivery_record(&self, record: &OutboundDeliveryRecord) -> Result<(), StoreError> {
        let json = serde_json::to_string(record)?;
        self.connection.execute(
            "INSERT INTO delivery_records(
                delivery_id, recipient_client_id, tenant_id, actor_id, client_id,
                visibility, session_id, status, attempts, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(delivery_id, recipient_client_id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                session_id = excluded.session_id,
                status = excluded.status,
                attempts = excluded.attempts,
                json = excluded.json",
            params![
                record.delivery_id.as_str(),
                record.recipient_client_id.as_str(),
                record.scope.tenant_id.as_str(),
                record.scope.actor_id.as_str(),
                record.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(record.scope.visibility),
                record.session_id.as_str(),
                delivery_status_label(record.status),
                i64::from(record.attempts),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_delivery_records(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<OutboundDeliveryRecord>, StoreError> {
        let rows = self.visible_json_rows("delivery_records", context)?;
        let mut records = Vec::new();
        for json in rows {
            let record = serde_json::from_str::<OutboundDeliveryRecord>(&json)?;
            if record.scope.is_visible_to(context) {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// # Errors
    /// Returns an error when the usage record cannot be serialized or written.
    pub fn save_usage_record(&self, record: &UsageRecord) -> Result<(), StoreError> {
        let json = serde_json::to_string(record)?;
        self.connection.execute(
            "INSERT INTO usage_records(
                turn_id, tenant_id, actor_id, client_id, visibility, session_id,
                model, input_tokens, output_tokens, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(turn_id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                session_id = excluded.session_id,
                model = excluded.model,
                input_tokens = excluded.input_tokens,
                output_tokens = excluded.output_tokens,
                json = excluded.json",
            params![
                record.turn_id.as_str(),
                record.scope.tenant_id.as_str(),
                record.scope.actor_id.as_str(),
                record.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(record.scope.visibility),
                record.session_id.as_str(),
                record.model.as_str(),
                u64_to_i64(record.usage.input_tokens, "input_tokens")?,
                u64_to_i64(record.usage.output_tokens, "output_tokens")?,
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_usage_records(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<UsageRecord>, StoreError> {
        let rows = self.visible_json_rows("usage_records", context)?;
        let mut records = Vec::new();
        for json in rows {
            let record = serde_json::from_str::<UsageRecord>(&json)?;
            if record.scope.is_visible_to(context) {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// # Errors
    /// Returns an error when the usage ledger cannot be queried.
    pub fn usage_total(&self, context: &AuthContext) -> Result<TokenUsage, StoreError> {
        Ok(self
            .visible_usage_records(context)?
            .into_iter()
            .fold(TokenUsage::default(), |total, record| {
                total.saturating_add(record.usage)
            }))
    }

    /// # Errors
    /// Returns an error when the side-effect intent cannot be written.
    pub fn save_side_effect_intent(&self, intent: &SideEffectIntent) -> Result<(), StoreError> {
        let json = serde_json::to_string(intent)?;
        self.connection.execute(
            "INSERT INTO side_effect_intents(
                id, tenant_id, actor_id, client_id, visibility, kind, idempotency_key, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                kind = excluded.kind,
                idempotency_key = excluded.idempotency_key,
                json = excluded.json",
            params![
                intent.id.as_str(),
                intent.scope.tenant_id.as_str(),
                intent.scope.actor_id.as_str(),
                intent.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(intent.scope.visibility),
                side_effect_kind_label(intent.kind),
                intent.idempotency_key.as_str(),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the side-effect result cannot be associated with a
    /// known intent or cannot be written.
    pub fn save_side_effect_record(&self, record: &SideEffectRecord) -> Result<(), StoreError> {
        let intent = self.side_effect_intent(&record.intent_id)?;
        if intent.scope != record.scope {
            return Err(StoreError::AccessDenied);
        }
        let json = serde_json::to_string(record)?;
        self.connection.execute(
            "INSERT INTO side_effect_records(
                intent_id, tenant_id, actor_id, client_id, visibility, status, json
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(intent_id) DO UPDATE SET
                tenant_id = excluded.tenant_id,
                actor_id = excluded.actor_id,
                client_id = excluded.client_id,
                visibility = excluded.visibility,
                status = excluded.status,
                json = excluded.json",
            params![
                record.intent_id.as_str(),
                intent.scope.tenant_id.as_str(),
                intent.scope.actor_id.as_str(),
                intent.scope.client_id.as_ref().map(ClientId::as_str),
                visibility_label(intent.scope.visibility),
                side_effect_status_label(record.status),
                json,
            ],
        )?;
        Ok(())
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_side_effect_intents(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<SideEffectIntent>, StoreError> {
        let rows = self.visible_json_rows("side_effect_intents", context)?;
        let mut intents = Vec::new();
        for json in rows {
            let intent = serde_json::from_str::<SideEffectIntent>(&json)?;
            if intent.scope.is_visible_to(context) {
                intents.push(intent);
            }
        }
        Ok(intents)
    }

    /// # Errors
    /// Returns an error when the query or decode fails.
    pub fn visible_side_effect_records(
        &self,
        context: &AuthContext,
    ) -> Result<Vec<SideEffectRecord>, StoreError> {
        let rows = self.visible_json_rows("side_effect_records", context)?;
        let mut records = Vec::new();
        for json in rows {
            let record = serde_json::from_str::<SideEffectRecord>(&json)?;
            if record.scope.is_visible_to(context) {
                records.push(record);
            }
        }
        Ok(records)
    }

    /// # Errors
    /// Returns an error when the legacy directory cannot be read or imported
    /// sessions cannot be written.
    pub fn import_legacy_sessions(
        &self,
        sessions_dir: impl AsRef<Path>,
        tenant_id: &TenantId,
        fallback_client_id: &ClientId,
    ) -> Result<MigrationReport, StoreError> {
        let mut report = MigrationReport {
            imported_sessions: 0,
            skipped_files: 0,
        };
        let sessions_dir = sessions_dir.as_ref();
        if !sessions_dir.exists() {
            return Ok(report);
        }
        for entry in std::fs::read_dir(sessions_dir)? {
            let entry = entry?;
            if entry
                .path()
                .extension()
                .is_none_or(|extension| extension != "json")
            {
                continue;
            }
            let raw = std::fs::read_to_string(entry.path())?;
            let Ok(metadata) = serde_json::from_str::<LegacySessionMetadata>(&raw) else {
                report.skipped_files += 1;
                continue;
            };
            let (actor_id, client_id) =
                legacy_owner_to_scope_parts(&metadata.owner_actor, fallback_client_id);
            self.upsert_session(
                &metadata.id,
                &OwnedScope::new(
                    tenant_id.clone(),
                    actor_id,
                    Some(client_id),
                    Visibility::Private,
                ),
            )?;
            report.imported_sessions += 1;
        }
        Ok(report)
    }

    fn session(&self, session_id: &SessionId) -> Result<Option<SessionRecord>, StoreError> {
        self.connection
            .query_row(
                "SELECT id, tenant_id, actor_id, client_id, visibility
                 FROM sessions
                 WHERE id = ?1",
                params![session_id.as_str()],
                session_from_row,
            )
            .optional()
            .map_err(StoreError::from)
    }

    fn side_effect_intent(&self, intent_id: &SideEffectId) -> Result<SideEffectIntent, StoreError> {
        let json = self.connection.query_row(
            "SELECT json FROM side_effect_intents WHERE id = ?1",
            params![intent_id.as_str()],
            |row| row.get::<_, String>(0),
        )?;
        serde_json::from_str(&json).map_err(StoreError::from)
    }

    fn visible_json_rows(
        &self,
        table: &'static str,
        context: &AuthContext,
    ) -> Result<Vec<String>, StoreError> {
        let sql = match table {
            "fast_captures" => {
                "SELECT json FROM fast_captures
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY id"
            }
            "semantic_memories" => {
                "SELECT json FROM semantic_memories
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY id"
            }
            "permission_requests" => {
                "SELECT json FROM permission_requests
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY id"
            }
            "permission_tickets" => {
                "SELECT json FROM permission_tickets
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY request_id"
            }
            "delivery_records" => {
                "SELECT json FROM delivery_records
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY delivery_id, recipient_client_id"
            }
            "usage_records" => {
                "SELECT json FROM usage_records
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY turn_id"
            }
            "side_effect_intents" => {
                "SELECT json FROM side_effect_intents
                 WHERE tenant_id = ?1
                   AND (actor_id = ?2 OR visibility IN ('tenant_shared', 'public'))
                 ORDER BY id"
            }
            "side_effect_records" => {
                "SELECT records.json FROM side_effect_records records
                 JOIN side_effect_intents intents ON intents.id = records.intent_id
                 WHERE records.tenant_id = ?1
                   AND (records.actor_id = ?2 OR records.visibility IN ('tenant_shared', 'public'))
                 ORDER BY records.intent_id"
            }
            other => return Err(StoreError::InvalidVisibility(other.to_string())),
        };
        let mut statement = self.connection.prepare(sql)?;
        let rows = statement.query_map(
            params![context.tenant_id.as_str(), context.actor_id.as_str()],
            |row| row.get::<_, String>(0),
        )?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StoreError::from)
    }
}

impl DbWriter {
    /// # Errors
    /// Returns an error when the `SQLite` store cannot be opened or the writer
    /// thread cannot report startup.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbWriterError> {
        let path = path.as_ref().to_path_buf();
        let (sender, receiver) = mpsc::channel::<DbMessage>();
        let (ready_sender, ready_receiver) = mpsc::channel::<Result<(), StoreError>>();
        let handle = thread::spawn(move || {
            let store = match SqliteStore::open(path) {
                Ok(store) => store,
                Err(error) => {
                    let _ = ready_sender.send(Err(error));
                    return;
                }
            };
            let _ = ready_sender.send(Ok(()));
            while let Ok(message) = receiver.recv() {
                match message {
                    DbMessage::Write(job) => job(&store),
                    DbMessage::Shutdown => break,
                }
            }
        });
        ready_receiver
            .recv()
            .map_err(|_| DbWriterError::Receive)??;
        Ok(Self {
            sender,
            handle: Some(handle),
        })
    }

    /// # Errors
    /// Returns an error when the writer thread is unavailable or the operation
    /// returns a store error.
    pub fn write<R>(
        &self,
        operation: impl FnOnce(&SqliteStore) -> Result<R, StoreError> + Send + 'static,
    ) -> Result<R, DbWriterError>
    where
        R: Send + 'static,
    {
        let (result_sender, result_receiver) = mpsc::channel();
        self.sender
            .send(DbMessage::Write(Box::new(move |store| {
                let _ = result_sender.send(operation(store));
            })))
            .map_err(|_| DbWriterError::Send)?;
        result_receiver
            .recv()
            .map_err(|_| DbWriterError::Receive)?
            .map_err(DbWriterError::from)
    }
}

impl Drop for DbWriter {
    fn drop(&mut self) {
        let _ = self.sender.send(DbMessage::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

trait VisibleMemory<T> {
    fn filter_visible(self, context: &AuthContext) -> Result<Vec<T>, StoreError>;
}

impl<I> VisibleMemory<FastCapture> for I
where
    I: Iterator<Item = Result<FastCapture, StoreError>>,
{
    fn filter_visible(self, context: &AuthContext) -> Result<Vec<FastCapture>, StoreError> {
        let mut captures = Vec::new();
        for capture in self {
            let capture = capture?;
            if capture.scope.is_visible_to(context) {
                captures.push(capture);
            }
        }
        Ok(captures)
    }
}

impl<I> VisibleMemory<SemanticMemory> for I
where
    I: Iterator<Item = Result<SemanticMemory, StoreError>>,
{
    fn filter_visible(self, context: &AuthContext) -> Result<Vec<SemanticMemory>, StoreError> {
        let mut memories = Vec::new();
        for memory in self {
            let memory = memory?;
            if memory.scope.is_visible_to(context) {
                memories.push(memory);
            }
        }
        Ok(memories)
    }
}

#[derive(Debug, Deserialize)]
struct LegacySessionMetadata {
    id: SessionId,
    #[serde(default = "default_legacy_owner")]
    owner_actor: String,
}

fn default_legacy_owner() -> String {
    "local:default".to_string()
}

fn legacy_owner_to_scope_parts(
    owner_actor: &str,
    fallback_client_id: &ClientId,
) -> (ActorId, ClientId) {
    let actor_id = ActorId::from_raw(owner_actor.to_string());
    let client_id = if owner_actor.contains(':') {
        ClientId::from_raw(owner_actor.to_string())
    } else {
        fallback_client_id.clone()
    };
    (actor_id, client_id)
}

fn session_from_row(row: &rusqlite::Row<'_>) -> Result<SessionRecord, rusqlite::Error> {
    let visibility_text = row.get::<_, String>(4)?;
    let visibility = parse_visibility(&visibility_text).map_err(|error| match error {
        StoreError::InvalidVisibility(value) => rusqlite::Error::InvalidParameterName(value),
        StoreError::AccessDenied
        | StoreError::CountOutOfRange(_)
        | StoreError::Io(_)
        | StoreError::Json(_)
        | StoreError::Permission(_)
        | StoreError::Sqlite(_)
        | StoreError::ValueOutOfRange(_) => {
            rusqlite::Error::InvalidParameterName("visibility".to_string())
        }
    })?;
    let client_id = row.get::<_, Option<String>>(3)?.map(ClientId::from_raw);
    Ok(SessionRecord {
        session_id: SessionId::from_raw(row.get::<_, String>(0)?),
        scope: OwnedScope::new(
            TenantId::from_raw(row.get::<_, String>(1)?),
            ActorId::from_raw(row.get::<_, String>(2)?),
            client_id,
            visibility,
        ),
    })
}

const fn visibility_label(visibility: Visibility) -> &'static str {
    match visibility {
        Visibility::Private => "private",
        Visibility::ActorShared => "actor_shared",
        Visibility::TenantShared => "tenant_shared",
        Visibility::Public => "public",
    }
}

fn parse_visibility(value: &str) -> Result<Visibility, StoreError> {
    match value {
        "private" => Ok(Visibility::Private),
        "actor_shared" => Ok(Visibility::ActorShared),
        "tenant_shared" => Ok(Visibility::TenantShared),
        "public" => Ok(Visibility::Public),
        other => Err(StoreError::InvalidVisibility(other.to_string())),
    }
}

const fn decision_label(decision: PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow => "allow",
        PermissionDecision::RequireConfirmation => "require_confirmation",
        PermissionDecision::Deny => "deny",
    }
}

const fn permission_status_label(status: PermissionStatus) -> &'static str {
    match status {
        PermissionStatus::Pending => "pending",
        PermissionStatus::Approved => "approved",
        PermissionStatus::Denied => "denied",
        PermissionStatus::TimedOut => "timed_out",
        PermissionStatus::Cancelled => "cancelled",
        PermissionStatus::Superseded => "superseded",
    }
}

const fn delivery_status_label(status: DeliveryStatus) -> &'static str {
    match status {
        DeliveryStatus::Planned => "planned",
        DeliveryStatus::Sent => "sent",
        DeliveryStatus::Failed => "failed",
        DeliveryStatus::Acknowledged => "acknowledged",
    }
}

const fn side_effect_kind_label(kind: SideEffectKind) -> &'static str {
    match kind {
        SideEffectKind::ModelCall => "model_call",
        SideEffectKind::ToolCall => "tool_call",
        SideEffectKind::EmbeddingCall => "embedding_call",
        SideEffectKind::DeliverySend => "delivery_send",
        SideEffectKind::ExternalIo => "external_io",
    }
}

const fn side_effect_status_label(status: SideEffectStatus) -> &'static str {
    match status {
        SideEffectStatus::Succeeded => "succeeded",
        SideEffectStatus::Failed => "failed",
    }
}

fn u64_to_i64(value: u64, name: &'static str) -> Result<i64, StoreError> {
    i64::try_from(value).map_err(|_| StoreError::ValueOutOfRange(name))
}

fn i64_to_u64(value: i64) -> Result<u64, StoreError> {
    u64::try_from(value).map_err(|_| StoreError::ValueOutOfRange("pragma"))
}

fn synchronous_label(value: i64) -> String {
    match value {
        0 => "off",
        1 => "normal",
        2 => "full",
        3 => "extra",
        _ => "unknown",
    }
    .to_string()
}
