#![forbid(unsafe_code)]

pub mod control;
pub mod deployment;
pub mod event;
pub mod id;
pub mod memory;
pub mod outbound;
pub mod ownership;
pub mod policy;
pub mod retrieval;
pub mod usage;
pub mod workspace;

pub use control::{
    Accumulator, ControlDecision, ControlSignal, EvidenceSignal, ExpectedControlValue,
};
pub use deployment::{
    DeploymentArtifact, DeploymentError, DeploymentEvidence, DeploymentPlan, DeploymentRecord,
    DeploymentStatus, DeploymentStep,
};
pub use event::{Event, EventPayload};
pub use id::{
    ActorId, ClientId, CorpusId, DeliveryId, EventId, PermissionRequestId, SessionId, TenantId,
    TurnId,
};
pub use memory::{
    ConsolidationDecision, ConsolidationJob, FastCapture, InterferenceReport, MemoryKind,
    SemanticMemory,
};
pub use outbound::{
    DeliveryItem, DeliveryPhase, DeliveryPlan, DeliveryStatus, DeliveryTextMode, MediaKind,
    OutboundBlock, OutboundDeliveryRecord, OutboundMessage, TransportCapabilities,
};
pub use ownership::{AuthContext, OwnedScope, Visibility};
pub use policy::{
    ActionRisk, PermissionDecision, PermissionRequest, PermissionResolution,
    PermissionResolutionError, PolicyMode,
};
pub use retrieval::{
    AccessClass, Evidence, EvidenceTaint, HybridScores, PlacementStrategy, QueryPlan,
    RetrievalDecision, decide, place,
};
pub use usage::{TokenUsage, UsageRecord};
pub use workspace::{
    AdmissionError, BroadcastFrame, DroppedItem, Subscriber, WorkspaceBudget, WorkspaceItem,
    WorkspaceItemKind,
};
