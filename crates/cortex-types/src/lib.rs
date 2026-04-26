#![forbid(unsafe_code)]

pub mod cognitive;
pub mod control;
pub mod deployment;
pub mod event;
pub mod id;
pub mod memory;
pub mod outbound;
pub mod ownership;
pub mod policy;
pub mod retrieval;
pub mod side_effect;
pub mod usage;
pub mod workspace;

pub use cognitive::{
    ConflictKind, ConflictSignal, ContextLoadItem, ControlGoal, ControlLevel, ExecutionTrace,
    GoalConflict, GoalGraph, GoalGraphError, GoalStatus, LoadClass, LoadProfile, LoadWeights,
    MonitoringRecord, MonitoringReport, MonitoringThresholds, PressureAction,
};
pub use control::{
    Accumulator, ControlDecision, ControlSignal, EvidenceSignal, ExpectedControlValue,
    ProductionCondition, ProductionContext, ProductionRule, ProductionSystem, TurnFrontier,
    TurnState, TurnTransitionError,
};
pub use deployment::{
    DeploymentArtifact, DeploymentError, DeploymentEvidence, DeploymentPlan, DeploymentRecord,
    DeploymentStatus, DeploymentStep,
};
pub use event::{Event, EventPayload};
pub use id::{
    ActorId, ClientId, CorpusId, DeliveryId, EventId, PermissionRequestId, SessionId, SideEffectId,
    TenantId, TurnId,
};
pub use memory::{
    ConsolidationDecision, ConsolidationJob, FastCapture, InterferenceReport, MemoryKind,
    OffloadedChunk, SemanticMemory, WorkingMemory, WorkingMemoryBudget, WorkingMemoryChunk,
    WorkingMemoryError,
};
pub use outbound::{
    DeliveryItem, DeliveryPhase, DeliveryPlan, DeliveryStatus, DeliveryTextMode, MediaKind,
    OutboundBlock, OutboundDeliveryRecord, OutboundMessage, TransportCapabilities,
};
pub use ownership::{AuthContext, OwnedScope, Visibility};
pub use policy::{
    ActionRisk, PermissionDecision, PermissionLifecycleError, PermissionRequest,
    PermissionResolution, PermissionResolutionError, PermissionStatus, PermissionTicket,
    PolicyMode,
};
pub use retrieval::{
    AccessClass, Evidence, EvidenceTaint, HybridScores, PlacementStrategy, QueryPlan,
    RetrievalDecision, decide, place,
};
pub use side_effect::{SideEffectIntent, SideEffectKind, SideEffectRecord, SideEffectStatus};
pub use usage::{TokenUsage, UsageRecord};
pub use workspace::{
    AdmissionError, BroadcastFrame, DroppedItem, Subscriber, WorkspaceBudget, WorkspaceItem,
    WorkspaceItemKind,
};
