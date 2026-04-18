use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::id::{CorrelationId, EventId, TurnId};

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

    // Context (2)
    ContextPressureObserved {
        level: String,
        occupancy: f64,
    },
    ContextCompacted {
        original_tokens: usize,
        compressed_tokens: usize,
    },

    // Metacognition (4)
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

    // Attention channels (3)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::id::{CorrelationId, TurnId};

    fn core_event_payload_variants() -> Vec<Payload> {
        vec![
            Payload::TurnStarted,
            Payload::TurnCompleted,
            Payload::TurnInterrupted,
            Payload::SessionStarted {
                session_id: String::new(),
            },
            Payload::SessionEnded {
                session_id: String::new(),
            },
            Payload::UserMessage {
                content: String::new(),
            },
            Payload::AssistantMessage {
                content: String::new(),
            },
            Payload::ToolInvocationIntent {
                tool_name: String::new(),
                input: String::new(),
            },
            Payload::ToolInvocationResult {
                tool_name: String::new(),
                output: String::new(),
                is_error: false,
            },
            Payload::PermissionRequested {
                tool_name: String::new(),
                risk_level: String::new(),
            },
            Payload::PermissionGranted {
                tool_name: String::new(),
            },
            Payload::PermissionDenied {
                tool_name: String::new(),
                reason: String::new(),
            },
            Payload::ContextPressureObserved {
                level: String::new(),
                occupancy: 0.0,
            },
            Payload::ContextCompacted {
                original_tokens: 0,
                compressed_tokens: 0,
            },
            Payload::ImpasseDetected {
                detector: String::new(),
                details: String::new(),
            },
            Payload::ConflictDetected {
                description: String::new(),
            },
            Payload::MetaControlApplied {
                action: String::new(),
            },
            Payload::FrameCheckResult {
                signals: vec![],
                level: String::new(),
                confidence_score: 0.0,
            },
            Payload::GoalSet {
                level: String::new(),
                description: String::new(),
            },
            Payload::GoalShifted {
                from: String::new(),
                to: String::new(),
            },
            Payload::GoalCompleted {
                level: String::new(),
                description: String::new(),
            },
            Payload::MemoryCaptured {
                memory_id: String::new(),
                memory_type: String::new(),
            },
            Payload::MemoryMaterialized {
                memory_id: String::new(),
            },
            Payload::MemoryStabilized {
                memory_id: String::new(),
            },
        ]
    }

    fn extended_event_payload_variants_part1() -> Vec<Payload> {
        vec![
            Payload::LlmCallCompleted {
                input_tokens: 0,
                output_tokens: 0,
                model: String::new(),
                estimated_cost_usd: 0.0,
            },
            Payload::WorkingMemoryItemActivated {
                tag: String::new(),
                relevance: 0.0,
            },
            Payload::WorkingMemoryItemRehearsed {
                tag: String::new(),
                new_relevance: 0.0,
            },
            Payload::WorkingMemoryItemEvicted {
                tag: String::new(),
                reason: String::new(),
            },
            Payload::WorkingMemoryCapacityExceeded {
                current_count: 0,
                capacity: 0,
            },
            Payload::ChannelScheduled {
                channel: String::new(),
                task_count: 0,
            },
            Payload::MaintenanceExecuted {
                task_name: String::new(),
            },
            Payload::EmergencyTriggered {
                task_name: String::new(),
                details: String::new(),
            },
            Payload::ConfidenceAssessed {
                level: String::new(),
                score: 0.0,
                evidence_count: 0,
            },
            Payload::ConfidenceLow {
                score: 0.0,
                suggestion: String::new(),
            },
            Payload::PressureResponseApplied {
                level: String::new(),
                actions: vec![],
            },
            Payload::AcpClientSpawned {
                command: String::new(),
                agent_id: String::new(),
            },
            Payload::AcpClientResponse {
                agent_id: String::new(),
                response_len: 0,
            },
            Payload::AgentWorkerSpawned {
                worker_name: String::new(),
            },
            Payload::AgentWorkerCompleted {
                worker_name: String::new(),
                result_len: 0,
                input_tokens: 0,
                output_tokens: 0,
            },
            Payload::DelegationCompleted {
                task_count: 0,
                summary: String::new(),
            },
            Payload::PromptUpdated {
                layer: String::new(),
            },
            Payload::TaskDecomposed {
                parent_id: "task-1".into(),
                sub_task_count: 3,
            },
            Payload::TaskAggregated {
                parent_id: "task-1".into(),
                completed_count: 3,
                strategy: "Concatenate".into(),
            },
            Payload::TaskClaimed {
                task_id: "task-1".into(),
                instance_id: "worker-01".into(),
            },
        ]
    }

    fn extended_event_payload_variants_part2() -> Vec<Payload> {
        vec![
            Payload::CausalAnalysisCompleted {
                chain_count: 1,
                root_causes: vec!["PressureObserved".into()],
                total_links: 2,
            },
            Payload::ReasoningStarted {
                mode: "CoT".into(),
                input_summary: String::new(),
            },
            Payload::ReasoningStepCompleted {
                step_index: 0,
                step_type: "Inference".into(),
                confidence: 0.8,
            },
            Payload::ReasoningBranchEvaluated {
                branch_id: "a".into(),
                score: 0.9,
                selected: true,
            },
            Payload::ReasoningChainCompleted {
                chain_id: "chain-1".into(),
                mode: "CoT".into(),
                step_count: 3,
                overall_confidence: 0.85,
                conclusion_summary: "done".into(),
            },
            Payload::WorkflowSpecLoaded {
                source: "meta/workflow.md".into(),
                verify_command_count: 3,
                has_commit_convention: true,
            },
            Payload::EmbeddingModelSwitched {
                from_model: "nomic-embed-text".into(),
                to_model: "bge-large-en".into(),
                precision_improvement: 0.15,
            },
        ]
    }

    fn extended_event_payload_variants_part3() -> Vec<Payload> {
        vec![
            Payload::PluginLoaded {
                name: "test-plugin".into(),
                version: "1.0.0".into(),
                plugin_type: "Tool".into(),
            },
            Payload::AlertFired {
                rule_name: "low_confidence".into(),
                metric_name: "avg_confidence".into(),
                threshold: 0.3,
                current_value: 0.2,
            },
            Payload::SecuritySanitized { redacted_count: 1 },
            Payload::ConfigValidated {
                warning_count: 0,
                health_score: 1.0,
            },
            Payload::PluginDiscovered {
                name: "remote-plugin".into(),
                version: "0.2.0".into(),
                source_url: "https://example.com/manifest.json".into(),
            },
            Payload::AuditQueryExecuted {
                query_type: "summary".into(),
                result_count: 42,
            },
            Payload::HealthAutoRecoveryTriggered {
                dimension: "memory_fragmentation".into(),
                score: 0.85,
                action: "ConsolidateMemory".into(),
            },
            Payload::MemorySplit {
                parent_id: "mem-1".into(),
                child_count: 3,
                reason: "multi_topic".into(),
            },
            Payload::MemoryGraphHealthAssessed {
                score: 0.75,
                orphan_ratio: 0.1,
                avg_degree: 2.5,
                largest_component_ratio: 0.8,
                dead_link_count: 0,
            },
            Payload::MemoryRelationReorganized {
                dead_links_removed: 2,
                duplicate_relations_found: 0,
            },
            Payload::ReasoningReflection {
                chain_id: "chain-1".into(),
                quality_score: 0.75,
                weaknesses: vec![],
            },
            Payload::CausalRetrospect {
                chain_count: 1,
                longest_chain_summary: "A -> B".into(),
                root_causes: vec!["A".into()],
            },
            Payload::SnapshotCreated { offset: 0 },
            Payload::ProjectionCheckpoint {
                projection_name: String::new(),
                offset: 0,
            },
            Payload::SelfModification {
                file_path: String::new(),
                reason: String::new(),
                success: true,
                error_message: None,
            },
            Payload::SideEffectRecorded {
                kind: SideEffectKind::LlmResponse,
                key: String::new(),
                value: String::new(),
            },
            Payload::ExternalizedPayload {
                hash: String::new(),
                size: 0,
                original_type: String::new(),
            },
            Payload::QualityCheckSuggested {
                modifying_tools: Vec::new(),
            },
            Payload::ExplorationTriggered {
                tool_name: String::new(),
                bonus: 0.0,
            },
            Payload::MaintenanceCycleCompleted {
                upgraded: 0,
                deprecated: 0,
                elapsed_ms: 0,
            },
        ]
    }

    #[test]
    fn event_payload_variant_count() {
        let mut variants = core_event_payload_variants();
        variants.extend(extended_event_payload_variants_part1());
        variants.extend(extended_event_payload_variants_part2());
        variants.extend(extended_event_payload_variants_part3());
        assert_eq!(variants.len(), 71);
    }

    #[test]
    fn event_payload_msgpack_roundtrip() {
        let payload = Payload::ToolInvocationIntent {
            tool_name: "read".into(),
            input: r#"{"file_path":"test.rs"}"#.into(),
        };
        let bytes = rmp_serde::to_vec(&payload).unwrap();
        let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(payload, back);
    }

    #[test]
    fn working_memory_payload_msgpack_roundtrip() {
        let variants = vec![
            Payload::WorkingMemoryItemActivated {
                tag: "read".into(),
                relevance: 0.8,
            },
            Payload::WorkingMemoryItemRehearsed {
                tag: "write".into(),
                new_relevance: 0.95,
            },
            Payload::WorkingMemoryItemEvicted {
                tag: "old_tool".into(),
                reason: "capacity_overflow".into(),
            },
            Payload::WorkingMemoryCapacityExceeded {
                current_count: 5,
                capacity: 5,
            },
        ];
        for v in &variants {
            let bytes = rmp_serde::to_vec(v).unwrap();
            let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn attention_channel_payload_msgpack_roundtrip() {
        let variants = vec![
            Payload::ChannelScheduled {
                channel: "maintenance".into(),
                task_count: 2,
            },
            Payload::MaintenanceExecuted {
                task_name: "meta_check".into(),
            },
            Payload::EmergencyTriggered {
                task_name: "pressure_check".into(),
                details: "level=Compress".into(),
            },
        ];
        for v in &variants {
            let bytes = rmp_serde::to_vec(v).unwrap();
            let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn confidence_payload_msgpack_roundtrip() {
        let variants = vec![
            Payload::ConfidenceAssessed {
                level: "high".into(),
                score: 0.85,
                evidence_count: 5,
            },
            Payload::ConfidenceLow {
                score: 0.15,
                suggestion: "consider additional verification".into(),
            },
        ];
        for v in &variants {
            let bytes = rmp_serde::to_vec(v).unwrap();
            let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn pressure_response_payload_msgpack_roundtrip() {
        let v = Payload::PressureResponseApplied {
            level: "Urgent".into(),
            actions: vec!["accelerate_decay".into(), "compress_history".into()],
        };
        let bytes = rmp_serde::to_vec(&v).unwrap();
        let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn acp_client_payload_msgpack_roundtrip() {
        let variants = vec![
            Payload::AcpClientSpawned {
                command: "claude".into(),
                agent_id: "claude-code".into(),
            },
            Payload::AcpClientResponse {
                agent_id: "claude-code".into(),
                response_len: 1024,
            },
        ];
        for v in &variants {
            let bytes = rmp_serde::to_vec(v).unwrap();
            let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn agent_worker_payload_msgpack_roundtrip() {
        let variants = vec![
            Payload::AgentWorkerSpawned {
                worker_name: "reviewer".into(),
            },
            Payload::AgentWorkerCompleted {
                worker_name: "reviewer".into(),
                result_len: 500,
                input_tokens: 0,
                output_tokens: 0,
            },
        ];
        for v in &variants {
            let bytes = rmp_serde::to_vec(v).unwrap();
            let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
            assert_eq!(*v, back);
        }
    }

    #[test]
    fn session_ended_payload_msgpack_roundtrip() {
        let v = Payload::SessionEnded {
            session_id: "test-session-123".into(),
        };
        let bytes = rmp_serde::to_vec(&v).unwrap();
        let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn frame_check_result_payload_msgpack_roundtrip() {
        let v = Payload::FrameCheckResult {
            signals: vec!["tool_monotony".into(), "goal_stagnation".into()],
            level: "Medium".into(),
            confidence_score: 0.35,
        };
        let bytes = rmp_serde::to_vec(&v).unwrap();
        let back: Payload = rmp_serde::from_slice(&bytes).unwrap();
        assert_eq!(v, back);
    }

    #[test]
    fn event_new_has_timestamp() {
        let turn_id = TurnId::new();
        let corr_id = CorrelationId::new();
        let event = Event::new(turn_id, corr_id, Payload::TurnStarted);
        assert!(event.timestamp <= Utc::now());
    }
}
