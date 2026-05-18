#![warn(clippy::pedantic, clippy::nursery)]

pub mod attention;
pub mod audit;
pub mod causal;
pub mod confidence;
pub mod config;
pub mod control;
pub mod event;
pub mod evolution;
pub mod feedback;
pub mod goal;
pub mod id;
pub mod mcp;
pub mod memory;
pub mod message;
pub mod model_routing;
pub mod permission;
pub mod plugin;
pub mod prompt;
pub mod provenance;
pub mod reasoning;
pub mod resume;
pub mod secret;
pub mod session;
pub mod shared_task;
pub mod skills;
pub mod tool_effect;
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
    AssistantResponse, Attachment, ContentBlock, MediaExternalPolicy, MediaMemoryPolicy,
    MediaPublishPolicy, MediaTaint, Message, ResponsePart, Role, TextFormat,
};

// Memory
pub use memory::{
    MemoryClaim, MemoryEntry, MemoryEvidence, MemoryKind, MemoryRelation, MemorySource,
    MemoryStatus, MemoryStatusError, MemoryType, MemoryUsageOutcome, MemoryUsageOutcomeKind,
    TrustLevel,
};

// Feedback
pub use feedback::{
    FeedbackAttribution, FeedbackKind, FeedbackReplayCheck, FeedbackTarget, classify_feedback_kind,
    classify_feedback_target,
};

// Goals
pub use goal::{Goal, GoalLevel, GoalSource, GoalStatus, GoalTransitionError};

// Permission & risk
pub use permission::{
    ConfirmationCallback, ConfirmationRequest, ConfirmationResponse, DenyAllConfirmation,
    PermissionDecision, RiskLevel, RiskScore,
};

// Tool effects
pub use tool_effect::{
    DryRunSupport, EffectConfirmation, EffectReversibility, ToolEffect, ToolEffectKind,
};

// Plugin governance
pub use plugin::{
    PluginConformanceCertificate, PluginConformanceCheck, PluginPackageMetadata,
    PluginSandboxLevel, PluginSandboxProfile, PluginTrustTier, SandboxFilesystemMode,
    SandboxNetworkMode,
};

// Confidence
pub use confidence::ConfidenceLevel;

// Control
pub use control::{
    ActionCandidate as ControlActionCandidate, Conflict as ConflictSignal,
    Decision as ControlDecision, Impasse, ImpasseKind,
    RejectedAlternative as ControlRejectedAlternative, Signal as ControlSignal, Subgoal,
};

// Reasoning
pub use reasoning::{
    EvidenceStrength, ReasoningChain, ReasoningMode, ReasoningStep, ReasoningStepType,
};

// Causal
pub use causal::{CausalChain, CausalLink, CausalRelation};

// Session
pub use session::SessionMetadata;

// Shared tasks
pub use shared_task::{
    AggregationStrategy, SharedTask, SharedTaskStatus, SharedTaskTransitionError, TaskAssignment,
};

// Skills
pub use skills::{
    ExecutionMode, InvocationTrigger, SkillActivation, SkillExecutionTrace, SkillInvocation,
    SkillManifest, SkillMetadata, SkillParameter, SkillSource, SkillSummary, SkillTraceStatus,
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

// Secret dataflow
pub use secret::{SecretHandle, SecretReference, SecretSink, SecretSinkDecision, SecretSinkPolicy};

// Audit
pub use audit::{AuditSummary, AuditTimeRange, DecisionPath, DecisionPathStep};

// Trace
pub use config::TraceLevel;

// Model routing
pub use model_routing::{
    ModelCapability, ModelCapabilityRegistry, ModelFallbackReason, ModelHealth, ModelProfile,
    ModelRouteCandidate, ModelRouteDecision, ModelRouteIntent, ModelRouteRequest, ModelRouteTarget,
};

// Plugin
pub use plugin::{
    NativeLibConfig, NativePluginIsolation, PluginCapabilities, PluginManifest, PluginType,
    PluginVersionCheck, ProcessToolConfig, check_plugin_version,
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
    AdmissionDisposition as WorkspaceAdmissionDisposition,
    AdmissionOutcome as WorkspaceAdmissionOutcome, AdmissionPolicy as WorkspaceAdmissionPolicy,
    Budget as WorkspaceBudget, EvictionRecord as WorkspaceEvictionRecord, Frame as WorkspaceFrame,
    FrameError, Item as WorkspaceItem, ItemKind as WorkspaceItemKind, Lane as WorkspaceLane,
    Taint as WorkspaceTaint, Volatility as WorkspaceVolatility,
};
