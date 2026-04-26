use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::{
    BroadcastFrame, ClientId, DeliveryId, EventId, OwnedScope, PermissionRequestId, SessionId,
    TenantId, TransportCapabilities, TurnId,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub scope: OwnedScope,
    pub payload: EventPayload,
    pub recorded_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    TenantRegistered {
        tenant_id: TenantId,
        name: String,
    },
    SessionCreated {
        session_id: SessionId,
    },
    ClientBound {
        client_id: ClientId,
        capabilities: TransportCapabilities,
    },
    SessionActivated {
        session_id: SessionId,
        client_id: ClientId,
    },
    TurnStarted {
        turn_id: TurnId,
        session_id: SessionId,
    },
    WorkspaceBroadcast {
        frame: Box<BroadcastFrame>,
    },
    DeliveryPlanned {
        delivery_id: DeliveryId,
        session_id: SessionId,
        recipient_client_id: ClientId,
    },
    PermissionRequested {
        request_id: PermissionRequestId,
    },
    AccessDenied {
        reason: String,
    },
}

impl Event {
    #[must_use]
    pub fn new(scope: OwnedScope, payload: EventPayload) -> Self {
        Self {
            id: EventId::new(),
            scope,
            payload,
            recorded_at: Utc::now(),
        }
    }
}
