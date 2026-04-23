use cortex_kernel::{Journal, SessionStore, project_message_history};
use cortex_types::{CorrelationId, Event, Message, Payload, SessionId, SessionMetadata, TurnId};

/// Error returned by session management operations.
#[derive(Debug)]
pub enum SessionError {
    /// No session matches the given prefix.
    NotFound(String),
    /// Multiple sessions match the prefix — ambiguous.
    Ambiguous(Vec<String>),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(prefix) => write!(f, "no session matching prefix '{prefix}'"),
            Self::Ambiguous(matches) => {
                write!(f, "ambiguous prefix, matches: ")?;
                for (i, m) in matches.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{m}")?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for SessionError {}

/// Manages session lifecycle: create, end, list, switch (resume).
///
/// Wraps the kernel-level `Journal` and `SessionStore` with
/// higher-level semantics (event emission, history restoration).
pub struct SessionManager<'a> {
    journal: &'a Journal,
    session_store: &'a SessionStore,
}

impl<'a> SessionManager<'a> {
    /// Create a new session manager referencing the given stores.
    #[must_use]
    pub const fn new(journal: &'a Journal, session_store: &'a SessionStore) -> Self {
        Self {
            journal,
            session_store,
        }
    }

    /// Create a new session, emit `SessionStarted`, persist metadata.
    ///
    /// Returns `(session_id, metadata)`.
    #[must_use]
    pub fn create_session(&self) -> (SessionId, SessionMetadata) {
        self.create_session_for_actor("local:default")
    }

    /// Create a new session owned by the given actor.
    #[must_use]
    pub fn create_session_for_actor(&self, owner_actor: &str) -> (SessionId, SessionMetadata) {
        self.create_session_inner(SessionId::new(), owner_actor)
    }

    /// Create a session with a user-chosen ID.
    ///
    /// If the ID is a valid UUID, it is used directly as the `SessionId`.
    /// Otherwise, a new UUID is generated and the user string is stored
    /// as the session `name`, which is returned in API responses.
    #[must_use]
    pub fn create_session_with_id(&self, id_str: &str) -> (SessionId, SessionMetadata) {
        self.create_session_with_id_for_actor(id_str, "local:default")
    }

    /// Create a session with a user-chosen ID owned by the given actor.
    #[must_use]
    pub fn create_session_with_id_for_actor(
        &self,
        id_str: &str,
        owner_actor: &str,
    ) -> (SessionId, SessionMetadata) {
        id_str.parse().map_or_else(
            |_| {
                let (sid, mut meta) = self.create_session_inner(SessionId::new(), owner_actor);
                meta.name = Some(id_str.to_string());
                let _ = self.session_store.save(&meta);
                (sid, meta)
            },
            |sid| self.create_session_inner(sid, owner_actor),
        )
    }

    fn create_session_inner(
        &self,
        session_id: SessionId,
        owner_actor: &str,
    ) -> (SessionId, SessionMetadata) {
        let event = Event::new(
            TurnId::new(),
            CorrelationId::new(),
            Payload::SessionStarted {
                session_id: session_id.to_string(),
            },
        );
        let offset = self.journal.append(&event).unwrap_or(0);
        let mut meta = SessionMetadata::new(session_id, offset);
        meta.owner_actor = owner_actor.to_string();
        let _ = self.session_store.save(&meta);
        (session_id, meta)
    }

    /// End the given session, emit `SessionEnded`, update metadata.
    pub fn end_session(&self, meta: &mut SessionMetadata, turn_count: usize) {
        let event = Event::new(
            TurnId::new(),
            CorrelationId::new(),
            Payload::SessionEnded {
                session_id: meta.id.to_string(),
            },
        );
        let offset = self.journal.append(&event).unwrap_or(0);
        meta.ended_at = Some(chrono::Utc::now());
        meta.end_offset = Some(offset);
        meta.turn_count = turn_count;
        let _ = self.session_store.save(meta);
    }

    /// List all persisted sessions.
    #[must_use]
    pub fn list_sessions(&self) -> Vec<SessionMetadata> {
        self.session_store.list()
    }

    /// Switch to a session by ID prefix.
    ///
    /// Ends the current session, restores message history from the target
    /// session, and creates a new session with that history.
    ///
    /// # Errors
    ///
    /// Returns [`SessionError::NotFound`] if no session matches,
    /// or [`SessionError::Ambiguous`] if multiple sessions match.
    pub fn resume_session(
        &self,
        prefix: &str,
        current_meta: &mut SessionMetadata,
        current_turn_count: usize,
    ) -> Result<ResumedSession, SessionError> {
        let sessions = self.session_store.list();
        let matched: Vec<&SessionMetadata> = sessions
            .iter()
            .filter(|s| s.id.to_string().starts_with(prefix))
            .collect();

        match matched.len() {
            0 => Err(SessionError::NotFound(prefix.to_string())),
            1 => {
                let target = matched[0];
                // End current session
                self.end_session(current_meta, current_turn_count);

                // Restore history from target
                let start = target.start_offset;
                let end = target.end_offset.unwrap_or(u64::MAX);
                let events = self
                    .journal
                    .events_in_range(start, end + 1)
                    .unwrap_or_default();
                let history = project_message_history(&events);
                let resume_packet = cortex_turn::orchestrator::resume::build_resume_packet(&events);

                // Create new session
                let (new_id, new_meta) = self.create_session_for_actor(&current_meta.owner_actor);

                Ok(ResumedSession {
                    restored_from: target.id.to_string()[..8].to_string(),
                    new_session_id: new_id,
                    new_meta,
                    history,
                    resume_packet,
                    message_count: events.len(),
                })
            }
            _ => {
                let ids = matched
                    .iter()
                    .map(|s| s.id.to_string()[..8].to_string())
                    .collect();
                Err(SessionError::Ambiguous(ids))
            }
        }
    }
}

/// Result of a successful session switch operation.
#[derive(Debug)]
pub struct ResumedSession {
    /// Short ID prefix of the session that was restored from.
    pub restored_from: String,
    /// The newly created session's ID.
    pub new_session_id: SessionId,
    /// The newly created session's metadata.
    pub new_meta: SessionMetadata,
    /// Restored message history.
    pub history: Vec<Message>,
    /// Resume packet built from the restored session's events.
    pub resume_packet: cortex_types::ResumePacket,
    /// Number of events in the restored range.
    pub message_count: usize,
}
