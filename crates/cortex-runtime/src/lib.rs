#![forbid(unsafe_code)]

pub mod daemon;
pub mod ingress;
pub mod transport;

use std::collections::{BTreeMap, BTreeSet};

use cortex_kernel::{
    DbWriter, DbWriterError, FileJournal, JournalError, SqliteStore, StoreError, StoreHealth,
};
use cortex_retrieval::RetrievalEngine;
use cortex_types::{
    ActorId, AuthContext, ClientId, CorpusId, DeliveryId, DeliveryPlan, Event, EventPayload,
    OutboundDeliveryRecord, OutboundMessage, OwnedScope, QueryPlan, SessionId, TenantId,
    TransportCapabilities, TurnFrontier, TurnId, TurnState, TurnTransitionError, Visibility,
    WorkspaceBudget, WorkspaceItem, WorkspaceItemKind,
};
pub use daemon::{
    DaemonConfig, DaemonError, DaemonRequest, DaemonResponse, DaemonServer, DaemonStatus,
    SubmittedTurn, send_request,
};
pub use ingress::{AuthenticatedClient, IngressError, IngressRegistry};

#[derive(Debug)]
pub enum RuntimeError {
    Journal(JournalError),
    Store(StoreError),
    DbWriter(DbWriterError),
    Ingress(IngressError),
    AccessDenied,
    MissingClient,
    MissingSession,
    MissingTenant,
    TurnTransition(TurnTransitionError),
    WorkspaceAdmission,
}

impl From<JournalError> for RuntimeError {
    fn from(error: JournalError) -> Self {
        Self::Journal(error)
    }
}

impl From<StoreError> for RuntimeError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<DbWriterError> for RuntimeError {
    fn from(error: DbWriterError) -> Self {
        Self::DbWriter(error)
    }
}

