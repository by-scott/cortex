#![warn(clippy::pedantic, clippy::nursery)]

pub mod attention;
pub mod audit;
pub mod causal;
pub mod confidence;
pub mod config;
pub mod control;
pub mod event;
pub mod evolution;
pub mod goal;
pub mod id;
pub mod mcp;
pub mod memory;
pub mod message;
pub mod permission;
pub mod plugin;
pub mod prompt;
pub mod provenance;
pub mod reasoning;
pub mod resume;
pub mod retrieval;
pub mod session;
pub mod shared_task;
pub mod skills;
pub mod turn;
pub mod web;
pub mod working_memory;
pub mod workspace;

// Core IDs
pub use id::{CorrelationId, EventId, SessionId, TurnId};

// Event system
pub use event::{EXECUTION_VERSION, Event, Payload, SideEffectKind};

// Turn lifecycle
pub use turn::{TurnPhase, TurnState, TurnTransitionError};

// Messages
pub use message::{
    AssistantResponse, Attachment, ContentBlock, Message, ResponsePart, Role, TextFormat,
};

// Memory
pub use memory::{
    MemoryEntry, MemoryKind, MemoryRelation, MemorySource, MemoryStatus, MemoryStatusError,
    MemoryType, TrustLevel,
};

// Permission & risk
pub use permission::{
    ConfirmationCallback, ConfirmationRequest, ConfirmationResponse, DenyAllConfirmation,
    PermissionDecision, RiskLevel, RiskScore,
};

// Confidence
pub use confidence::ConfidenceLevel;

// Control
pub use control::{
    Conflict as ConflictSignal, Decision as ControlDecision, Impasse, ImpasseKind,
    Signal as ControlSignal, Subgoal,
};

// Reasoning
pub use reasoning::{
    EvidenceStrength, ReasoningChain, ReasoningMode, ReasoningStep, ReasoningStepType,
};

// Causal
pub use causal::{CausalChain, CausalLink, CausalRelation};

// Goals
pub use goal::{Goal, GoalLevel, GoalStack, GoalStatus};

// Session
pub use session::SessionMetadata;

// Shared tasks
pub use shared_task::{
    AggregationStrategy, SharedTask, SharedTaskStatus, SharedTaskTransitionError, TaskAssignment,
};

// Skills
pub use skills::{
    ExecutionMode, InvocationTrigger, SkillActivation, SkillInvocation, SkillMetadata,
    SkillParameter, SkillSource, SkillSummary,
};

// Prompt
pub use prompt::PromptLayer;

// Attention & working memory
pub use attention::AttentionChannel;
pub use working_memory::WorkingMemoryItem;

// Evolution
pub use evolution::{CheckResult, GateCheckResult, VerifyResult};

// Resume
pub use resume::ResumePacket;

// Retrieval
pub use retrieval::{
    AccessClass as EvidenceAccessClass, Decision as RetrievalDecision,
    DecisionKind as RetrievalDecisionKind, Evidence as EvidenceItem,
    QueryPlan as RetrievalQueryPlan, QueryTransform, QueryTransformKind, Scores as RetrievalScores,
    Stage as RetrievalStage, Taint as EvidenceTaint,
};

// Audit
pub use audit::{AuditSummary, AuditTimeRange, DecisionPath, DecisionPathStep};

// Trace
pub use config::TraceLevel;

// Plugin
pub use plugin::{
    NativeLibConfig, NativePluginIsolation, PluginCapabilities, PluginCompatibility,
    PluginManifest, PluginType, ProcessToolConfig, check_compatibility,
};

// Provenance
pub use provenance::{SourceProvenance, SourceTrust};

// Web/API types
pub use web::{
    ErrorBody, HealthResponse, MemorySearchRequest, OAuthCallbackParams, ResendRequest,
    SaveMemoryRequest, SessionCreateResponse, SessionInfoResponse, TokenRequest, TokenResponse,
    TurnEvent, TurnRequest,
};

// MCP
pub use mcp::MCP_PROTOCOL_VERSION;

// Workspace
pub use workspace::{
    Budget as WorkspaceBudget, Frame as WorkspaceFrame, FrameError, Item as WorkspaceItem,
    ItemKind as WorkspaceItemKind, Taint as WorkspaceTaint,
};
