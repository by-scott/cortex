use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::control;
use crate::id::{CorrelationId, EventId, TurnId};
use crate::message::Message;
use crate::retrieval;
use crate::workspace;

/// Current execution version — incremented on event schema changes.
/// Current execution version — derived from Cargo.toml workspace version.
pub const EXECUTION_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: EventId,
    pub turn_id: TurnId,
    pub correlation_id: CorrelationId,
    pub timestamp: DateTime<Utc>,
    pub payload: Payload,
    /// Execution version at the time this event was created.
    /// Empty string for events written before versioning was introduced.
    #[serde(default)]
    pub execution_version: String,
}

impl Event {
    #[must_use]
    pub fn new(turn_id: TurnId, correlation_id: CorrelationId, payload: Payload) -> Self {
        Self {
            id: EventId::new(),
            turn_id,
            correlation_id,
            timestamp: Utc::now(),
            payload,
            execution_version: EXECUTION_VERSION.to_owned(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum Payload {
    // Turn lifecycle (3)
    TurnStarted,
    TurnCompleted,
    TurnInterrupted,

    // Session (2)
    SessionStarted {
        session_id: String,
    },
    SessionEnded {
        session_id: String,
    },

    // Messages (2)
    UserMessage {
        content: String,
    },
    AssistantMessage {
        content: String,
    },

    // Tool intent/result pair (2)
    ToolInvocationIntent {
        tool_name: String,
        input: String,
    },
    ToolInvocationResult {
        tool_name: String,
        output: String,
        is_error: bool,
    },

    // Permission (3)
    PermissionRequested {
        tool_name: String,
        risk_level: String,
    },
    PermissionGranted {
        tool_name: String,
    },
    PermissionDenied {
        tool_name: String,
        reason: String,
    },

    // Context (4)
    ContextPressureObserved {
        level: String,
        occupancy: f64,
    },
    ContextCompacted {
        original_tokens: usize,
        compressed_tokens: usize,
    },
    ContextCompactBoundary {
        original_tokens: usize,
        compressed_tokens: usize,
        preserved_user_messages: usize,
        suffix_messages: usize,
        summary: String,
        replacement_messages: Vec<Message>,
    },
    WorkspaceFrameAssembled {
        frame: Box<workspace::Frame>,
    },
    WorkspaceItemPromoted {
        item: Box<workspace::Item>,
    },

    // Metacognition (6)
    ImpasseDetected {
        detector: String,
        details: String,
    },
    ConflictDetected {
        description: String,
    },
    MetaControlApplied {
        action: String,
    },
    FrameCheckResult {
        signals: Vec<String>,
        level: String,
        #[serde(default)]
        confidence_score: f64,
    },
    ControlDecisionRecorded {
        decision: control::Decision,
    },
    ImpasseRecorded {
        impasse: control::Impasse,
    },

    // Retrieval and RAG (3)
    RetrievalDecisionRecorded {
        decision: retrieval::Decision,
    },
    EvidenceRetrieved {
        evidence: Box<retrieval::Evidence>,
    },
    EvidencePromoted {
        evidence_id: String,
        frame_item_id: String,
    },

    // Goals (3)
    GoalSet {
        level: String,
        description: String,
    },
    GoalShifted {
        from: String,
        to: String,
    },
    GoalCompleted {
        level: String,
        description: String,
    },

    // Memory (3)
    MemoryCaptured {
        memory_id: String,
        memory_type: String,
    },
    MemoryMaterialized {
        memory_id: String,
    },
    MemoryStabilized {
        memory_id: String,
    },

    // Cost tracking (1)
    LlmCallCompleted {
        input_tokens: usize,
        output_tokens: usize,
        model: String,
        estimated_cost_usd: f64,
    },

    // Working memory (4)
    WorkingMemoryItemActivated {
        tag: String,
        relevance: f64,
    },
    WorkingMemoryItemRehearsed {
        tag: String,
        new_relevance: f64,
    },
    WorkingMemoryItemEvicted {
        tag: String,
        reason: String,
    },
    WorkingMemoryCapacityExceeded {
        current_count: usize,
        capacity: usize,
    },

    // Attention and external input surface (5)
    ChannelScheduled {
        channel: String,
        task_count: usize,
    },
    MaintenanceExecuted {
        task_name: String,
    },
    EmergencyTriggered {
        task_name: String,
        details: String,
    },
    GuardrailTriggered {
        category: String,
        reason: String,
        source: String,
    },
    ExternalInputObserved {
        source: String,
        trust: String,
        summary: String,
    },

    // Decision confidence (2)
    ConfidenceAssessed {
        level: String,
        score: f64,
        evidence_count: usize,
    },
    ConfidenceLow {
        score: f64,
        suggestion: String,
    },

    // Pressure response (1)
    PressureResponseApplied {
        level: String,
        actions: Vec<String>,
    },

    // ACP Client (2)
    AcpClientSpawned {
        command: String,
        agent_id: String,
    },
    AcpClientResponse {
        agent_id: String,
        response_len: usize,
    },

    // Concurrent agents (2)
    AgentWorkerSpawned {
        worker_name: String,
    },
    AgentWorkerCompleted {
        worker_name: String,
        result_len: usize,
        input_tokens: usize,
        output_tokens: usize,
    },

    // Delegation (1)
    DelegationCompleted {
        task_count: usize,
        summary: String,
    },

    // Prompt evolution (1)
    PromptUpdated {
        layer: String,
    },

    // Reasoning chain (4)
    ReasoningStarted {
        mode: String,
        input_summary: String,
    },
    ReasoningStepCompleted {
        step_index: usize,
        step_type: String,
        confidence: f64,
    },
    ReasoningBranchEvaluated {
        branch_id: String,
        score: f64,
        selected: bool,
    },
    ReasoningChainCompleted {
        chain_id: String,
        mode: String,
        step_count: usize,
        overall_confidence: f64,
        conclusion_summary: String,
    },

    // Multi-instance tasks (2)
    TaskDecomposed {
        parent_id: String,
        sub_task_count: usize,
    },
    TaskAggregated {
        parent_id: String,
        completed_count: usize,
        strategy: String,
    },

    // Task discovery (1)
    TaskClaimed {
        task_id: String,
        instance_id: String,
    },

    // Workflow awareness (1)
    WorkflowSpecLoaded {
        source: String,
        verify_command_count: usize,
        has_commit_convention: bool,
    },

    // Causal analysis (1)
    CausalAnalysisCompleted {
        chain_count: usize,
        root_causes: Vec<String>,
        total_links: usize,
    },

    // Embedding auto-switch (1)
    EmbeddingModelSwitched {
        from_model: String,
        to_model: String,
        precision_improvement: f64,
    },

    // Embedding health (1)
    EmbeddingDegraded {
        reason: String,
    },

    // Skills (2)
    SkillInvoked {
        name: String,
        trigger: String,
        execution_mode: String,
    },
    SkillCompleted {
        name: String,
        duration_ms: u64,
        success: bool,
    },

    // Plugin ecosystem (1)
    PluginLoaded {
        name: String,
        version: String,
        plugin_type: String,
    },

    // Audit dashboard (1)
    AuditQueryExecuted {
        query_type: String,
        result_count: usize,
    },

    // Health auto-recovery (1)
    HealthAutoRecoveryTriggered {
        dimension: String,
        score: f64,
        action: String,
    },

    // Observability (1)
    AlertFired {
        rule_name: String,
        metric_name: String,
        threshold: f64,
        current_value: f64,
    },

    // Security (1)
    SecuritySanitized {
        redacted_count: usize,
    },

    // Config validation (1)
    ConfigValidated {
        warning_count: usize,
        health_score: f64,
    },

    // Plugin discovery (1)
    PluginDiscovered {
        name: String,
        version: String,
        source_url: String,
    },

    // Memory evolution (3)
    MemorySplit {
        parent_id: String,
        child_count: usize,
        reason: String,
    },
    MemoryGraphHealthAssessed {
        score: f64,
        orphan_ratio: f64,
        avg_degree: f64,
        largest_component_ratio: f64,
        dead_link_count: usize,
    },
    MemoryRelationReorganized {
        dead_links_removed: usize,
        duplicate_relations_found: usize,
    },

    // DMN reasoning reflection (1)
    ReasoningReflection {
        chain_id: String,
        quality_score: f64,
        weaknesses: Vec<String>,
    },

    // DMN causal retrospect (1)
    CausalRetrospect {
        chain_count: usize,
        longest_chain_summary: String,
        root_causes: Vec<String>,
    },

    // Infrastructure (2)
    SnapshotCreated {
        offset: u64,
    },
    ProjectionCheckpoint {
        projection_name: String,
        offset: u64,
    },

    // Self-evolution (1)
    SelfModification {
        file_path: String,
        reason: String,
        success: bool,
        error_message: Option<String>,
    },

    // Deterministic replay (1)
    SideEffectRecorded {
        kind: SideEffectKind,
        key: String,
        value: String,
    },

    // Journal externalization (1)
    ExternalizedPayload {
        hash: String,
        size: usize,
        original_type: String,
    },

    // Quality (1)
    QualityCheckSuggested {
        modifying_tools: Vec<String>,
    },

    // Exploration (1)
    ExplorationTriggered {
        tool_name: String,
        bonus: f64,
    },

    // Maintenance (1)
    MaintenanceCycleCompleted {
        upgraded: usize,
        deprecated: usize,
        elapsed_ms: u64,
    },
}

/// Classification of non-deterministic inputs for deterministic replay.
///
/// Every external input that cannot be reproduced by replaying the event
/// stream must be recorded as a [`Payload::SideEffectRecorded`] event with
/// one of these kinds so that the replay engine can substitute the recorded
/// value instead of re-executing the call.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum SideEffectKind {
    /// LLM response content (largest non-determinism source).
    LlmResponse,
    /// Wall-clock timestamp captured at a decision point.
    WallClock,
    /// Result from an external I/O operation (HTTP, file-system, etc.).
    ExternalIo,
    /// Random value (seed, sampling, etc.).
    Random,
}