impl From<IngressError> for RuntimeError {
    fn from(error: IngressError) -> Self {
        Self::Ingress(error)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TenantRecord {
    pub id: TenantId,
    pub name: String,
    pub actors: BTreeSet<ActorId>,
    pub clients: BTreeSet<ClientId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientBinding {
    pub context: AuthContext,
    pub capabilities: TransportCapabilities,
    pub active_session: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryEnvelope {
    pub client_id: ClientId,
    pub delivery_id: DeliveryId,
    pub plan: DeliveryPlan,
}

pub struct CortexRuntime {
    tenants: BTreeMap<TenantId, TenantRecord>,
    clients: BTreeMap<ClientKey, ClientBinding>,
    sessions: BTreeMap<SessionId, OwnedScope>,
    journal: FileJournal,
    retrieval: RetrievalEngine,
    state: Option<DbWriter>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ClientKey {
    tenant: TenantId,
    actor: ActorId,
    client: ClientId,
}

impl ClientKey {
    fn from_context(context: &AuthContext) -> Self {
        Self {
            tenant: context.tenant_id.clone(),
            actor: context.actor_id.clone(),
            client: context.client_id.clone(),
        }
    }
}

impl CortexRuntime {
    /// # Errors
    /// Returns an error when the journal cannot be opened or replayed.
    pub fn open(journal_path: impl AsRef<std::path::Path>) -> Result<Self, RuntimeError> {
        let journal = FileJournal::open(journal_path)?;
        let mut runtime = Self {
            tenants: BTreeMap::new(),
            clients: BTreeMap::new(),
            sessions: BTreeMap::new(),
            journal,
            retrieval: RetrievalEngine::default(),
            state: None,
        };
        runtime.recover_from_journal()?;
        Ok(runtime)
    }

    /// # Errors
    /// Returns an error when the journal or state database cannot be opened,
    /// replayed, or synchronized.
    pub fn open_persistent(
        journal_path: impl AsRef<std::path::Path>,
        state_path: impl AsRef<std::path::Path>,
    ) -> Result<Self, RuntimeError> {
        let journal = FileJournal::open(journal_path)?;
        let state = DbWriter::open(state_path)?;
        let mut runtime = Self {
            tenants: BTreeMap::new(),
            clients: BTreeMap::new(),
            sessions: BTreeMap::new(),
            journal,
            retrieval: RetrievalEngine::default(),
            state: Some(state),
        };
        runtime.recover_from_journal()?;
        runtime.sync_state_store()?;
        Ok(runtime)
    }

    pub fn add_tenant(&mut self, id: TenantId, name: impl Into<String>) {
        self.insert_tenant(id, name.into());
    }

    #[must_use]
    pub fn tenant_count(&self) -> usize {
        self.tenants.len()
    }

    #[must_use]
    pub fn client_binding_count(&self) -> usize {
        self.clients.len()
    }

    #[must_use]
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }

    #[must_use]
    pub const fn is_persistent(&self) -> bool {
        self.state.is_some()
    }

    /// # Errors
    /// Returns an error when the registration cannot be journaled.
    pub fn register_tenant(
        &mut self,
        id: &TenantId,
        name: impl Into<String>,
    ) -> Result<(), RuntimeError> {
        let name = name.into();
        self.insert_tenant(id.clone(), name.clone());
        self.journal.append(&Event::new(
            OwnedScope::new(
                id.clone(),
                ActorId::from_static("tenant-admin"),
                None,
                Visibility::TenantShared,
            ),
            EventPayload::TenantRegistered {
                tenant_id: id.clone(),
                name: name.clone(),
            },
        ))?;
        if let Some(state) = &self.state {
            let tenant_id = id.clone();
            let tenant_name = name;
            state.write(move |store| store.upsert_tenant(&tenant_id, &tenant_name))?;
        }
        Ok(())
    }

    fn insert_tenant(&mut self, id: TenantId, name: String) {
        self.tenants.insert(
            id.clone(),
            TenantRecord {
                id,
                name,
                actors: BTreeSet::new(),
                clients: BTreeSet::new(),
            },
        );
    }

    /// # Errors
    /// Returns an error when the tenant is unknown or journaling fails.
    pub fn bind_client(
        &mut self,
        context: &AuthContext,
        capabilities: TransportCapabilities,
    ) -> Result<(), RuntimeError> {
        self.ensure_tenant(context)?;
        let Some(tenant) = self.tenants.get_mut(&context.tenant_id) else {
            return Err(RuntimeError::MissingTenant);
        };
        tenant.actors.insert(context.actor_id.clone());
        tenant.clients.insert(context.client_id.clone());

        self.clients.insert(
            ClientKey::from_context(context),
            ClientBinding {
                context: context.clone(),
                capabilities: capabilities.clone(),
                active_session: None,
            },
        );
        self.journal.append(&Event::new(
            OwnedScope::new(
                context.tenant_id.clone(),
                context.actor_id.clone(),
                Some(context.client_id.clone()),
                Visibility::ActorShared,
            ),
            EventPayload::ClientBound {
                client_id: context.client_id.clone(),
                capabilities,
            },
        ))?;
        if let Some(state) = &self.state {
            let context = context.clone();
            let capabilities = binding_capabilities(self, &context);
            state.write(move |store| store.upsert_client(&context, &capabilities))?;
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error when ingress authentication fails, the tenant is
    /// unknown, or the authenticated client cannot be journaled.
    pub fn bind_authenticated_client(
        &mut self,
        registry: &IngressRegistry,
        tenant_id: &TenantId,
        actor_id: &ActorId,
        client_id: &ClientId,
        token: &str,
    ) -> Result<(), RuntimeError> {
        let authenticated = registry.authenticate(tenant_id, actor_id, client_id, token)?;
        self.bind_client(&authenticated.context, authenticated.capabilities)
    }

    /// # Errors
    /// Returns an error when the tenant is unknown or journaling fails.
    pub fn create_session(&mut self, context: &AuthContext) -> Result<SessionId, RuntimeError> {
        self.ensure_tenant(context)?;
        let session_id = SessionId::new();
        let scope = OwnedScope::new(
            context.tenant_id.clone(),
            context.actor_id.clone(),
            Some(context.client_id.clone()),
            Visibility::ActorShared,
        );
        self.journal.append(&Event::new(
            scope.clone(),
            EventPayload::SessionCreated {
                session_id: session_id.clone(),
            },
        ))?;
        self.sessions.insert(session_id.clone(), scope);
        if let Some(state) = &self.state {
            let session_id = session_id.clone();
            let scope = self.sessions[&session_id].clone();
            state.write(move |store| store.upsert_session(&session_id, &scope))?;
        }
        Ok(session_id)
    }

    /// # Errors
    /// Returns an error when the tenant, client binding, session, or journal is unavailable.
    pub fn activate_session(
        &mut self,
        context: &AuthContext,
        session_id: &SessionId,
    ) -> Result<(), RuntimeError> {
        let scope = self.authorize_session(context, session_id)?;
        let key = ClientKey::from_context(context);
        let Some(binding) = self.clients.get_mut(&key) else {
            return Err(RuntimeError::MissingClient);
        };
        binding.active_session = Some(session_id.clone());
        self.journal.append(&Event::new(
            scope,
            EventPayload::SessionActivated {
                session_id: session_id.clone(),
                client_id: context.client_id.clone(),
            },
        ))?;
        if let Some(state) = &self.state {
            let context = context.clone();
            let session_id = session_id.clone();
            state.write(move |store| store.set_active_session(&context, &session_id))?;
        }
        Ok(())
    }

    /// # Errors
    /// Returns an error when tenant, client, session, or journal state cannot be used.
    pub fn ensure_session_for_turn(
        &mut self,
        context: &AuthContext,
    ) -> Result<SessionId, RuntimeError> {
        self.ensure_tenant(context)?;
        if !self.clients.contains_key(&ClientKey::from_context(context)) {
            self.bind_client(context, TransportCapabilities::plain(4_096))?;
        }
        if let Some(active_session) = self.active_session(context)? {
            return Ok(active_session);
        }
        if let Some(session_id) = self.reusable_session_for(context) {
            self.activate_session(context, &session_id)?;
            return Ok(session_id);
        }
        let session_id = self.create_session(context)?;
        self.activate_session(context, &session_id)?;
        Ok(session_id)
    }

    /// # Errors
    /// Returns an error when the turn cannot be scoped, admitted to the
    /// workspace, or written to the journal.
    pub fn submit_user_message(
        &mut self,
        context: &AuthContext,
        input: &str,
    ) -> Result<SubmittedTurn, RuntimeError> {
        let session_id = self.ensure_session_for_turn(context)?;
        let scope = self.authorize_session(context, &session_id)?;
        let turn_id = TurnId::new();
        let mut frontier = TurnFrontier::new(
            turn_id.clone(),
            session_id.clone(),
            env!("CARGO_PKG_VERSION"),
        );
        self.journal.append(&Event::new(
            scope.clone(),
            EventPayload::TurnStarted {
                turn_id: turn_id.clone(),
                session_id: session_id.clone(),
            },
        ))?;
        let from = frontier.state;
        frontier
            .transition(TurnState::Processing)
            .map_err(RuntimeError::TurnTransition)?;
        self.journal.append(&Event::new(
            scope.clone(),
            EventPayload::TurnTransitioned {
                turn_id: turn_id.clone(),
                from,
                to: frontier.state,
                execution_version: frontier.execution_version,
            },
        ))?;

        let mut frame = cortex_types::BroadcastFrame::new(scope, Self::default_workspace_budget());
        let item = WorkspaceItem::new(
            format!("{}:user-input", turn_id.as_str()),
            OwnedScope::private_for(context),
            WorkspaceItemKind::UserInput,
            input,
        )
        .with_scores(0.8, 0.9)
        .with_tokens(input.chars().count() / 4 + 1);
        frame
            .admit(item)
            .map_err(|_| RuntimeError::WorkspaceAdmission)?;
        self.journal.append(&Event::new(
            OwnedScope::new(
                context.tenant_id.clone(),
                context.actor_id.clone(),
                Some(context.client_id.clone()),
                Visibility::ActorShared,
            ),
            EventPayload::WorkspaceBroadcast {
                frame: Box::new(frame),
            },
        ))?;

        Ok(SubmittedTurn {
            session_id,
            turn_id,
        })
    }

    /// # Errors
    /// Returns an error when the tenant is unknown.
    pub fn active_session(&self, context: &AuthContext) -> Result<Option<SessionId>, RuntimeError> {
        self.ensure_tenant(context)?;
        let Some(binding) = self.clients.get(&ClientKey::from_context(context)) else {
            return Ok(None);
        };
        Ok(binding
            .active_session
            .as_ref()
            .filter(|session_id| {
                self.sessions
                    .get(*session_id)
                    .is_some_and(|scope| scope.is_visible_to(context))
            })
            .cloned())
    }

    /// # Errors
    /// Returns an error when the session is unknown or journaling fails.
    pub fn deliver_to_active_subscribers(
        &self,
        session_id: &SessionId,
        message: &OutboundMessage,
    ) -> Result<Vec<DeliveryEnvelope>, RuntimeError> {
        let Some(session_scope) = self.sessions.get(session_id) else {
            return Err(RuntimeError::MissingSession);
        };
        let mut envelopes = Vec::new();
        for binding in self.clients.values() {
            if binding.active_session.as_ref() != Some(session_id)
                || !session_scope.is_visible_to(&binding.context)
                || !message.scope.is_visible_to(&binding.context)
            {
                continue;
            }
            let plan = message.plan(&binding.capabilities);
            if let Some(state) = &self.state {
                let record = OutboundDeliveryRecord::planned(
                    message.id.clone(),
                    session_id.clone(),
                    &binding.context,
                    plan.clone(),
                );
                state.write(move |store| store.save_delivery_record(&record))?;
            }
            let envelope = DeliveryEnvelope {
                client_id: binding.context.client_id.clone(),
                delivery_id: message.id.clone(),
                plan,
            };
            self.journal.append(&Event::new(
                OwnedScope::new(
                    binding.context.tenant_id.clone(),
                    binding.context.actor_id.clone(),
                    Some(binding.context.client_id.clone()),
                    Visibility::Private,
                ),
                EventPayload::DeliveryPlanned {
                    delivery_id: envelope.delivery_id.clone(),
                    session_id: session_id.clone(),
                    recipient_client_id: envelope.client_id.clone(),
                },
            ))?;
            envelopes.push(envelope);
        }
        Ok(envelopes)
    }

    /// # Errors
    /// Returns an error when replay fails.
    pub fn visible_events(&self, context: &AuthContext) -> Result<Vec<Event>, RuntimeError> {
        Ok(self.journal.replay_visible(context)?)
    }

    #[must_use]
    pub fn known_clients(&self, tenant_id: &TenantId) -> usize {
        self.tenants
            .get(tenant_id)
            .map_or(0, |tenant| tenant.clients.len())
    }

    /// # Errors
    /// Returns an error when this runtime has no persistent state store or the
    /// state query fails.
    pub fn persisted_client_count(&self, tenant_id: &TenantId) -> Result<usize, RuntimeError> {
        let Some(state) = &self.state else {
            return Err(RuntimeError::MissingTenant);
        };
        let tenant_id = tenant_id.clone();
        Ok(state.write(move |store| store.client_count(&tenant_id))?)
    }

    /// # Errors
    /// Returns an error when the persistent state store is present but cannot
    /// report its health.
    pub fn store_health(&self) -> Result<Option<StoreHealth>, RuntimeError> {
        let Some(state) = &self.state else {
            return Ok(None);
        };
        Ok(Some(state.write(SqliteStore::health)?))
    }

    #[must_use]
    pub const fn retrieval(&self) -> &RetrievalEngine {
        &self.retrieval
    }

    #[must_use]
    pub fn default_query(context: &AuthContext, query: impl Into<String>) -> QueryPlan {
        QueryPlan {
            query: query.into(),
            scope: OwnedScope::new(
                context.tenant_id.clone(),
                context.actor_id.clone(),
                Some(context.client_id.clone()),
                Visibility::ActorShared,
            ),
            corpus_id: CorpusId::new(),
            active_retrieval: true,
            query_embedding: None,
        }
    }

    #[must_use]
    pub fn default_workspace_budget() -> WorkspaceBudget {
        WorkspaceBudget::default()
    }

    fn ensure_tenant(&self, context: &AuthContext) -> Result<(), RuntimeError> {
        if self.tenants.contains_key(&context.tenant_id) {
            Ok(())
        } else {
            Err(RuntimeError::MissingTenant)
        }
    }

    fn authorize_session(
        &self,
        context: &AuthContext,
        session_id: &SessionId,
    ) -> Result<OwnedScope, RuntimeError> {
        self.ensure_tenant(context)?;
        let Some(scope) = self.sessions.get(session_id) else {
            return Err(RuntimeError::MissingSession);
        };
        if scope.is_visible_to(context) {
            Ok(scope.clone())
        } else {
            Err(RuntimeError::AccessDenied)
        }
    }

    fn reusable_session_for(&self, context: &AuthContext) -> Option<SessionId> {
        self.sessions
            .iter()
            .find(|(_, scope)| {
                scope.visibility == Visibility::ActorShared && scope.is_visible_to(context)
            })
            .map(|(session_id, _)| session_id.clone())
    }

    fn recover_from_journal(&mut self) -> Result<(), RuntimeError> {
        for event in self.journal.replay_all()? {
            self.apply_replayed_event(event);
        }
        Ok(())
    }

    fn apply_replayed_event(&mut self, event: Event) {
        match event.payload {
            EventPayload::TenantRegistered { tenant_id, name } => {
                self.insert_tenant(tenant_id, name);
            }
            EventPayload::ClientBound {
                client_id,
                capabilities,
            } => {
                if self.tenants.contains_key(&event.scope.tenant_id) {
                    let context = AuthContext::new(
                        event.scope.tenant_id.clone(),
                        event.scope.actor_id.clone(),
                        client_id,
                    );
                    let Some(tenant) = self.tenants.get_mut(&context.tenant_id) else {
                        return;
                    };
                    tenant.actors.insert(context.actor_id.clone());
                    tenant.clients.insert(context.client_id.clone());
                    self.clients.insert(
                        ClientKey::from_context(&context),
                        ClientBinding {
                            context,
                            capabilities,
                            active_session: None,
                        },
                    );
                }
            }
            EventPayload::SessionCreated { session_id } => {
                if self.tenants.contains_key(&event.scope.tenant_id) {
                    self.sessions.insert(session_id, event.scope);
                }
            }
            EventPayload::SessionActivated {
                session_id,
                client_id,
            } => {
                let context = AuthContext::new(
                    event.scope.tenant_id.clone(),
                    event.scope.actor_id.clone(),
                    client_id,
                );
                if self.sessions.contains_key(&session_id)
                    && let Some(binding) = self.clients.get_mut(&ClientKey::from_context(&context))
                {
                    binding.active_session = Some(session_id);
                }
            }
            EventPayload::TurnStarted { .. }
            | EventPayload::TurnTransitioned { .. }
            | EventPayload::WorkspaceBroadcast { .. }
            | EventPayload::DeliveryPlanned { .. }
            | EventPayload::PermissionRequested { .. }
            | EventPayload::SideEffectIntended { .. }
            | EventPayload::SideEffectRecorded { .. }
            | EventPayload::AccessDenied { .. } => {}
        }
    }

    fn sync_state_store(&self) -> Result<(), RuntimeError> {
        let Some(state) = &self.state else {
            return Ok(());
        };
        for tenant in self.tenants.values() {
            let tenant_id = tenant.id.clone();
            let tenant_name = tenant.name.clone();
            state.write(move |store| store.upsert_tenant(&tenant_id, &tenant_name))?;
        }
        for binding in self.clients.values() {
            let context = binding.context.clone();
            let capabilities = binding.capabilities.clone();
            state.write(move |store| store.upsert_client(&context, &capabilities))?;
        }
        for (session_id, scope) in &self.sessions {
            let session_id = session_id.clone();
            let scope = scope.clone();
            state.write(move |store| store.upsert_session(&session_id, &scope))?;
        }
        for binding in self.clients.values() {
            if let Some(session_id) = &binding.active_session {
                let context = binding.context.clone();
                let session_id = session_id.clone();
                state.write(move |store| store.set_active_session(&context, &session_id))?;
            }
        }
        Ok(())
    }
}

fn binding_capabilities(runtime: &CortexRuntime, context: &AuthContext) -> TransportCapabilities {
    runtime
        .clients
        .get(&ClientKey::from_context(context))
        .map_or_else(
            || TransportCapabilities::plain(4_096),
            |binding| binding.capabilities.clone(),
        )
}
