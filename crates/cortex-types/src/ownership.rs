use serde::{Deserialize, Serialize};

use crate::{ActorId, ClientId, TenantId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    Private,
    ActorShared,
    TenantShared,
    Public,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnedScope {
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client_id: Option<ClientId>,
    pub visibility: Visibility,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthContext {
    pub tenant_id: TenantId,
    pub actor_id: ActorId,
    pub client_id: ClientId,
}

impl OwnedScope {
    #[must_use]
    pub const fn new(
        tenant_id: TenantId,
        actor_id: ActorId,
        client_id: Option<ClientId>,
        visibility: Visibility,
    ) -> Self {
        Self {
            tenant_id,
            actor_id,
            client_id,
            visibility,
        }
    }

    #[must_use]
    pub fn private_for(context: &AuthContext) -> Self {
        Self::new(
            context.tenant_id.clone(),
            context.actor_id.clone(),
            Some(context.client_id.clone()),
            Visibility::Private,
        )
    }

    #[must_use]
    pub fn is_visible_to(&self, context: &AuthContext) -> bool {
        if self.tenant_id != context.tenant_id {
            return self.visibility == Visibility::Public;
        }
        match self.visibility {
            Visibility::Private => {
                self.actor_id == context.actor_id
                    && self
                        .client_id
                        .as_ref()
                        .is_none_or(|id| id == &context.client_id)
            }
            Visibility::ActorShared => self.actor_id == context.actor_id,
            Visibility::TenantShared | Visibility::Public => true,
        }
    }
}

impl AuthContext {
    #[must_use]
    pub const fn new(tenant_id: TenantId, actor_id: ActorId, client_id: ClientId) -> Self {
        Self {
            tenant_id,
            actor_id,
            client_id,
        }
    }

    #[must_use]
    pub fn owns(&self, scope: &OwnedScope) -> bool {
        scope.tenant_id == self.tenant_id && scope.actor_id == self.actor_id
    }
}
