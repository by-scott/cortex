use std::collections::{HashMap, HashSet};

use cortex_types::SessionMetadata;

use super::{BroadcastMessage, DaemonState};

impl DaemonState {
    fn save_client_sessions(&self) {
        let sessions = self
            .client_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Self::runtime_state_store(&self.data_dir).save_client_sessions(&sessions);
    }

    fn save_actor_sessions(&self) {
        let sessions = self
            .actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        Self::runtime_state_store(&self.data_dir).save_actor_sessions(&sessions);
    }

    #[must_use]
    pub(crate) const fn local_actor() -> &'static str {
        "local:default"
    }

    #[must_use]
    pub(crate) fn channel_actor(platform: &str, user_id: &str) -> String {
        format!("{platform}:{user_id}")
    }

    fn normalize_transport(transport: &str) -> &str {
        match transport {
            "sock" => "socket",
            other => other,
        }
    }

    pub(crate) fn transport_actor(&self, transport: &str) -> String {
        let transport = Self::normalize_transport(transport);
        self.transport_actors
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(transport)
            .cloned()
            .unwrap_or_else(|| Self::local_actor().to_string())
    }

    pub(super) fn canonical_actor(&self, actor: &str) -> String {
        let mut current = actor.to_string();
        let mut visited = HashSet::new();
        let actor_aliases = self
            .actor_aliases
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while let Some(next) = actor_aliases.get(&current) {
            if !visited.insert(current.clone()) {
                break;
            }
            current.clone_from(next);
        }
        current
    }

    pub(super) fn is_admin_actor(actor: &str) -> bool {
        actor == Self::local_actor()
    }

    pub(super) fn session_lookup(&self, session_id: &str) -> Option<SessionMetadata> {
        self.session_manager()
            .list_sessions()
            .into_iter()
            .find(|session| {
                session.id.to_string() == session_id || session.name.as_deref() == Some(session_id)
            })
    }

    pub(super) fn session_token_total(&self, session_id: Option<&str>) -> Option<u64> {
        let session_id = session_id?;
        let in_memory_tokens = self
            .sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(session_id)
            .map(|session| session.meta.total_tokens());
        in_memory_tokens.or_else(|| {
            self.session_lookup(session_id)
                .map(|session| session.total_tokens())
        })
    }

    pub(super) fn session_id_or_name_exists(&self, session_id: &str) -> bool {
        self.session_lookup(session_id).is_some()
            || self
                .sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(session_id)
    }

    pub(super) fn session_visible_to_actor(&self, actor: &str, session: &SessionMetadata) -> bool {
        let canonical = self.canonical_actor(actor);
        let owner = self.canonical_actor(&session.owner_actor);
        Self::is_admin_actor(&canonical) || owner == canonical
    }

    pub(crate) fn actor_can_access_session(&self, actor: &str, session_id: &str) -> bool {
        self.session_lookup(session_id)
            .is_some_and(|session| self.session_visible_to_actor(actor, &session))
    }

    pub(crate) fn transport_can_access_session(&self, transport: &str, session_id: &str) -> bool {
        let actor = self.transport_actor(transport);
        self.actor_can_access_session(&actor, session_id)
    }

    pub(crate) fn active_actor_session(&self, actor: &str) -> Option<String> {
        let actor_sessions = self
            .actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        actor_sessions.get(actor).cloned().filter(|session_id| {
            self.session_lookup(session_id).is_some_and(|session| {
                session.is_active() && self.session_visible_to_actor(actor, &session)
            })
        })
    }

    pub(crate) fn resolve_actor_session(&self, actor: &str) -> String {
        if let Some(existing) = self.active_actor_session(actor) {
            return existing;
        }

        let canonical = self.canonical_actor(actor);
        let linked_session = {
            let actor_sessions = self
                .actor_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let canonical_fallback = if actor == canonical {
                None
            } else {
                actor_sessions.get(&canonical).cloned()
            };
            let alias_fallback = actor_sessions.iter().find_map(|(bound_actor, session_id)| {
                if bound_actor == actor || bound_actor == &canonical {
                    return None;
                }
                (self.canonical_actor(bound_actor) == canonical).then(|| session_id.clone())
            });
            let linked_session =
                canonical_fallback
                    .into_iter()
                    .chain(alias_fallback)
                    .find(|session_id| {
                        self.session_lookup(session_id).is_some_and(|session| {
                            session.is_active() && self.session_visible_to_actor(actor, &session)
                        })
                    });
            drop(actor_sessions);
            linked_session
        };
        if let Some(existing) = linked_session {
            self.set_actor_session(actor, &existing);
            return existing;
        }

        if let Some(existing) = self
            .visible_sessions(&canonical)
            .into_iter()
            .filter(cortex_types::SessionMetadata::is_active)
            .max_by_key(|session| session.created_at)
            .map(|session| session.id.to_string())
        {
            self.set_actor_session(actor, &existing);
            return existing;
        }

        let (sid, _meta) = self.session_manager().create_session_for_actor(&canonical);
        let sid = sid.to_string();
        self.set_actor_session(actor, &sid);
        sid
    }

    pub(crate) fn set_actor_session(&self, actor: &str, session_id: &str) {
        self.actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(actor.to_string(), session_id.to_string());
        self.save_actor_sessions();
    }

    pub(crate) fn clear_actor_session(&self, actor: &str) {
        self.actor_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(actor);
        self.save_actor_sessions();
    }

    pub(crate) fn visible_sessions(&self, actor: &str) -> Vec<SessionMetadata> {
        let canonical = self.canonical_actor(actor);
        self.session_manager()
            .list_sessions()
            .into_iter()
            .filter(|session| self.session_visible_to_actor(&canonical, session))
            .collect()
    }

    pub(crate) fn visible_sessions_for_transport(&self, transport: &str) -> Vec<SessionMetadata> {
        let actor = self.transport_actor(transport);
        self.visible_sessions(&actor)
    }

    pub(crate) fn create_session_for_actor(&self, actor: &str) -> (String, SessionMetadata) {
        let canonical = self.canonical_actor(actor);
        let (sid, meta) = self.session_manager().create_session_for_actor(&canonical);
        let sid = sid.to_string();
        self.set_actor_session(actor, &sid);
        (sid, meta)
    }

    pub(super) fn active_session_bindings(&self) -> Vec<(String, Vec<String>)> {
        let mut bindings: HashMap<String, Vec<String>> = HashMap::new();

        {
            let client_sessions = self
                .client_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (client, session_id) in &*client_sessions {
                if !session_id.is_empty() && self.session_exists_and_active(session_id) {
                    bindings
                        .entry(session_id.clone())
                        .or_default()
                        .push(client.clone());
                }
            }
        }

        {
            let actor_sessions = self
                .actor_sessions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for (actor, session_id) in &*actor_sessions {
                if actor == Self::local_actor() {
                    continue;
                }
                if self.session_exists_and_active(session_id) {
                    bindings
                        .entry(session_id.clone())
                        .or_default()
                        .push(actor.clone());
                }
            }
        }

        let mut grouped: Vec<(String, Vec<String>)> = bindings
            .into_iter()
            .map(|(session_id, mut owners)| {
                owners.sort();
                (session_id, owners)
            })
            .collect();
        grouped.sort_by(|(left_id, left_owners), (right_id, right_owners)| {
            right_owners
                .len()
                .cmp(&left_owners.len())
                .then_with(|| left_id.cmp(right_id))
        });
        grouped
    }

    fn session_exists_and_active(&self, session_id: &str) -> bool {
        self.session_manager().list_sessions().into_iter().any(|s| {
            (s.id.to_string() == session_id || s.name.as_deref() == Some(session_id))
                && s.ended_at.is_none()
        })
    }

    pub(crate) fn resolve_client_session(&self, client: &str) -> String {
        let actor = self.transport_actor(client);
        let sid = self.resolve_actor_session(&actor);
        self.set_client_session(client, &sid);
        sid
    }

    pub(crate) fn set_client_session(&self, client: &str, session_id: &str) {
        let client = Self::normalize_transport(client);
        self.client_sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(client.to_string(), session_id.to_string());
        self.save_client_sessions();
    }

    pub(super) fn tracks_client_session(source: &str) -> bool {
        matches!(source, "rpc" | "http" | "ws" | "socket" | "sock" | "stdio")
    }

    /// Get or create a broadcast sender for a session.
    pub(crate) fn session_broadcast(
        &self,
        session_id: &str,
    ) -> tokio::sync::broadcast::Sender<BroadcastMessage> {
        let mut channels = self
            .session_channels
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        channels
            .entry(session_id.to_string())
            .or_insert_with(|| tokio::sync::broadcast::channel(64).0)
            .clone()
    }

    /// Subscribe to a session's event stream.
    pub(crate) fn subscribe_session(
        &self,
        session_id: &str,
    ) -> tokio::sync::broadcast::Receiver<BroadcastMessage> {
        self.session_broadcast(session_id).subscribe()
    }
}
